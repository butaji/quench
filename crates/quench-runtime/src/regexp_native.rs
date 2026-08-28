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

enum NativePattern {
    Literal(Box<[u8]>),
    CharacterClass(CharacterClass),
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
    })
}

fn parse(source: &str, flags: &str) -> Option<NativePattern> {
    if !flags.contains(['g', 'y']) {
        if let Some(class) = source
            .strip_prefix("\\\\")
            .or_else(|| source.strip_prefix('\\'))
            .and_then(parse_class)
        {
            return Some(NativePattern::CharacterClass(class));
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
    Some(NativePattern::Literal(source.as_bytes().into()))
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
}
