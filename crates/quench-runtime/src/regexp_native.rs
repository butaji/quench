//! Small native RegExp programs for patterns whose semantics are fully known
//! at parse time. The general engine remains behind `regexp_backend`; these
//! programs keep literal/class scans allocation-free and form the first slice
//! of the VM-owned parser → IR → interpreter pipeline.

#[derive(Clone, Copy)]
enum CharacterClass {
    Digit,
    NotDigit,
    Space,
    NotSpace,
    Word,
    NotWord,
}

enum NativePattern<'a> {
    Literal(&'a [u8]),
    CharacterClass(CharacterClass),
    Repeat {
        unit: u8,
        min: usize,
        max: Option<usize>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeMatch {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn find_str(
    source: &str,
    flags: &str,
    input: &str,
    start: usize,
) -> Option<NativeMatch> {
    let pattern = parse(source, flags)?;
    let sticky = flags.contains('y');
    let tail = input.get(start..)?;
    match pattern {
        NativePattern::Literal(pattern) => {
            let needle = std::str::from_utf8(pattern).ok()?;
            if sticky && !tail.starts_with(needle) {
                return None;
            }
            let offset = if sticky { 0 } else { tail.find(needle)? };
            Some(NativeMatch {
                start: start + offset,
                end: start + offset + needle.len(),
            })
        }
        NativePattern::CharacterClass(class) => {
            let (offset, character) = if sticky {
                tail.char_indices()
                    .next()
                    .filter(|(_, ch)| class_matches(class, *ch))?
            } else {
                tail.char_indices()
                    .find(|(_, ch)| class_matches(class, *ch))?
            };
            Some(NativeMatch {
                start: start + offset,
                end: start + offset + character.len_utf8(),
            })
        }
        NativePattern::Repeat { unit, min, max } => {
            find_repeat(tail.as_bytes(), unit, min, max, sticky).map(|matched| NativeMatch {
                start: start + matched.start,
                end: start + matched.end,
            })
        }
    }
}

pub(crate) fn find_units(
    source: &str,
    flags: &str,
    input: &[u16],
    start: usize,
) -> Option<NativeMatch> {
    let pattern = parse(source, flags)?;
    let sticky = flags.contains('y');
    let tail = input.get(start..)?;
    match pattern {
        NativePattern::Literal(pattern) => {
            let offset = if sticky {
                0
            } else {
                tail.windows(pattern.len()).position(|window| {
                    window
                        .iter()
                        .zip(pattern.iter())
                        .all(|(unit, byte)| *unit == u16::from(*byte))
                })?
            };
            if tail.len() < offset + pattern.len()
                || !tail[offset..offset + pattern.len()]
                    .iter()
                    .zip(pattern.iter())
                    .all(|(unit, byte)| *unit == u16::from(*byte))
            {
                return None;
            }
            Some(NativeMatch {
                start: start + offset,
                end: start + offset + pattern.len(),
            })
        }
        NativePattern::CharacterClass(class) => {
            let offset = if sticky {
                0
            } else {
                tail.iter().position(|unit| unit_matches(class, *unit))?
            };
            tail.get(offset)
                .filter(|unit| unit_matches(class, **unit))?;
            Some(NativeMatch {
                start: start + offset,
                end: start + offset + 1,
            })
        }
        NativePattern::Repeat { unit, min, max } => find_repeat_units(tail, unit, min, max, sticky)
            .map(|matched| NativeMatch {
                start: start + matched.start,
                end: start + matched.end,
            }),
    }
}

pub(crate) fn test_str(source: &str, flags: &str, input: &str, start: usize) -> Option<bool> {
    Some(find_str(source, flags, input, start).is_some())
}

pub(crate) fn test_units(source: &str, flags: &str, input: &[u16], start: usize) -> Option<bool> {
    Some(find_units(source, flags, input, start).is_some())
}

fn parse<'a>(source: &'a str, flags: &str) -> Option<NativePattern<'a>> {
    if let Some(class) = source
        .strip_prefix("\\\\")
        .or_else(|| source.strip_prefix('\\'))
        .and_then(parse_class)
    {
        return Some(NativePattern::CharacterClass(class));
    }
    if !flags.contains(['i', 'm', 'u', 'v']) {
        if let Some(repeat) = parse_repeat(source) {
            return Some(repeat);
        }
    }
    if source.is_empty()
        || !source.is_ascii()
        || flags.contains(['i', 'm', 'u', 'v'])
        || source
            .bytes()
            .any(|byte| b"\\.^$*+?()[]{}|".contains(&byte))
    {
        return None;
    }
    Some(NativePattern::Literal(source.as_bytes()))
}

fn parse_repeat(source: &str) -> Option<NativePattern<'_>> {
    let bytes = source.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii() || b"\\.^$*+?()[]{}|".contains(&bytes[0]) {
        return None;
    }
    let (min, max, quantifier_start): (usize, Option<usize>, usize) = match bytes[1] {
        b'*' => (0, None, 1),
        b'+' => (1, None, 1),
        b'?' => (0, Some(1), 1),
        b'{' => {
            let close = source[2..].find('}')? + 2;
            let body = &source[2..close];
            let (lower, upper) = body
                .split_once(',')
                .map_or((body, None), |(lower, upper)| (lower, Some(upper)));
            let min = lower.parse().ok()?;
            let max = match upper {
                None => Some(min),
                Some("") => None,
                Some(value) => Some(value.parse().ok()?),
            };
            (min, max, close)
        }
        _ => return None,
    };
    if quantifier_start + 1 != bytes.len() || max.is_some_and(|upper| upper < min) {
        return None;
    }
    Some(NativePattern::Repeat {
        unit: bytes[0],
        min,
        max,
    })
}

fn find_repeat(
    input: &[u8],
    unit: u8,
    min: usize,
    max: Option<usize>,
    sticky: bool,
) -> Option<std::ops::Range<usize>> {
    let starts = if sticky {
        0..input.len().min(1)
    } else {
        0..input.len()
    };
    for start in starts {
        let available = input[start..]
            .iter()
            .take_while(|candidate| **candidate == unit)
            .count();
        if available >= min {
            let end = start + max.map_or(available, |upper| available.min(upper));
            return Some(start..end);
        }
    }
    (min == 0).then_some(0..0)
}

fn find_repeat_units(
    input: &[u16],
    unit: u8,
    min: usize,
    max: Option<usize>,
    sticky: bool,
) -> Option<std::ops::Range<usize>> {
    let starts = if sticky {
        0..input.len().min(1)
    } else {
        0..input.len()
    };
    for start in starts {
        let available = input[start..]
            .iter()
            .take_while(|candidate| **candidate == u16::from(unit))
            .count();
        if available >= min {
            let end = start + max.map_or(available, |upper| available.min(upper));
            return Some(start..end);
        }
    }
    (min == 0).then_some(0..0)
}

fn parse_class(class: &str) -> Option<CharacterClass> {
    Some(match class {
        "d" => CharacterClass::Digit,
        "D" => CharacterClass::NotDigit,
        "s" => CharacterClass::Space,
        "S" => CharacterClass::NotSpace,
        "w" => CharacterClass::Word,
        "W" => CharacterClass::NotWord,
        _ => return None,
    })
}

fn class_matches(class: CharacterClass, character: char) -> bool {
    match class {
        CharacterClass::Digit => character.is_ascii_digit(),
        CharacterClass::NotDigit => !character.is_ascii_digit(),
        CharacterClass::Space => crate::regexp::is_ecma_whitespace(character),
        CharacterClass::NotSpace => !crate::regexp::is_ecma_whitespace(character),
        CharacterClass::Word => character.is_ascii_alphanumeric() || character == '_',
        CharacterClass::NotWord => !(character.is_ascii_alphanumeric() || character == '_'),
    }
}

fn unit_matches(class: CharacterClass, unit: u16) -> bool {
    char::from_u32(u32::from(unit)).map_or(
        matches!(
            class,
            CharacterClass::NotDigit | CharacterClass::NotSpace | CharacterClass::NotWord
        ),
        |character| class_matches(class, character),
    )
}

#[cfg(test)]
mod tests {
    use super::{find_str, find_units, test_str, test_units};

    #[test]
    fn literal_scan_respects_sticky_start() {
        assert_eq!(test_str("abc", "", "zabc", 1), Some(true));
        assert_eq!(test_str("abc", "y", "zabc", 1), Some(true));
        assert_eq!(test_str("abc", "y", "zabc", 0), Some(false));
    }

    #[test]
    fn character_class_scan_handles_utf16_surrogates() {
        assert_eq!(test_units("\\D", "", &[0xD800], 0), Some(true));
        assert_eq!(test_units("\\S", "", &[0xD800], 0), Some(true));
        assert_eq!(test_units("\\W", "", &[0xD800], 0), Some(true));
    }

    #[test]
    fn simple_repetition_is_bounded_by_the_input_run() {
        assert_eq!(test_str("a+", "", "xxaa", 0), Some(true));
        assert_eq!(test_str("a{3}", "", "xxaa", 0), Some(false));
        assert_eq!(test_str("a{2,4}", "", "xxaaaaa", 0), Some(true));
        assert_eq!(test_units("a*", "", &[], 0), Some(true));
    }

    #[test]
    fn scans_begin_at_the_requested_index() {
        assert_eq!(test_str("\\d", "", "1x", 1), Some(false));
        assert_eq!(test_str("\\d", "", "x1", 1), Some(true));
        assert_eq!(
            test_units("a+", "", &[b'x' as u16, b'a' as u16], 1),
            Some(true)
        );
    }

    #[test]
    fn native_matches_report_consumed_length() {
        assert_eq!(
            find_str("a+", "g", "xxaaa", 2),
            Some(super::NativeMatch { start: 2, end: 5 })
        );
        assert_eq!(
            find_str("a?", "y", "x", 0),
            Some(super::NativeMatch { start: 0, end: 0 })
        );
        assert_eq!(
            find_units("\\d", "y", &[b'7' as u16], 0),
            Some(super::NativeMatch { start: 0, end: 1 })
        );
    }
}
