use super::JsString;
use std::borrow::Cow;

/// The mapped text, and whether the receiver itself was the result.
fn mapped(text: &str, upper: bool) -> Option<(String, bool)> {
    let string = JsString::from_str(text);
    string.default_case(upper).map(|result| (result.host_string().to_owned(), matches!(result, Cow::Borrowed(_))))
}

#[test]
fn default_case_maps_ascii_latin1_and_wide_text_as_the_full_mapping() {
    for (text, upper, expected) in [
        ("MiXeD-7", true, "MIXED-7"), ("MiXeD-7", false, "mixed-7"),
        ("Ça Été À", true, "ÇA ÉTÉ À"), ("ÇA ÉTÉ À", false, "ça été à"),
        ("Straße", true, "STRASSE"), ("ÿ", true, "\u{0178}"), ("µ", true, "\u{039C}"),
        ("ΑΒΓ дом", true, "ΑΒΓ ДОМ"), ("ΑΒΓ ДОМ", false, "αβγ дом"), ("ς", true, "Σ"),
        ("\u{0130}", false, "i\u{0307}"), ("\u{212A}", false, "k"), ("ı", true, "I"),
        ("\u{10428}x", true, "\u{10400}X"), ("ǆ", true, "Ǆ"),
    ] {
        assert_eq!(mapped(text, upper), Some((expected.to_owned(), false)), "{text:?} upper={upper}");
        let host = if upper { text.to_uppercase() } else { text.to_lowercase() };
        assert_eq!(expected, host, "{text:?} is the host's full mapping");
    }
}

#[test]
fn default_case_of_an_unchanged_string_is_the_string_itself() {
    for (text, upper) in [("", true), ("", false), ("ABC-7", true), ("abc-7", false),
        ("ÀÉ ДОМ", true), ("àé дом", false), ("ß", false), ("σ", false)] {
        assert_eq!(mapped(text, upper), Some((text.to_owned(), true)), "{text:?} upper={upper}");
    }
}

#[test]
fn default_case_keeps_lone_surrogates_and_defers_a_capital_sigma_lowering() {
    let units = [0xD800, u16::from(b'a'), 0xDC00, 0x00E9];
    let lone = JsString::from_units(&units);
    let upper = lone.default_case(true).expect("uppercase has no context");
    assert_eq!(upper.units().to_vec(), [0xD800, u16::from(b'A'), 0xDC00, 0x00C9]);
    assert!(JsString::from_str("ΟΔΟΣ").default_case(false).is_none());
    assert_eq!(mapped("οδοσ", true), Some(("ΟΔΟΣ".to_owned(), false)));
}
