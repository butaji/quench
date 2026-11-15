use super::JsString;

#[test]
fn concatenation_length_checks_the_limit_and_arithmetic_overflow() {
    use super::{MAX_STRING_UNITS, StringBuildError, checked_string_length};
    assert_eq!(
        checked_string_length(MAX_STRING_UNITS - 1, 1),
        Ok(MAX_STRING_UNITS)
    );
    assert_eq!(
        checked_string_length(MAX_STRING_UNITS, 1),
        Err(StringBuildError::InvalidLength)
    );
    assert_eq!(
        checked_string_length(usize::MAX, 1),
        Err(StringBuildError::InvalidLength)
    );
}

#[test]
fn preserves_lone_surrogates_until_host_conversion() {
    let value = JsString::from_units(&[0xD800, b'a' as u16, 0xDC00]);
    assert_eq!(value.units(), &[0xD800, b'a' as u16, 0xDC00]);
    assert_eq!(value.to_string(), "�a�");
}

#[test]
fn encodes_unicode_scalars_as_utf16_units() {
    let value = JsString::from_str("A🦀");
    assert_eq!(value.units(), &[u16::from(b'A'), 0xD83E, 0xDD80]);
}

#[test]
fn concatenation_keeps_utf16_units_and_flattens_on_demand() {
    let mut value = JsString::from_units(&[0xD800, b'a' as u16]);
    value.push_js_string(&JsString::from_str("bc"));
    assert_eq!(value.units().len(), 4);
    assert_eq!(
        value.units(),
        &[0xD800, b'a' as u16, b'b' as u16, b'c' as u16]
    );
    assert_eq!(value.host_string(), "�abc");
}

#[test]
fn repeats_units_without_reencoding_surrogates() {
    let value = JsString::from_units(&[0xD800, b'a' as u16]);
    let repeated = value.repeat(2);
    assert_eq!(
        repeated.units(),
        &[0xD800, b'a' as u16, 0xD800, b'a' as u16]
    );
}

#[test]
fn searches_and_splits_by_units() {
    let value = JsString::from_units(&[0xD800, b'|' as u16, 0xDC00]);
    assert_eq!(
        value.find_units(super::Units::Wide(&[b'|' as u16]), 0),
        Some(1)
    );
    let mut parts = Vec::new();
    value.split_runs(super::Units::Wide(&[b'|' as u16]), usize::MAX, |run| parts.push(JsString::from_unit_run(run)));
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].units(), &[0xD800]);
    assert_eq!(parts[1].units(), &[0xDC00]);
}

#[test]
fn host_text_is_lazy_and_utf16_remains_authoritative() {
    let paired = JsString::from_units(&[0xD83E, 0xDD80]);
    let lone = JsString::from_units(&[0xD800]);

    assert!(paired.existing_host().is_none());
    assert!(lone.existing_host().is_none());
    assert!(paired.existing_host().is_none());
    assert!(lone.existing_host().is_none());
    assert_eq!(lone.host_string(), "�");
    assert_eq!(lone.units(), &[0xD800]);
    assert!(lone.existing_host().is_some());
}
