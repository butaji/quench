use super::{
    code_unit_bytes, contains_code_unit_bytes, literal_finder, normalize_new_unicode_scripts,
    Flags, Regex, Subject, NEW_UNICODE_SCRIPTS,
};

/// The wide search's required-literal check: an empty literal occurs everywhere, one with a
/// unit past UTF-16 nowhere.
fn contains_code_units(input: &[u16], literal: &[u32]) -> bool {
    literal.is_empty()
        || code_unit_bytes(literal)
            .is_some_and(|needle| contains_code_unit_bytes(input, &literal_finder(&needle)))
}

#[test]
fn code_unit_literals_match_whole_units() {
    let text = |text: &str| text.encode_utf16().collect::<Vec<_>>();
    let literal = |text: &str| text.encode_utf16().map(u32::from).collect::<Vec<_>>();
    assert!(contains_code_units(&text("aé=bc"), &literal("=b")));
    assert!(contains_code_units(&text("aé=bc"), &literal("")));
    assert!(!contains_code_units(&text("aé=bc"), &literal("cb")));
    // Little-endian bytes 01 02 03 00 hold the bytes of 0x0302 across a unit boundary.
    assert!(!contains_code_units(&[0x0201, 0x0003], &[0x0302]));
    assert!(contains_code_units(&[0x0201, 0x0302], &[0x0302]));
    assert!(!contains_code_units(&text("abc"), &[0x1_F600]));
}

#[test]
fn multiline_end_anchor_accepts_crlf_and_other_ecmascript_terminators() {
    let regex = Regex::with_flags(r"^Connection: close\r\n$", Flags::from("m")).unwrap();
    for subject in [
        "A\r\nConnection: close\r\n\r\n",
        "A\nConnection: close\r\n\n",
        "A\u{2028}Connection: close\r\n\u{2028}",
        "A\u{2029}Connection: close\r\n\u{2029}",
    ] {
        assert!(regex.find_from(subject, 0).next().is_some(), "{subject:?}");
    }
}

#[test]
fn new_unicode_script_normalization_borrows_unrelated_patterns() {
    for pattern in [r"\s+", r"\p{Letter}", r"\p{Script=Latin}"] {
        assert!(matches!(
            normalize_new_unicode_scripts(pattern),
            std::borrow::Cow::Borrowed(_)
        ));
    }
}

#[test]
fn new_unicode_script_aliases_keep_their_ranges() {
    for script in NEW_UNICODE_SCRIPTS {
        let character = char::from_u32(script.ranges[0].0).unwrap().to_string();
        for alias in script.aliases {
            let positive =
                Regex::with_flags(&format!(r"\p{{Script={alias}}}"), Flags::from("u")).unwrap();
            assert!(
                positive.find_from(&character, 0).next().is_some(),
                "positive property did not match {alias}"
            );

            let negative =
                Regex::with_flags(&format!(r"\P{{Script={alias}}}"), Flags::from("u")).unwrap();
            assert!(
                negative.find_from(&character, 0).next().is_none(),
                "negative property matched {alias}"
            );
        }
    }
}

#[test]
fn case_insensitive_ranges_match_members_not_folded_endpoints() {
    // U+03CE folds to U+038F, below the range's start: folding the endpoints empties
    // the range, while its members (all Greek lowercase letters here) still match.
    for flags in ["i", "iu"] {
        let regex = Regex::with_flags(r"^[\u0391-\u03ce]+$", Flags::from(flags)).unwrap();
        assert!(
            regex
                .find_from("αβγδεζηθικλμνξοπρςστυφχψω", 0)
                .next()
                .is_some(),
            "{flags}"
        );
        let ascii = Regex::with_flags(r"^[a-z]+$", Flags::from(flags)).unwrap();
        assert!(ascii.find_from("QUENCH", 0).next().is_some(), "{flags}");
    }
}

#[test]
fn sticky_matches_only_at_the_start_position() {
    let regex = Regex::with_flags(r"y|\d+", Flags::default()).unwrap();
    let subject = super::Subject::from_ascii("ab12y".into(), 0.."ab12y".len());
    assert!(regex.find_sticky_in_subject(&subject, 0).next().is_none());
    let digits = regex.find_sticky_in_subject(&subject, 2).next().unwrap();
    assert_eq!(digits.range, 2..4);
    assert_eq!(
        regex
            .find_sticky_in_subject(&subject, 4)
            .next()
            .unwrap()
            .range,
        4..5
    );
    assert!(regex.find_sticky_in_subject(&subject, 6).next().is_none());
}

#[test]
fn captures_partition_a_greedy_run() {
    let regex = Regex::with_flags("(b+)(b+)(b+)", Flags::default()).unwrap();
    let matched = regex.find_from("abbbbbbbc", 0).next().unwrap();
    assert_eq!(matched.range, 1..8);
    assert_eq!(matched.captures, vec![Some(1..6), Some(6..7), Some(7..8)]);
}

#[test]
fn automata_backend_preserves_ascii_capture_ranges() {
    let regex = Regex::with_flags("(ab+)", Flags::default()).unwrap();
    let matched = regex.find_from("zzabbb", 0).next().unwrap();
    assert_eq!(matched.range, 2..6);
    assert_eq!(matched.captures, vec![Some(2..6)]);
}

#[test]
fn unicode_string_class_matches() {
    for (pattern, input) in [("[[0-9]&&\\q{0|2|4|9️⃣}]", "0"), ("[\\q{0|2|4|9️⃣}]", "9️⃣")]
    {
        let regex = Regex::with_flags(pattern, Flags::from("v")).unwrap();
        assert!(
            regex.find_from(input, 0).next().is_some(),
            "{pattern} {input}"
        );
    }
}
#[test]
fn lookbehind_sticky_prefix() {
    let regex = Regex::with_flags("(?<=^(\\w+))def", Flags::from("g")).unwrap();
    let input = "abcdefdef".encode_utf16().collect::<Vec<_>>();
    assert!(regex.find_from_utf16(&input, 0).next().is_some());
}

#[test]
fn empty_match_search_past_input_end_terminates() {
    let regex = Regex::with_flags("(?:)", Flags::default()).unwrap();
    let input = "A😀B".encode_utf16().collect::<Vec<_>>();
    assert!(regex
        .find_from_utf16(&input, input.len() + 1)
        .next()
        .is_none());
    assert!(regex.find_from("A😀B", "A😀B".len() + 1).next().is_none());
}
#[test]
fn lookahead_capture_preserves_extent() {
    let regex = Regex::with_flags("(?:(?=(abc)))a", Flags::default()).unwrap();
    let matched = regex.find_from("abc", 0).next().unwrap();
    assert_eq!(matched.range, 0..1);
    assert_eq!(matched.captures, vec![Some(0..3)]);
}
#[test]
fn duplicate_group_properties_pattern_matches() {
    let regex = Regex::with_flags(
        "(?:(?<x>a)|(?<y>a)(?<x>b))(?:(?<z>c)|(?<z>d))",
        Flags::default(),
    )
    .unwrap();
    assert!(regex.find_from("abc", 0).next().is_some());
}

#[test]
fn subjects_shorter_than_every_match_fail_and_longer_ones_still_match() {
    // The minimum length gates every search path: ASCII and wide subjects, sticky and
    // ranged searches, with case folding, optional tails, and empty-matching parts.
    let find = |pattern: &str, flags: &str, text: &str, start: usize| {
        let regex = Regex::with_flags(pattern, Flags::from(flags)).unwrap();
        let units: std::rc::Rc<[u16]> = text.encode_utf16().collect::<Vec<_>>().into();
        let subject = Subject::new(units);
        let found = regex.find_in_subject(&subject, start).next().map(|m| m.range);
        let sticky = regex.find_sticky_in_subject(&subject, start).next().map(|m| m.range);
        let range = regex.find_range_in_subject(&subject, start, false);
        assert_eq!(found, range, "{pattern} on {text:?}");
        (found, sticky)
    };
    let route = r"^(?:\/users\/profile)(?:\/$)?$";
    assert_eq!(find(route, "i", "/", 0), (None, None));
    assert_eq!(find(route, "i", "/USERS/profile", 0), (Some(0..14), Some(0..14)));
    assert_eq!(find(route, "i", "/users/profile/", 0), (Some(0..15), Some(0..15)));
    assert_eq!(find("a{3}", "", "xaa", 0), (None, None));
    assert_eq!(find("a{3}", "", "xaaa", 1), (Some(1..4), Some(1..4)));
    assert_eq!(find("ab|c", "", "xc", 1), (Some(1..2), Some(1..2)));
    // Zero-width parts consume nothing, so an empty remainder still matches.
    assert_eq!(find("(?=)", "", "ab", 2), (Some(2..2), Some(2..2)));
    assert_eq!(find(r"(a?)\1$", "", "", 0), (Some(0..0), Some(0..0)));
    assert_eq!(find(r"[\q{}]", "v", "", 0), (Some(0..0), Some(0..0)));
    // One astral code point is two units of the subject.
    assert_eq!(find("\u{1F600}", "u", "x\u{1F600}", 1), (Some(1..3), Some(1..3)));
    assert_eq!(find(".", "u", "\u{1F600}", 0), (Some(0..2), Some(0..2)));
}
