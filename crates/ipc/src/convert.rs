//! Centralized conversion between the wire key/modifier enums, the desktop UI string tokens, and
//! the domain key/modifier types.
//!
//! Both binaries that touch guarded-automation keys route through this module: the daemon decodes
//! transmitted wire enums into domain types, and the desktop shell encodes UI string tokens into
//! wire enums. Sharing one module keeps the accepted UI vocabulary from silently drifting from the
//! contract. The token vocabulary itself is single-sourced in `screensearch_domain`
//! ([`AutomationKey::ui_token`]), and the domain <-> wire mappings below are exhaustive matches, so
//! adding a key variant fails to compile until it is handled in every direction.

use screensearch_domain::{AutomationKey, KeyModifier};

use crate::v1;

/// Failure to convert an automation key or modifier between representations.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{0}")]
pub struct AutomationConvertError(pub String);

/// Maps a desktop UI string token (e.g. `"s"`, `"enter"`, `"arrowleft"`) to the wire key enum.
///
/// Accepts the canonical [`AutomationKey::ui_token`] vocabulary case-insensitively, plus the
/// `"esc"` alias for Escape.
pub fn key_from_token(token: &str) -> Result<v1::AutomationKey, AutomationConvertError> {
    let normalized = token.trim().to_ascii_lowercase();
    let normalized = match normalized.as_str() {
        "esc" => "escape",
        other => other,
    };
    AutomationKey::all()
        .into_iter()
        .find(|key| key.ui_token() == normalized)
        .map(domain_key_to_wire)
        .ok_or_else(|| AutomationConvertError(format!("unknown automation key: {token}")))
}

/// Maps a desktop UI string token (`"control"`, `"alt"`, `"shift"`) to the wire modifier enum.
pub fn modifier_from_token(
    token: &str,
) -> Result<v1::AutomationKeyModifier, AutomationConvertError> {
    let normalized = token.trim().to_ascii_lowercase();
    KeyModifier::all()
        .into_iter()
        .find(|modifier| modifier.ui_token() == normalized)
        .map(domain_modifier_to_wire)
        .ok_or_else(|| AutomationConvertError(format!("unknown automation key modifier: {token}")))
}

/// Maps a transmitted wire key value to the domain key, rejecting unknown or unspecified values.
pub fn key_to_domain(value: i32) -> Result<AutomationKey, AutomationConvertError> {
    let wire = v1::AutomationKey::try_from(value)
        .map_err(|_| AutomationConvertError(format!("unknown automation key value: {value}")))?;
    wire_key_to_domain(wire)
        .ok_or_else(|| AutomationConvertError("automation key is unspecified".to_owned()))
}

/// Maps a transmitted wire modifier value to the domain modifier, rejecting unknown/unspecified.
pub fn modifier_to_domain(value: i32) -> Result<KeyModifier, AutomationConvertError> {
    let wire = v1::AutomationKeyModifier::try_from(value).map_err(|_| {
        AutomationConvertError(format!("unknown automation key modifier value: {value}"))
    })?;
    wire_modifier_to_domain(wire)
        .ok_or_else(|| AutomationConvertError("automation key modifier is unspecified".to_owned()))
}

fn domain_modifier_to_wire(modifier: KeyModifier) -> v1::AutomationKeyModifier {
    match modifier {
        KeyModifier::Control => v1::AutomationKeyModifier::Control,
        KeyModifier::Alt => v1::AutomationKeyModifier::Alt,
        KeyModifier::Shift => v1::AutomationKeyModifier::Shift,
    }
}

fn wire_modifier_to_domain(modifier: v1::AutomationKeyModifier) -> Option<KeyModifier> {
    match modifier {
        v1::AutomationKeyModifier::Control => Some(KeyModifier::Control),
        v1::AutomationKeyModifier::Alt => Some(KeyModifier::Alt),
        v1::AutomationKeyModifier::Shift => Some(KeyModifier::Shift),
        v1::AutomationKeyModifier::Unspecified => None,
    }
}

#[allow(clippy::too_many_lines)]
fn domain_key_to_wire(key: AutomationKey) -> v1::AutomationKey {
    match key {
        AutomationKey::A => v1::AutomationKey::A,
        AutomationKey::B => v1::AutomationKey::B,
        AutomationKey::C => v1::AutomationKey::C,
        AutomationKey::D => v1::AutomationKey::D,
        AutomationKey::E => v1::AutomationKey::E,
        AutomationKey::F => v1::AutomationKey::F,
        AutomationKey::G => v1::AutomationKey::G,
        AutomationKey::H => v1::AutomationKey::H,
        AutomationKey::I => v1::AutomationKey::I,
        AutomationKey::J => v1::AutomationKey::J,
        AutomationKey::K => v1::AutomationKey::K,
        AutomationKey::L => v1::AutomationKey::L,
        AutomationKey::M => v1::AutomationKey::M,
        AutomationKey::N => v1::AutomationKey::N,
        AutomationKey::O => v1::AutomationKey::O,
        AutomationKey::P => v1::AutomationKey::P,
        AutomationKey::Q => v1::AutomationKey::Q,
        AutomationKey::R => v1::AutomationKey::R,
        AutomationKey::S => v1::AutomationKey::S,
        AutomationKey::T => v1::AutomationKey::T,
        AutomationKey::U => v1::AutomationKey::U,
        AutomationKey::V => v1::AutomationKey::V,
        AutomationKey::W => v1::AutomationKey::W,
        AutomationKey::X => v1::AutomationKey::X,
        AutomationKey::Y => v1::AutomationKey::Y,
        AutomationKey::Z => v1::AutomationKey::Z,
        AutomationKey::Digit0 => v1::AutomationKey::Digit0,
        AutomationKey::Digit1 => v1::AutomationKey::Digit1,
        AutomationKey::Digit2 => v1::AutomationKey::Digit2,
        AutomationKey::Digit3 => v1::AutomationKey::Digit3,
        AutomationKey::Digit4 => v1::AutomationKey::Digit4,
        AutomationKey::Digit5 => v1::AutomationKey::Digit5,
        AutomationKey::Digit6 => v1::AutomationKey::Digit6,
        AutomationKey::Digit7 => v1::AutomationKey::Digit7,
        AutomationKey::Digit8 => v1::AutomationKey::Digit8,
        AutomationKey::Digit9 => v1::AutomationKey::Digit9,
        AutomationKey::Enter => v1::AutomationKey::Enter,
        AutomationKey::Escape => v1::AutomationKey::Escape,
        AutomationKey::Tab => v1::AutomationKey::Tab,
        AutomationKey::Space => v1::AutomationKey::Space,
        AutomationKey::Backspace => v1::AutomationKey::Backspace,
        AutomationKey::Delete => v1::AutomationKey::Delete,
        AutomationKey::ArrowLeft => v1::AutomationKey::ArrowLeft,
        AutomationKey::ArrowRight => v1::AutomationKey::ArrowRight,
        AutomationKey::ArrowUp => v1::AutomationKey::ArrowUp,
        AutomationKey::ArrowDown => v1::AutomationKey::ArrowDown,
        AutomationKey::Home => v1::AutomationKey::Home,
        AutomationKey::End => v1::AutomationKey::End,
        AutomationKey::F1 => v1::AutomationKey::F1,
        AutomationKey::F2 => v1::AutomationKey::F2,
        AutomationKey::F3 => v1::AutomationKey::F3,
        AutomationKey::F4 => v1::AutomationKey::F4,
        AutomationKey::F5 => v1::AutomationKey::F5,
        AutomationKey::F6 => v1::AutomationKey::F6,
        AutomationKey::F7 => v1::AutomationKey::F7,
        AutomationKey::F8 => v1::AutomationKey::F8,
        AutomationKey::F9 => v1::AutomationKey::F9,
        AutomationKey::F10 => v1::AutomationKey::F10,
        AutomationKey::F11 => v1::AutomationKey::F11,
        AutomationKey::F12 => v1::AutomationKey::F12,
    }
}

#[allow(clippy::too_many_lines)]
fn wire_key_to_domain(key: v1::AutomationKey) -> Option<AutomationKey> {
    match key {
        v1::AutomationKey::Unspecified => None,
        v1::AutomationKey::A => Some(AutomationKey::A),
        v1::AutomationKey::B => Some(AutomationKey::B),
        v1::AutomationKey::C => Some(AutomationKey::C),
        v1::AutomationKey::D => Some(AutomationKey::D),
        v1::AutomationKey::E => Some(AutomationKey::E),
        v1::AutomationKey::F => Some(AutomationKey::F),
        v1::AutomationKey::G => Some(AutomationKey::G),
        v1::AutomationKey::H => Some(AutomationKey::H),
        v1::AutomationKey::I => Some(AutomationKey::I),
        v1::AutomationKey::J => Some(AutomationKey::J),
        v1::AutomationKey::K => Some(AutomationKey::K),
        v1::AutomationKey::L => Some(AutomationKey::L),
        v1::AutomationKey::M => Some(AutomationKey::M),
        v1::AutomationKey::N => Some(AutomationKey::N),
        v1::AutomationKey::O => Some(AutomationKey::O),
        v1::AutomationKey::P => Some(AutomationKey::P),
        v1::AutomationKey::Q => Some(AutomationKey::Q),
        v1::AutomationKey::R => Some(AutomationKey::R),
        v1::AutomationKey::S => Some(AutomationKey::S),
        v1::AutomationKey::T => Some(AutomationKey::T),
        v1::AutomationKey::U => Some(AutomationKey::U),
        v1::AutomationKey::V => Some(AutomationKey::V),
        v1::AutomationKey::W => Some(AutomationKey::W),
        v1::AutomationKey::X => Some(AutomationKey::X),
        v1::AutomationKey::Y => Some(AutomationKey::Y),
        v1::AutomationKey::Z => Some(AutomationKey::Z),
        v1::AutomationKey::Digit0 => Some(AutomationKey::Digit0),
        v1::AutomationKey::Digit1 => Some(AutomationKey::Digit1),
        v1::AutomationKey::Digit2 => Some(AutomationKey::Digit2),
        v1::AutomationKey::Digit3 => Some(AutomationKey::Digit3),
        v1::AutomationKey::Digit4 => Some(AutomationKey::Digit4),
        v1::AutomationKey::Digit5 => Some(AutomationKey::Digit5),
        v1::AutomationKey::Digit6 => Some(AutomationKey::Digit6),
        v1::AutomationKey::Digit7 => Some(AutomationKey::Digit7),
        v1::AutomationKey::Digit8 => Some(AutomationKey::Digit8),
        v1::AutomationKey::Digit9 => Some(AutomationKey::Digit9),
        v1::AutomationKey::Enter => Some(AutomationKey::Enter),
        v1::AutomationKey::Escape => Some(AutomationKey::Escape),
        v1::AutomationKey::Tab => Some(AutomationKey::Tab),
        v1::AutomationKey::Space => Some(AutomationKey::Space),
        v1::AutomationKey::Backspace => Some(AutomationKey::Backspace),
        v1::AutomationKey::Delete => Some(AutomationKey::Delete),
        v1::AutomationKey::ArrowLeft => Some(AutomationKey::ArrowLeft),
        v1::AutomationKey::ArrowRight => Some(AutomationKey::ArrowRight),
        v1::AutomationKey::ArrowUp => Some(AutomationKey::ArrowUp),
        v1::AutomationKey::ArrowDown => Some(AutomationKey::ArrowDown),
        v1::AutomationKey::Home => Some(AutomationKey::Home),
        v1::AutomationKey::End => Some(AutomationKey::End),
        v1::AutomationKey::F1 => Some(AutomationKey::F1),
        v1::AutomationKey::F2 => Some(AutomationKey::F2),
        v1::AutomationKey::F3 => Some(AutomationKey::F3),
        v1::AutomationKey::F4 => Some(AutomationKey::F4),
        v1::AutomationKey::F5 => Some(AutomationKey::F5),
        v1::AutomationKey::F6 => Some(AutomationKey::F6),
        v1::AutomationKey::F7 => Some(AutomationKey::F7),
        v1::AutomationKey::F8 => Some(AutomationKey::F8),
        v1::AutomationKey::F9 => Some(AutomationKey::F9),
        v1::AutomationKey::F10 => Some(AutomationKey::F10),
        v1::AutomationKey::F11 => Some(AutomationKey::F11),
        v1::AutomationKey::F12 => Some(AutomationKey::F12),
    }
}
