//! Windows-facing capture, OCR, and automation adapters.

mod automation;
mod memory_pressure;

pub use automation::WindowsAutomationPlatform;
pub use memory_pressure::MemoryPressureMonitor;

#[cfg(test)]
use automation::{
    encode_key_chord_inputs, encode_text_inputs, keyboard_event, target_identity_matches,
    validate_send_input_count,
};

use std::{
    io::Cursor,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use image::{DynamicImage, ImageFormat, imageops::FilterType};
use screensearch_domain::{BoundingBox, CapturedFrame, OcrBlock};
use screensearch_ports::{CaptureSource, OcrEngine, PortError};
use unicode_normalization::UnicodeNormalization;
use windows::{
    Graphics::Imaging::BitmapDecoder,
    Media::Ocr::OcrEngine as WinOcrEngine,
    Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
};
use xcap::{Monitor, Window};

const MAX_CAPTURE_DIMENSION: u32 = 2_600;

/// Deterministic frame source retained for contract and integration tests.
#[derive(Default)]
pub struct FakeWindowsCaptureSource {
    sequence: AtomicU64,
}

#[async_trait]
impl CaptureSource for FakeWindowsCaptureSource {
    async fn capture(&self) -> Result<CapturedFrame, PortError> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        Ok(CapturedFrame {
            captured_at: chrono::Utc::now(),
            monitor_id: "windows-primary".to_owned(),
            application: "screensearch-bootstrap.exe".to_owned(),
            window_title: "ScreenSearch V2 bootstrap".to_owned(),
            width: 1,
            height: 1,
            bytes: format!("fake-windows-frame-{sequence}").into_bytes(),
            media_type: "application/octet-stream".to_owned(),
        })
    }
}

/// Captures the monitor containing the focused window and encodes it as PNG.
#[derive(Default)]
pub struct WindowsGraphicsCaptureSource;

#[async_trait]
impl CaptureSource for WindowsGraphicsCaptureSource {
    async fn capture(&self) -> Result<CapturedFrame, PortError> {
        tokio::task::spawn_blocking(capture_focused_monitor)
            .await
            .map_err(|error| PortError::Internal(format!("capture task failed: {error}")))?
    }
}

fn capture_focused_monitor() -> Result<CapturedFrame, PortError> {
    let focused = Window::all()
        .map_err(capture_error)?
        .into_iter()
        .find(|window| window.is_focused().unwrap_or(false));

    let (application, window_title, monitor) = if let Some(window) = focused {
        let application = window.app_name().unwrap_or_else(|_| "unknown".to_owned());
        let window_title = window.title().unwrap_or_default();
        let monitor = window.current_monitor().map_err(capture_error)?;
        (application, window_title, monitor)
    } else {
        let monitors = Monitor::all().map_err(capture_error)?;
        let monitor = monitors
            .iter()
            .find(|monitor| monitor.is_primary().unwrap_or(false))
            .or_else(|| monitors.first())
            .cloned()
            .ok_or_else(|| PortError::Unavailable("Windows reported no monitors".to_owned()))?;
        ("unknown".to_owned(), String::new(), monitor)
    };

    let monitor_id = monitor.id().map_or_else(
        |_| "windows-monitor-unknown".to_owned(),
        |id| format!("windows-monitor-{id}"),
    );
    let mut image = DynamicImage::ImageRgba8(monitor.capture_image().map_err(capture_error)?);
    if image.width().max(image.height()) > MAX_CAPTURE_DIMENSION {
        image = image.resize(
            MAX_CAPTURE_DIMENSION,
            MAX_CAPTURE_DIMENSION,
            FilterType::Triangle,
        );
    }
    let width = image.width();
    let height = image.height();
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|error| PortError::Internal(format!("encode PNG capture: {error}")))?;

    Ok(CapturedFrame {
        captured_at: chrono::Utc::now(),
        monitor_id,
        application,
        window_title,
        width,
        height,
        bytes: encoded.into_inner(),
        media_type: "image/png".to_owned(),
    })
}

#[allow(clippy::needless_pass_by_value)]
fn capture_error(error: xcap::XCapError) -> PortError {
    PortError::Transient(format!("Windows screen capture: {error}"))
}

/// Offline OCR adapter backed by the language packs installed in Windows.
#[derive(Clone, Debug)]
pub struct WindowsOcrEngine {
    asset_root: PathBuf,
}

impl WindowsOcrEngine {
    /// Creates an OCR adapter for content-addressed assets below `asset_root`.
    pub fn new(asset_root: impl Into<PathBuf>) -> Self {
        Self {
            asset_root: asset_root.into(),
        }
    }

    fn resolve(&self, relative_path: &str) -> Result<PathBuf, PortError> {
        let relative = Path::new(relative_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(PortError::InvalidData(
                "OCR asset path escapes the configured root".to_owned(),
            ));
        }
        Ok(self.asset_root.join(relative))
    }
}

#[async_trait]
impl OcrEngine for WindowsOcrEngine {
    fn model_id(&self) -> &'static str {
        "windows-media-ocr-user-profile-v1"
    }

    async fn recognize(
        &self,
        asset: &screensearch_domain::AssetRef,
    ) -> Result<Vec<OcrBlock>, PortError> {
        if asset.media_type != "image/png" {
            return Err(PortError::InvalidData(format!(
                "Windows OCR requires image/png, received {}",
                asset.media_type
            )));
        }
        let path = self.resolve(&asset.relative_path)?;
        tokio::task::spawn_blocking(move || recognize_image(&path))
            .await
            .map_err(|error| PortError::Internal(format!("OCR task failed: {error}")))?
    }
}

#[allow(clippy::cast_precision_loss)]
fn recognize_image(path: &Path) -> Result<Vec<OcrBlock>, PortError> {
    let encoded = std::fs::read(path)
        .map_err(|error| PortError::Internal(format!("read OCR image: {error}")))?;
    futures::executor::block_on(async move {
        let stream = InMemoryRandomAccessStream::new().map_err(ocr_error)?;
        let writer = DataWriter::CreateDataWriter(&stream).map_err(ocr_error)?;
        writer.WriteBytes(&encoded).map_err(ocr_error)?;
        writer
            .StoreAsync()
            .map_err(ocr_error)?
            .await
            .map_err(ocr_error)?;
        writer.DetachStream().map_err(ocr_error)?;
        stream.Seek(0).map_err(ocr_error)?;
        let decoder = BitmapDecoder::CreateAsync(&stream)
            .map_err(ocr_error)?
            .await
            .map_err(ocr_error)?;
        let width = decoder.PixelWidth().map_err(ocr_error)? as f32;
        let height = decoder.PixelHeight().map_err(ocr_error)? as f32;
        if width <= 0.0 || height <= 0.0 {
            return Err(PortError::InvalidData(
                "OCR image has invalid dimensions".to_owned(),
            ));
        }
        let bitmap = decoder
            .GetSoftwareBitmapAsync()
            .map_err(ocr_error)?
            .await
            .map_err(ocr_error)?;
        let engine = WinOcrEngine::TryCreateFromUserProfileLanguages().map_err(ocr_error)?;
        let language = engine
            .RecognizerLanguage()
            .and_then(|value| value.LanguageTag())
            .ok()
            .map(|value| value.to_string());
        let result = engine
            .RecognizeAsync(&bitmap)
            .map_err(ocr_error)?
            .await
            .map_err(ocr_error)?;
        let lines = result.Lines().map_err(ocr_error)?;
        let mut blocks = Vec::with_capacity(lines.Size().map_err(ocr_error)? as usize);
        for index in 0..lines.Size().map_err(ocr_error)? {
            let line = lines.GetAt(index).map_err(ocr_error)?;
            let text = line.Text().map_err(ocr_error)?.to_string();
            if text.trim().is_empty() {
                continue;
            }
            let words = line.Words().map_err(ocr_error)?;
            let mut left = f32::INFINITY;
            let mut top = f32::INFINITY;
            let mut right = 0.0_f32;
            let mut bottom = 0.0_f32;
            for word_index in 0..words.Size().map_err(ocr_error)? {
                let rect = words
                    .GetAt(word_index)
                    .map_err(ocr_error)?
                    .BoundingRect()
                    .map_err(ocr_error)?;
                left = left.min(rect.X);
                top = top.min(rect.Y);
                right = right.max(rect.X + rect.Width);
                bottom = bottom.max(rect.Y + rect.Height);
            }
            if !left.is_finite() || !top.is_finite() {
                continue;
            }
            let bounds = BoundingBox {
                x: (left / width).clamp(0.0, 1.0),
                y: (top / height).clamp(0.0, 1.0),
                width: ((right - left) / width).clamp(0.0, 1.0),
                height: ((bottom - top) / height).clamp(0.0, 1.0),
            }
            .validate()
            .map_err(|error| PortError::InvalidData(error.to_string()))?;
            blocks.push(OcrBlock {
                reading_order: u32::try_from(blocks.len()).unwrap_or(u32::MAX),
                bounds,
                text: normalize_ocr_text(&text),
                confidence: None,
                language: language.clone(),
            });
        }
        Ok(blocks)
    })
}

/// Normalizes OCR line text to a canonical form while preserving human-readable content (spec §7.2).
///
/// Windows Media OCR returns one line per block, but the recognized string may carry platform line
/// endings or decomposed Unicode. This collapses CR/CRLF to `\n` and applies Unicode NFC so the
/// persisted block text, derived FTS terms, and chunk embeddings share one canonical representation.
fn normalize_ocr_text(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    unified.nfc().collect()
}

#[allow(clippy::needless_pass_by_value)]
fn ocr_error(error: windows::core::Error) -> PortError {
    PortError::Internal(format!("Windows OCR: {error}"))
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use screensearch_domain::{
        AutomationFailureCode, AutomationKey, AutomationTarget, KeyModifier,
    };
    use screensearch_ports::PortError;
    use windows::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, KEYEVENTF_UNICODE};

    use super::{
        encode_key_chord_inputs, encode_text_inputs, keyboard_event, normalize_ocr_text,
        target_identity_matches, validate_send_input_count,
    };

    #[test]
    fn ocr_text_is_normalized_to_lf_and_nfc() {
        // CRLF and bare CR collapse to LF without otherwise altering the text.
        assert_eq!(
            normalize_ocr_text("line1\r\nline2\rline3"),
            "line1\nline2\nline3"
        );
        // Decomposed "é" (e + U+0301 combining acute) becomes the composed NFC scalar U+00E9.
        assert_eq!(normalize_ocr_text("e\u{0301}"), "\u{00e9}");
        // Already-canonical ASCII text is returned unchanged.
        assert_eq!(normalize_ocr_text("hello world"), "hello world");
    }

    #[test]
    fn target_identity_requires_exact_hwnd_pid_and_executable() {
        let expected = AutomationTarget {
            process_id: 42,
            window_handle: 9001,
            executable_name: "Fixture.exe".to_owned(),
            display_title: "Before".to_owned(),
        };
        let mut actual = expected.clone();
        actual.display_title = "After".to_owned();
        actual.executable_name = "fixture.EXE".to_owned();
        assert!(target_identity_matches(&actual, &expected));

        actual.window_handle = 7;
        assert!(!target_identity_matches(&actual, &expected));
    }

    #[test]
    fn utf16_text_encoding_emits_key_down_and_up_for_every_code_unit() {
        let inputs = encode_text_inputs("A😀");
        assert_eq!(inputs.len(), 6);
        for pair in inputs.chunks_exact(2) {
            let down = keyboard_event(&pair[0]);
            let up = keyboard_event(&pair[1]);
            assert!(down.dwFlags.contains(KEYEVENTF_UNICODE));
            assert!(!down.dwFlags.contains(KEYEVENTF_KEYUP));
            assert!(up.dwFlags.contains(KEYEVENTF_UNICODE));
            assert!(up.dwFlags.contains(KEYEVENTF_KEYUP));
            assert_eq!(down.wScan, up.wScan);
        }
    }

    #[test]
    fn key_chord_releases_all_modifiers_in_reverse_order() {
        let inputs = encode_key_chord_inputs(
            &[KeyModifier::Control, KeyModifier::Shift],
            AutomationKey::S,
        );
        assert_eq!(inputs.len(), 6);
        let shift_up = keyboard_event(&inputs[4]);
        let control_up = keyboard_event(&inputs[5]);
        assert!(shift_up.dwFlags.contains(KEYEVENTF_KEYUP));
        assert!(control_up.dwFlags.contains(KEYEVENTF_KEYUP));
    }

    #[test]
    fn partial_send_input_is_a_stable_input_blocked_failure() {
        assert_eq!(
            validate_send_input_count(6, 5),
            Err(PortError::Automation(AutomationFailureCode::InputBlocked))
        );
        assert_eq!(validate_send_input_count(6, 6), Ok(()));
    }
}
