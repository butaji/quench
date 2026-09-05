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
    AnchoredLiteral {
        literal: &'a [u8],
        anchor_start: bool,
        anchor_end: bool,
    },
    CharacterClass(CharacterClass),
    PropertyRepeat {
        name: &'a str,
        value: Option<&'a str>,
        negative: bool,
    },
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
        NativePattern::AnchoredLiteral {
            literal,
            anchor_start,
            anchor_end,
        } => {
            if anchor_start && start != 0 {
                return None;
            }
            let candidate = if anchor_start {
                input
            } else {
                input.get(start..)?
            };
            let offset = if anchor_start || sticky {
                0
            } else {
                candidate
                    .as_bytes()
                    .windows(literal.len())
                    .position(|window| window == literal)?
            };
            let end = offset.checked_add(literal.len())?;
            if end > candidate.len()
                || &candidate.as_bytes()[offset..end] != literal
                || (anchor_end && end != candidate.len())
            {
                return None;
            }
            let absolute = if anchor_start { 0 } else { start + offset };
            Some(NativeMatch {
                start: absolute,
                end: absolute + literal.len(),
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
        NativePattern::PropertyRepeat {
            name,
            value,
            negative,
        } => find_property_repeat_str(name, value, negative, input, start),
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
        NativePattern::AnchoredLiteral {
            literal,
            anchor_start,
            anchor_end,
        } => {
            if anchor_start && start != 0 {
                return None;
            }
            let candidate = if anchor_start {
                input
            } else {
                input.get(start..)?
            };
            let offset = if anchor_start || sticky {
                0
            } else {
                candidate.windows(literal.len()).position(|window| {
                    window
                        .iter()
                        .zip(literal.iter())
                        .all(|(unit, byte)| *unit == u16::from(*byte))
                })?
            };
            let end = offset.checked_add(literal.len())?;
            if end > candidate.len()
                || candidate.get(offset..end).is_none_or(|window| {
                    !window
                        .iter()
                        .zip(literal.iter())
                        .all(|(unit, byte)| *unit == u16::from(*byte))
                })
                || (anchor_end && end != candidate.len())
            {
                return None;
            }
            let absolute = if anchor_start { 0 } else { start + offset };
            Some(NativeMatch {
                start: absolute,
                end: absolute + literal.len(),
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
        NativePattern::PropertyRepeat {
            name,
            value,
            negative,
        } => find_property_repeat_units(name, value, negative, flags, input, start),
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

#[inline]
pub(crate) fn supports_str(source: &str, flags: &str) -> bool {
    parse(source, flags).is_some()
}

fn parse<'a>(source: &'a str, flags: &str) -> Option<NativePattern<'a>> {
    if let Some(property) = parse_property_repeat(source, flags) {
        return Some(property);
    }
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
    if source.is_empty() || !source.is_ascii() || flags.contains(['i', 'm', 'u', 'v']) {
        return None;
    }
    let (anchor_start, body) = source
        .strip_prefix('^')
        .map_or((false, source), |body| (true, body));
    let (body, anchor_end) = body
        .strip_suffix('$')
        .map_or((body, false), |body| (body, true));
    if (anchor_start || anchor_end)
        && !body.is_empty()
        && !body.bytes().any(|byte| b"\\.^$*+?()[]{}|".contains(&byte))
    {
        return Some(NativePattern::AnchoredLiteral {
            literal: body.as_bytes(),
            anchor_start,
            anchor_end,
        });
    }
    if anchor_start || anchor_end || body.bytes().any(|byte| b"\\.^$*+?()[]{}|".contains(&byte)) {
        return None;
    }
    Some(NativePattern::Literal(body.as_bytes()))
}

fn parse_property_repeat<'a>(source: &'a str, flags: &str) -> Option<NativePattern<'a>> {
    if !flags.contains(['u', 'v']) {
        return None;
    }
    let (negative, body) = source
        .strip_prefix("^\\p{")
        .map(|body| (false, body))
        .or_else(|| source.strip_prefix("^\\P{").map(|body| (true, body)))?;
    let body = body.strip_suffix("}+$")?;
    let (name, value) = body
        .split_once('=')
        .map_or((body, None), |(name, value)| (name, Some(value)));
    (!name.is_empty() && value.is_none_or(|value| !value.is_empty())).then_some(
        NativePattern::PropertyRepeat {
            name,
            value,
            negative,
        },
    )
}

fn property_matches(
    matcher: crate::regexp_backend::PropertyMatcher,
    negative: bool,
    character: char,
) -> bool {
    let matched = matcher.matches(character);
    if negative {
        !matched
    } else {
        matched
    }
}

fn surrogate_property_matches(name: &str, value: Option<&str>) -> bool {
    matches!(
        (name, value),
        ("Any", _)
            | ("Other", None)
            | ("C", None)
            | (
                "General_Category" | "gc",
                Some("Other" | "C" | "Surrogate" | "Cs")
            )
            | (
                "Script" | "sc" | "Script_Extensions" | "scx",
                Some("Unknown" | "Zzzz")
            )
    )
}

fn find_property_repeat_str(
    name: &str,
    value: Option<&str>,
    negative: bool,
    input: &str,
    start: usize,
) -> Option<NativeMatch> {
    if start != 0 {
        return None;
    }
    if name == "Any" {
        return (!negative && !input.is_empty()).then_some(NativeMatch {
            start: 0,
            end: input.len(),
        });
    }
    let matcher = crate::regexp_backend::compile_property_matcher(name, value)?;
    let mut count = 0;
    for character in input.chars() {
        if !property_matches(matcher, negative, character) {
            return None;
        }
        count += character.len_utf8();
    }
    (count > 0).then_some(NativeMatch {
        start: 0,
        end: count,
    })
}

fn find_property_repeat_units(
    name: &str,
    value: Option<&str>,
    negative: bool,
    flags: &str,
    input: &[u16],
    start: usize,
) -> Option<NativeMatch> {
    if start != 0 {
        return None;
    }
    if name == "Any" {
        return (!negative && !input.is_empty()).then_some(NativeMatch {
            start: 0,
            end: input.len(),
        });
    }
    let unicode = flags.contains('u') || flags.contains('v');
    let matcher = crate::regexp_backend::compile_property_matcher(name, value)?;
    let mut index = 0;
    let mut count = 0;
    while index < input.len() {
        let Some((character, width)) = next_code_point(input, index, unicode) else {
            let unit = *input.get(index)?;
            if !(0xD800..=0xDFFF).contains(&unit) {
                return None;
            }
            let matched = surrogate_property_matches(name, value);
            if if negative { matched } else { !matched } {
                return None;
            }
            index += 1;
            count += 1;
            continue;
        };
        if !property_matches(matcher, negative, character) {
            return None;
        }
        index += width;
        count += width;
    }
    (count > 0).then_some(NativeMatch {
        start: 0,
        end: count,
    })
}

fn next_code_point(input: &[u16], index: usize, unicode: bool) -> Option<(char, usize)> {
    let first = *input.get(index)?;
    if unicode && (0xD800..=0xDBFF).contains(&first) {
        let second = *input.get(index + 1)?;
        if (0xDC00..=0xDFFF).contains(&second) {
            let code = 0x1_0000 + ((u32::from(first) - 0xD800) << 10) + u32::from(second) - 0xDC00;
            return Some((char::from_u32(code)?, 2));
        }
    }
    Some((char::from_u32(u32::from(first))?, 1))
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
    if min == 0 && input.first().copied() != Some(unit) {
        return Some(0..0);
    }
    let mut start = 0;
    while start < input.len() {
        if input[start] != unit {
            if sticky {
                break;
            }
            start += 1;
            continue;
        }
        let available = input[start..]
            .iter()
            .take_while(|candidate| **candidate == unit)
            .count();
        if available >= min {
            let end = start + max.map_or(available, |upper| available.min(upper));
            return Some(start..end);
        }
        if sticky {
            break;
        }
        start += available;
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
    if min == 0 && input.first().copied() != Some(u16::from(unit)) {
        return Some(0..0);
    }
    let mut start = 0;
    while start < input.len() {
        if input[start] != u16::from(unit) {
            if sticky {
                break;
            }
            start += 1;
            continue;
        }
        let available = input[start..]
            .iter()
            .take_while(|candidate| **candidate == u16::from(unit))
            .count();
        if available >= min {
            let end = start + max.map_or(available, |upper| available.min(upper));
            return Some(start..end);
        }
        if sticky {
            break;
        }
        start += available;
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
    fn anchored_literal_scan_keeps_absolute_anchor_semantics() {
        assert_eq!(
            find_str("^ba", "", "bare", 0),
            Some(super::NativeMatch { start: 0, end: 2 })
        );
        assert_eq!(test_str("^ba", "", "xbare", 1), Some(false));
        assert_eq!(
            find_str("ba$", "", "xxba", 0),
            Some(super::NativeMatch { start: 2, end: 4 })
        );
        assert_eq!(test_str("ba$", "", "bax", 0), Some(false));
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

    #[test]
    fn property_repetition_reuses_compiled_unicode_data() {
        assert_eq!(test_str("^\\p{Assigned}+$", "u", "abc", 0), Some(true));
        assert_eq!(test_str("^\\P{Assigned}+$", "u", "\u{38b}", 0), Some(true));
        assert_eq!(
            test_str("^\\p{Script_Extensions=Latin}+$", "u", "Aª", 0),
            Some(true)
        );
        assert_eq!(
            test_units("^\\p{Assigned}+$", "u", &[b'a' as u16, b'b' as u16], 0),
            Some(true)
        );
    }
}
