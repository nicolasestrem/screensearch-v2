//! Round-trip coverage for the shared automation key/modifier conversion.
//!
//! These tests pin the UI-token -> wire-enum -> domain loop closed over *every* domain variant, so
//! a key added to the domain/contract that is not handled in `convert` fails here instead of
//! silently becoming unproducible from the desktop UI.

use screensearch_domain::{AutomationKey, KeyModifier};
use screensearch_ipc::{convert, v1};

#[test]
fn every_key_round_trips_token_wire_domain() {
    for key in AutomationKey::all() {
        let token = key.ui_token();
        let wire = convert::key_from_token(token)
            .unwrap_or_else(|error| panic!("token {token:?} did not map to a wire key: {error}"));
        let domain = convert::key_to_domain(wire as i32)
            .unwrap_or_else(|error| panic!("wire key for {token:?} did not map back: {error}"));
        assert_eq!(
            domain, key,
            "round trip changed the key for token {token:?}"
        );
    }
}

#[test]
fn every_modifier_round_trips_token_wire_domain() {
    for modifier in KeyModifier::all() {
        let token = modifier.ui_token();
        let wire = convert::modifier_from_token(token)
            .unwrap_or_else(|error| panic!("token {token:?} did not map: {error}"));
        let domain = convert::modifier_to_domain(wire as i32).unwrap_or_else(|error| {
            panic!("wire modifier for {token:?} did not map back: {error}")
        });
        assert_eq!(
            domain, modifier,
            "round trip changed the modifier for token {token:?}"
        );
    }
}

#[test]
fn key_tokens_are_case_insensitive_and_accept_the_esc_alias() {
    assert_eq!(
        convert::key_from_token("  S  ").unwrap(),
        v1::AutomationKey::S
    );
    assert_eq!(
        convert::key_from_token("ESC").unwrap(),
        v1::AutomationKey::Escape
    );
    assert_eq!(
        convert::key_from_token("escape").unwrap(),
        v1::AutomationKey::Escape
    );
}

#[test]
fn unknown_and_unspecified_values_are_rejected() {
    assert!(convert::key_from_token("hyper").is_err());
    assert!(convert::modifier_from_token("meta").is_err());
    assert!(convert::key_to_domain(0).is_err());
    assert!(convert::key_to_domain(9_999).is_err());
    assert!(convert::modifier_to_domain(0).is_err());
}
