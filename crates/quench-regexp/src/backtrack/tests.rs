//! The VM against the answers recorded from the retired reference matcher, on every text form.
use super::super::{units_from_str, Flags, Regex};
use super::{Beyond, ASCII_UNITS};

/// A ranges test's derived ASCII mask: the units its per-unit test admits.
#[test]
fn ranges_ascii_mask_matches_each_unit() {
    let cases: [&[(u32, u32)]; 5] = [
        &[],
        &[(0, 0)],
        &[(0x30, 0x39), (0x41, 0x5a), (0x61, 0x7a)],
        &[(0x7f, 0x7f), (0x80, 0x10ffff)],
        &[(0x20, 0x2000)],
    ];
    for ranges in cases {
        for negative in [false, true] {
            let beyond = Beyond::Ranges {
                ranges: ranges.into(),
                negative,
            };
            let each = (0..ASCII_UNITS)
                .filter(|value| beyond.matches(*value))
                .fold(0u128, |bits, value| bits | 1 << value);
            assert_eq!(beyond.ascii_mask(), each, "{ranges:?} negative={negative}");
        }
    }
}

/// A case-folded literal's derived ASCII mask: the units its per-unit test admits, for
/// literals that fold with ASCII letters (the Kelvin sign, long s) and others.
#[test]
fn literal_ascii_mask_matches_each_unit() {
    const LITERALS_CHECKED: u32 = 0x2200;
    for value in 0..LITERALS_CHECKED {
        for unicode in [false, true] {
            let beyond = Beyond::Literal { value, unicode };
            let each = (0..ASCII_UNITS)
                .filter(|unit| beyond.matches(*unit))
                .fold(0u128, |bits, unit| bits | 1 << unit);
            assert_eq!(beyond.ascii_mask(), each, "{value:#x} unicode={unicode}");
        }
    }
}

/// A class test's derived ASCII mask: the units its per-unit test admits, for each class
/// kind, negation and flag set, over characters (some folding with ASCII letters),
/// ranges, escapes and a nested class.
#[test]
fn class_ascii_mask_matches_each_unit() {
    use super::super::ClassKind;
    for kind in [ClassKind::Union, ClassKind::Intersection, ClassKind::Subtraction] {
        for take in 0..=mask_items().len() {
            for negative in [false, true] {
                for ignore_case in [false, true] {
                    for unicode in [false, true] {
                        let class = super::super::ClassExpr { negative, kind, items: mask_items().into_iter().skip(take % 3).take(take).collect() };
                        let beyond = Beyond::Class { class: Box::new(class), ignore_case, unicode };
                        let each = (0..ASCII_UNITS)
                            .filter(|unit| beyond.matches(*unit))
                            .fold(0u128, |bits, unit| bits | 1 << unit);
                        assert_eq!(beyond.ascii_mask(), each, "take={take} negative={negative} i={ignore_case} u={unicode}");
                    }
                }
            }
        }
    }
}

/// Class items around the ASCII mask's edges: case forms past ASCII, ranges across it,
/// escapes and a negated nested class.
fn mask_items() -> Vec<super::super::ClassItem> {
    use super::super::{ClassExpr, ClassItem, ClassKind};
    use oxc::regular_expression::ast::CharacterClassEscapeKind as Escape;
    {
        vec![
            ClassItem::Character(u32::from('k')),
            ClassItem::Character(0x212a),
            ClassItem::Character(0x17f),
            ClassItem::Range(u32::from('A'), u32::from('F')),
            ClassItem::Range(0x60, 0x2000),
            ClassItem::Escape(Escape::D),
            ClassItem::Escape(Escape::NegativeW),
            ClassItem::Escape(Escape::S),
            ClassItem::Nested(ClassExpr {
                negative: true,
                kind: ClassKind::Union,
                items: vec![ClassItem::Range(u32::from('0'), u32::from('z'))],
            }),
        ]
    }
}

/// Every pattern, flag set and subject: the compiled program's match against the
/// repository matcher's, from each start position.
const PATTERNS: &[&str] = &[
        r"^ba",
        r"(((\w+):\/\/)([^\/:]*)(:(\d+))?)?([^#?]*)(\?([^#]*))?(#(.*))?",
        r"^\s*|\s*$",
        r"\bQBZPbageby_cynprubyqre\b",
        r"^[\s\xa0]+|[\s\xa0]+$",
        r"(\d*)(\D*)",
        r"(^|\s)lhv\-h(\s|$)",
        r"\?[\w\W]*(sevraqvq|punaaryvq|tebhcvq)=([^\&\?#]*)",
        r"^\s*(\S*(\s+\S+)*)\s*$",
        r"(-[a-z])",
        r"qqqq|qqq|qq|q|ZZZZ|ZZZ|ZZ|Z",
        r"(a*)*b",
        r"(a|)*",
        r"(a*?)+?b",
        r"(?:a|b)*?c",
        r"(z)((a+)?(b+)?(c))*",
        r"(a)|b",
        r"(a)\1",
        r"\1(a)",
        r"(a\1)",
        r"(?<x>a)\k<x>",
        r"(?=(a+))a*b\1",
        r"(?=(a+))",
        r"(?!(a)b)\w",
        r"(.)(?!\1)",
        r"a{2,3}",
        r"(ab){0}c",
        r"(a{2,}?)(a*)",
        r"x*",
        r"[^abc]+",
        r"\B\w+",
        r"^.*$",
        r"(?:)",
        r"((a)|b)+",
        r"(a)?(b)?",
        r"[a-z]+\d{1,2}",
        r".",
        r"(?:(a)|(b))+",
        r"\s*$",
        r"a?$|b",
        r"x*\b",
        r"(?=a)b|$",
        r"(?:^|c)a",
        r"\b(?:a|$)",
        r"a*(?!b)$",
        r"(x?)\1$",
        r"[ab]*c?$",
        r"NQ_VQ",
        r"ab(c)?",
        r"pad",
        r"(a?)?b",
        r"(a)??b",
        r"(?:(a)|b)??(a)",
        r"((a)?)?c",
        r"(x?)?",
        r"(a)?\1b",
        r";\s?",
        r"[;,]x",
        r"(?:a|b|c)d",
        r"[abcd]e",
        r"^ab|c",
        r"(?:x|;)y?$",
        r"\s?;\s?",
        r"^c",
        r"a$|^b",
        r"\nx",
        r"(a)?(b)?\2",
        r"((a)|(b))?c",
        r"(?:(x)|y)?(z)?",
        r"(a)?(?=(b))",
        r"((a)?b)?\2",
        r"(?:(a)?b)?a\1",
        r"((a)?)?\2b",
        r"(?:((a)b)?c)?\2",
        r"(?=((a)?))\1b",
        r"(?:(a)|b)*?\1",
        // Start sets the scans cover: byte sets, up to four ranges, more than four
        // ranges (the table), members past ASCII, and anchored first units.
        r"[0-9a-f]+",
        r"[A-Z_][a-z]",
        r"[0-2]|[5-6]|[b-c]|[x-y]|q",
        r"[!#%&(*,.]",
        r"\s+\w",
        r"\d\s",
        r"^(?=x)a",
        r"^$",
        r"^(?:ba|q)",
        r"^\s*a",
        // Guarded splits: optional bodies and alternatives whose first units, anchors
        // or boundaries decide whether they can start at a position.
        r"(^a)?b",
        r"(?:$|a)?b",
        r"(?:x|y\b|z)?\w",
        r"(?:(?=a)\w|b)c",
        r"(a|b\1)?c",
        r"(?:a|^b|c$)+?d",
        r"(?:ab|ac|b)",
        r"(;)?\s|(x)?$",
        r"(?:(\w+):)?(\w*)",
        r"(?:[+, ]|ab)",
        // Literal prefixes: whole literals with and without case, folded prefixes with
        // and without a caseless unit, followed by captures and other parts.
        r"zfvr",
        r"aBc",
        r"a(b)c",
        r"##yv4##",
        r"k#s",
        r"kelvin",
        r"ss\w",
        r"Ab|c",
        // Lookbehind, compiled backward: units, repeats both ways, captures closed at
        // their left end, backreferences, alternatives, nested lookarounds.
        r"(?<=a)b",
        r"(?<!a)b",
        r"(?<=\$)\d+(\.\d+)?",
        r"(?<!\w)\d{2}\b",
        r"(?<=(a+))b",
        r"(?<=(a+?))b",
        r"(?<=(\w)(\w))\s",
        r"(?<=\1(a))b",
        r"(?<=(a)\1)b",
        r"(?<=ab|b|q)\w",
        r"(?<=a(?=b)b)c",
        r"(?=(?<=a)b)\w",
        r"(?<=(?<!x)a)\w",
        r"(?<=^|\s)\w+",
        r"(?<=a{2,3})x?",
        r"(?<=[a-c]*?)c",
        r"(?<=(?:a|b)+)c",
        r"(?<=(?:(a)|b)*)c",
        r"(?<![a-z]+)\d",
        r"(?<=\b)x",
        r"(?<=([ab])+)$",
        r"(?<=(?<x>\w)\k<x>)\w",
    ];

const SUBJECTS: &[&str] = &[
        "",
        "a",
        "b",
        "ab",
        "aab",
        "aaab",
        "abab",
        "ba123",
        "pyvpx",
        "zaacbbbcac",
        "uggc://jjj.snprobbx.pbz/ybtva.cuc?x=1#frag",
        "  pad  ",
        "\u{a0} x \u{a0}",
        "qqqq, ZZZ q, llll",
        "line\nnext\r\nend",
        "abc1 xyz22 q333",
        "aaaa",
        "AbAB",
        "-x-Y",
        "é a ë",
        "x\u{2028}y",
        "\u{1F600}a\u{1F600}",
        "lhv-h lhv-h",
        "?sevraqvq=12&q",
        "a c ",
        "xx\n",
        " b \t",
        "cab\nab",
        "a;b ; c,x;",
        "bcd ade cxy",
        "c\nx",
        "ab;",
        // Longer than one scan block, with the match past the block body and in the tail.
        "...........................Q_bar 7f;",
        "................................. 5 x",
        "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzc",
        "xa..............................&",
        "\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0} a",
        // A split-style subject: a separator run, then ones past one scan block.
        "a ; b;c ;d , e+f..............................; g ;",
        "xb yb\nab b$a cd bd ac",
        "xxZfVr zfvrzfv aBC Abc abC",
        "##YV4## ##yv4# K#S k#\u{17F}",
        "\u{212A}elvin kElvin KELVIN \u{17F}\u{17F}x sSx",
        "abcabc xab bc",
        "\u{1F1EB}\u{1F1F7}x\u{1F1E9}\u{1F1EA}x \u{1F600}\u{1F600}",
    ];

#[test]
fn matches_classes_of_strings() {
    // (pattern, subject, expected match in UTF-8 offsets), checked against V8's results.
    let cases: &[(&str, &str, Option<(usize, usize)>)] = &[
        (r"[\q{abc|ab|a}]c?", "abc", Some((0, 3))),
        (r"[\q{abc|ab|a}]c?", "abd", Some((0, 2))),
        (r"[\q{abc|ab|a}]+?b", "ab", Some((0, 2))),
        (r"[\q{abc|ab|a}]+b", "abcab", Some((0, 5))),
        (r"(?<=[\q{ab|b}])c", "xabc", Some((3, 4))),
        (r"(?<=[\q{ab|b}])c", "xc", None),
        (r"(?<=(?:^|x)[\q{ab|b}])c", "xabc", Some((3, 4))),
        (r"(?<![\q{ab}])c", "abc ac", Some((5, 6))),
        (r"\p{RGI_Emoji_Flag_Sequence}x", "\u{1F1EB}\u{1F1F7}x", Some((0, 9))),
        (r"[[\q{bc}a]&&\q{bc}]", "abc", Some((1, 3))),
    ];
    for &(pattern, subject, expected) in cases {
        let regex = Regex::with_flags(pattern, Flags::from("v")).unwrap();
        let found = regex.find_from(subject, 0).next().map(|found| (found.range.start, found.range.end));
        assert_eq!(found, expected, "/{pattern}/v on {subject:?}");
    }
}

#[test]
fn matches_the_recorded_answers() {
    golden_check();
}

/// FNV-1a over the bytes of `text`.
fn fnv(hash: u64, text: &str) -> u64 {
    text.bytes().fold(hash, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3))
}

/// Each pattern's results over every subject and start, as one digest per pattern and flags:
/// the micro-VM's answers against the recorded ones (`golden.txt`, recorded from the
/// reference matcher before it was retired, lookbehind included).
fn golden_check() {
    let mut lines = Vec::new();
    for flags in ["", "i", "m", "s", "u", "im", "iu", "v", "iv"] {
        for pattern in PATTERNS {
            let Ok(regex) = Regex::with_flags(pattern, Flags::from(flags)) else { continue };
            let unicode = regex.flags.unicode || regex.flags.unicode_sets;
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            for subject in SUBJECTS {
                let units = units_from_str(subject, unicode);
                for first in 0..=units.len() {
                    let found = regex.golden_find(&units, first).map(|found| (found.range, found.captures));
                    hash = fnv(hash, &format!("{first}:{found:?};"));
                }
            }
            lines.push(format!("{flags}\t{pattern}\t{hash:016x}"));
        }
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/backtrack/golden.txt");
    if std::env::var_os("QUENCH_REGEXP_GOLDEN").is_some() {
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
        return;
    }
    let recorded = std::fs::read_to_string(path).unwrap();
    for (line, expected) in lines.iter().zip(recorded.lines()) {
        assert_eq!(line, expected);
    }
    assert_eq!(lines.len(), recorded.lines().count());
}

#[test]
/// One search's answer on every form of a subject: decoded units, code units, ASCII bytes,
/// and sticky searches at the match's start.
fn matches_on_every_text_form() {
    for flags in ["", "i", "m", "s", "u", "im", "iu", "v", "iv"] {
        for &pattern in PATTERNS {
            let Ok(regex) = Regex::with_flags(pattern, Flags::from(flags)) else { continue };
            for &subject in SUBJECTS {
                let units = units_from_str(subject, regex.flags.unicode || regex.flags.unicode_sets);
                for first in 0..=units.len() {
                    check_text_forms(&regex, subject, &units, first, &format!("/{pattern}/{flags} on {subject:?} from {first}"));
                }
            }
        }
    }
}

/// A match's bounds and captures in the subject's own positions.
type Raw = Option<(std::ops::Range<usize>, Vec<Option<std::ops::Range<usize>>>)>;

fn raw(found: Option<super::Found>) -> Raw {
    found.map(|found| (found.start..found.end, found.captures))
}

/// The search from `first` on the decoded units, the code units (outside unicode mode, where
/// a decoded unit is a code unit) and the ASCII bytes; and a sticky search, the search's match
/// where that starts at `first`.
fn check_text_forms(regex: &Regex, subject: &str, units: &[super::super::Unit], first: usize, context: &str) {
    let program = &regex.complete().backtracking;
    let decoded = raw(program.find(units, first, false, true));
    if !(regex.flags.unicode || regex.flags.unicode_sets) {
        let code_units = subject.encode_utf16().collect::<Vec<_>>();
        assert_eq!(raw(program.find(&code_units[..], first, false, true)), decoded, "{context}: code units");
    }
    let at_first = |text_find: &dyn Fn(bool) -> Option<super::Found>| {
        let search = raw(text_find(false)).filter(|(range, _)| range.start == first);
        assert_eq!(raw(text_find(true)), search, "{context}: sticky");
    };
    at_first(&|sticky| program.find(units, first, sticky, true));
    if subject.is_ascii() {
        let bytes = subject.as_bytes();
        assert_eq!(raw(program.find(bytes, first, false, true)), decoded, "{context}: bytes");
        at_first(&|sticky| program.find(bytes, first, sticky, true));
    }
}

#[test]
fn possessive_repeats_keep_their_answers() {
    // (pattern, subject, match in UTF-8 offsets, whether the pattern's first repeat is atomic)
    let cases: &[(&str, &str, Option<(usize, usize)>, bool)] = &[
        (r"[\w.+-]+@[\w-]+\.[\w.]+", "x john.doe+tag@example.com y", Some((2, 26)), true),
        (r"(\w+)\s(\w+)", "ab cd", Some((0, 5)), true),
        (r"\w+(?=@)", "foo bar@x", Some((4, 7)), true),
        (r"\w+(?!@)", "ab@", Some((0, 1)), false),
        (r"a+(?=ab)", "aaab", Some((0, 2)), false),
        (r"x*y", "xxz xy", Some((4, 6)), true),
        (r"[a-z]+\d", "abc1", Some((0, 4)), true),
        (r"\d+\d", "123", Some((0, 3)), false),
        (r"\s+é", "a  é", Some((1, 5)), true),
        (r"[a-c]+(d)", "abcd", Some((0, 4)), true),
    ];
    for &(pattern, subject, expected, atomic) in cases {
        let regex = Regex::with_flags(pattern, Flags::default()).unwrap();
        let program = &regex.complete().backtracking;
        let marked = program.instructions.iter().find_map(|inst| match inst {
            super::Inst::UnitRepeat { atomic, .. } => Some(*atomic),
            _ => None,
        });
        assert_eq!(marked, Some(atomic), "/{pattern}/ atomic");
        let found = regex.find_from(subject, 0).next().map(|found| (found.range.start, found.range.end));
        assert_eq!(found, expected, "/{pattern}/ on {subject:?}");
    }
}
