use crate::{Flags, Regex, Subject};
use std::ops::Range;

/// A pattern, its flags, a subject and the ranges a global match of it finds, as Node's
/// `matchAll` reports them.
type Case = (&'static str, &'static str, &'static str, &'static [(usize, usize)]);

fn subject(text: &str) -> Subject {
    Subject::new(text.encode_utf16().collect::<Vec<_>>().into())
}

/// The ranges of a global match of `pattern` over `text`, after checking that the searcher
/// answers every start as `find_range_in_subject` does.
fn global_ranges(pattern: &str, flags: &str, text: &str) -> Vec<Range<usize>> {
    let regex = Regex::with_flags(pattern, Flags::from(flags)).unwrap();
    let subject = subject(text);
    let searcher = regex.range_searcher(&subject);
    for start in 0..=subject.len() {
        assert_eq!(searcher.find(start), regex.find_range_in_subject(&subject, start, false), "/{pattern}/{flags} on {text:?} from {start}");
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    while let Some(range) = searcher.find(start) {
        start = if range.is_empty() { range.end + 1 } else { range.end };
        ranges.push(range);
    }
    ranges
}

fn assert_cases(cases: &[Case]) {
    for &(pattern, flags, text, expected) in cases {
        let expected: Vec<_> = expected.iter().map(|&(start, end)| start..end).collect();
        assert_eq!(global_ranges(pattern, flags, text), expected, "/{pattern}/{flags} on {text:?}");
    }
}

#[test]
fn literal_run_searches_find_the_general_ranges() {
    assert_cases(&[
        ("\\n", "", "a\nbb\n\nccc\n", &[(1, 2), (4, 5), (5, 6), (9, 10)]),
        ("aa", "", "aaaa", &[(0, 2), (2, 4)]),
        ("bb\\n", "", "a\nbb\n\nccc\n", &[(2, 5)]),
        ("q", "", "a\nbb", &[]),
        ("a", "", "", &[]),
        ("ab", "i", "xAbaBab", &[(1, 3), (3, 5), (5, 7)]),
        ("\\n", "", "é\nü\n\u{1F600}\n", &[(1, 2), (3, 4), (6, 7)]),
        ("ü", "", "é\nü\nü", &[(2, 3), (4, 5)]),
        ("\\n", "u", "é\n\u{1F600}\n", &[(1, 2), (4, 5)]),
    ]);
}

#[test]
fn non_literal_range_searches_find_the_general_ranges() {
    assert_cases(&[
        (".", "", "a.b", &[(0, 1), (1, 2), (2, 3)]),
        ("a|bb", "", "abba", &[(0, 1), (1, 3), (3, 4)]),
        ("\\d+", "", "x12y345", &[(1, 3), (4, 7)]),
        ("a+", "", "baaab", &[(1, 4)]),
        ("\u{1F600}", "u", "x\u{1F600}\u{1F600}", &[(1, 3), (3, 5)]),
        ("x*", "", "ab", &[(0, 0), (1, 1), (2, 2)]),
    ]);
}

/// Only a case-sensitive ASCII literal run over an ASCII subject is answered by one byte
/// search, whose starts never overlap the match before.
#[test]
fn literal_starts_answer_only_case_sensitive_ascii_runs() {
    let starts = |pattern: &str, flags: &str, text: &str| {
        let regex = Regex::with_flags(pattern, Flags::from(flags)).unwrap();
        let subject = subject(text);
        regex.range_searcher(&subject).literal_starts().map(|(length, starts)| (length, starts.collect::<Vec<_>>()))
    };
    assert_eq!(starts("aa", "", "aaaa"), Some((2, vec![0, 2])));
    assert_eq!(starts("\\n", "", "a\nbb\n\n"), Some((1, vec![1, 4, 5])));
    assert_eq!(starts("q", "", "abc"), Some((1, vec![])));
    assert_eq!(starts("ab", "i", "xAbab"), None);
    assert_eq!(starts("a.", "", "abab"), None);
    assert_eq!(starts("\\n", "", "é\nü"), None);
}
