//! Windows-facing capture, OCR, and automation adapters.

use std::{
    io::Cursor,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use image::{DynamicImage, ImageFormat, imageops::FilterType};
use screensearch_domain::{BoundingBox, CapturedFrame, OcrBlock};
use screensearch_ports::{
    ApprovedAutomationAction, AutomationExecutor, CaptureSource, OcrEngine, PortError,
};
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
                text,
                confidence: None,
                language: language.clone(),
            });
        }
        Ok(blocks)
    })
}

#[allow(clippy::needless_pass_by_value)]
fn ocr_error(error: windows::core::Error) -> PortError {
    PortError::Internal(format!("Windows OCR: {error}"))
}

/// Reads the foreground window without coupling policy tests to Win32 globals.
pub trait ForegroundWindowReader: Send + Sync {
    /// Returns the current foreground window label.
    fn current_window(&self) -> Result<String, PortError>;
}

/// Fixed foreground-window adapter for deterministic tests and bootstrap wiring.
pub struct FixedForegroundWindow(pub String);

impl ForegroundWindowReader for FixedForegroundWindow {
    fn current_window(&self) -> Result<String, PortError> {
        Ok(self.0.clone())
    }
}

/// Enforces approval, foreground-window, and emergency-abort policy before native input.
pub struct GuardedWindowsAutomation {
    foreground: Arc<dyn ForegroundWindowReader>,
    emergency_abort: Arc<AtomicBool>,
}

impl GuardedWindowsAutomation {
    /// Creates a guarded executor around a foreground-window adapter.
    pub fn new(
        foreground: Arc<dyn ForegroundWindowReader>,
        emergency_abort: Arc<AtomicBool>,
    ) -> Self {
        Self {
            foreground,
            emergency_abort,
        }
    }

    /// Activates the process-wide emergency stop.
    pub fn abort(&self) {
        self.emergency_abort.store(true, Ordering::SeqCst);
    }

    /// Clears the stop after an explicit user reset.
    pub fn reset_abort(&self) {
        self.emergency_abort.store(false, Ordering::SeqCst);
    }
}

#[async_trait]
impl AutomationExecutor for GuardedWindowsAutomation {
    async fn execute(&self, action: &ApprovedAutomationAction) -> Result<(), PortError> {
        if action.approval_id.trim().is_empty() {
            return Err(PortError::Denied("missing approval record".to_owned()));
        }
        if self.emergency_abort.load(Ordering::SeqCst) {
            return Err(PortError::Denied("emergency abort is active".to_owned()));
        }
        let foreground = self.foreground.current_window()?;
        if foreground != action.expected_window {
            return Err(PortError::Denied(format!(
                "foreground window changed from '{}' to '{foreground}'",
                action.expected_window
            )));
        }
        if action.action.trim().is_empty() {
            return Err(PortError::InvalidData(
                "automation action is empty".to_owned(),
            ));
        }

        // A production adapter will translate the validated action to UI Automation or
        // SendInput here. The bootstrap intentionally proves policy without emitting input.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use screensearch_ports::{ApprovedAutomationAction, AutomationExecutor, PortError};

    use super::{FixedForegroundWindow, GuardedWindowsAutomation};

    fn action() -> ApprovedAutomationAction {
        ApprovedAutomationAction {
            approval_id: "approval-1".to_owned(),
            expected_window: "Calculator".to_owned(),
            action: "press Enter".to_owned(),
        }
    }

    #[tokio::test]
    async fn approved_action_requires_the_expected_foreground_window() {
        let executor = GuardedWindowsAutomation::new(
            Arc::new(FixedForegroundWindow("Notepad".to_owned())),
            Arc::new(AtomicBool::new(false)),
        );

        assert!(matches!(
            executor.execute(&action()).await,
            Err(PortError::Denied(_))
        ));
    }

    #[tokio::test]
    async fn emergency_abort_blocks_an_otherwise_valid_action() {
        let executor = GuardedWindowsAutomation::new(
            Arc::new(FixedForegroundWindow("Calculator".to_owned())),
            Arc::new(AtomicBool::new(false)),
        );
        executor.abort();

        assert!(matches!(
            executor.execute(&action()).await,
            Err(PortError::Denied(_))
        ));
    }

    #[tokio::test]
    async fn valid_approved_action_passes_all_policy_gates() {
        let executor = GuardedWindowsAutomation::new(
            Arc::new(FixedForegroundWindow("Calculator".to_owned())),
            Arc::new(AtomicBool::new(false)),
        );

        assert!(executor.execute(&action()).await.is_ok());
    }
}
