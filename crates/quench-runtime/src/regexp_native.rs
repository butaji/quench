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

pub(crate) fn test_str(source: &str, flags: &str, input: &str, start: usize) -> Option<bool> {
    let pattern = parse(source, flags)?;
    Some(match pattern {
        NativePattern::Literal(pattern) => {
            let tail = input.get(start..)?;
            if flags.contains('y') {
                tail.as_bytes().starts_with(&pattern)
            } else {
                tail.as_bytes()
                    .windows(pattern.len())
                    .any(|window| window == pattern.as_ref())
            }
        }
        NativePattern::CharacterClass(class) => input
            .chars()
            .any(|character| class_matches(class, character)),
        NativePattern::Repeat { unit, min, max } => has_repeat(input.as_bytes(), unit, min, max),
    })
}

pub(crate) fn test_units(source: &str, flags: &str, input: &[u16], start: usize) -> Option<bool> {
    let pattern = parse(source, flags)?;
    Some(match pattern {
        NativePattern::Literal(pattern) => {
            if flags.contains('y') {
                input.get(start..).is_some_and(|tail| {
                    tail.len() >= pattern.len()
                        && tail[..pattern.len()]
                            .iter()
                            .zip(pattern.iter())
                            .all(|(unit, byte)| *unit == u16::from(*byte))
                })
            } else {
                input.windows(pattern.len()).skip(start).any(|window| {
                    window
                        .iter()
                        .zip(pattern.iter())
                        .all(|(unit, byte)| *unit == u16::from(*byte))
                })
            }
        }
        NativePattern::CharacterClass(class) => input.iter().any(|unit| unit_matches(class, *unit)),
        NativePattern::Repeat { unit, min, max } => has_repeat_units(input, unit, min, max),
    })
}

fn parse<'a>(source: &'a str, flags: &str) -> Option<NativePattern<'a>> {
    if !flags.contains(['g', 'y']) {
        if let Some(class) = source
            .strip_prefix("\\\\")
            .or_else(|| source.strip_prefix('\\'))
            .and_then(parse_class)
        {
            return Some(NativePattern::CharacterClass(class));
        }
    }
    if !flags.contains(['g', 'i', 'm', 'u', 'v', 'y']) {
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

fn has_repeat(input: &[u8], unit: u8, min: usize, max: Option<usize>) -> bool {
    if min == 0 {
        return true;
    }
    input.iter().enumerate().any(|(start, byte)| {
        if *byte != unit {
            return false;
        }
        let available = input[start..]
            .iter()
            .take_while(|candidate| **candidate == unit)
            .count();
        available >= min && max.is_none_or(|upper| available >= min.min(upper))
    })
}

fn has_repeat_units(input: &[u16], unit: u8, min: usize, max: Option<usize>) -> bool {
    if min == 0 {
        return true;
    }
    input.iter().enumerate().any(|(start, value)| {
        if *value != u16::from(unit) {
            return false;
        }
        let available = input[start..]
            .iter()
            .take_while(|candidate| **candidate == u16::from(unit))
            .count();
        available >= min && max.is_none_or(|upper| available >= min.min(upper))
    })
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
    use super::{test_str, test_units};

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
}
