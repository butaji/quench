//! Small native RegExp programs for patterns whose semantics are fully known
//! at parse time. The general engine remains behind `regexp_backend`; these
//! programs keep literal/class scans allocation-free and form the first slice
//! of the VM-owned parser → IR → interpreter pipeline.

use std::{cell::RefCell, collections::HashMap, ops::RangeInclusive, rc::Rc};

const PROPERTY_RANGE_CACHE_LIMIT: usize = 32;

#[derive(Hash, PartialEq, Eq)]
struct PropertyRangeKey {
    name: String,
    value: Option<String>,
    negative: bool,
}

thread_local! {
    static PROPERTY_RANGE_CACHE: RefCell<HashMap<PropertyRangeKey, Rc<[RangeInclusive<u32>]>>> =
        RefCell::new(HashMap::new());
}

pub(crate) fn reset_property_range_cache() {
    PROPERTY_RANGE_CACHE.with(|cache| cache.replace(HashMap::new()));
}

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

const PROPERTY_RANGE_THRESHOLD: usize = 256;

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
                (tail.len() >= pattern.len()
                    && tail[..pattern.len()]
                        .iter()
                        .zip(pattern.iter())
                        .all(|(unit, byte)| *unit == u16::from(*byte)))
                .then_some(0)?
            } else if pattern.len() == 1 {
                tail.iter()
                    .position(|unit| *unit == u16::from(pattern[0]))?
            } else {
                tail.windows(pattern.len()).position(|window| {
                    window
                        .iter()
                        .zip(pattern.iter())
                        .all(|(unit, byte)| *unit == u16::from(*byte))
                })?
            };
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
        NativePattern::PropertyRepeat {
            name,
            value,
            negative,
        } => find_property_repeat_units(name, value, negative, flags, input, start),
        NativePattern::Repeat { unit, min, max } => find_repeat(tail, u16::from(unit), min, max, sticky)
            .map(|matched| NativeMatch {
                start: start + matched.start,
                end: start + matched.end,
            }),
    }
}

#[cfg(test)]
pub(crate) fn test_str(source: &str, flags: &str, input: &str, start: usize) -> Option<bool> {
    Some(find_str(source, flags, input, start).is_some())
}

#[cfg(test)]
pub(crate) fn test_units(source: &str, flags: &str, input: &[u16], start: usize) -> Option<bool> {
    Some(find_units(source, flags, input, start).is_some())
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
        if !matches!(class, CharacterClass::Word | CharacterClass::NotWord)
            || !(flags.contains('i') && flags.contains(['u', 'v']))
        {
            return Some(NativePattern::CharacterClass(class));
        }
    }
    if !flags.contains(['i', 'm', 'u', 'v']) {
        if let Some(repeat) = parse_repeat(source) {
            return Some(repeat);
        }
    }
    if source.is_empty()
        || !source.is_ascii()
        || flags.contains(['i', 'm'])
        || source
            .bytes()
            .any(|byte| b"\\.^$*+?()[]{}|".contains(&byte))
    {
        return None;
    }
    Some(NativePattern::Literal(source.as_bytes()))
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
    (!name.is_empty() && value.map_or(true, |value| !value.is_empty())).then_some(
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
        ("Any" | "Assigned", _)
            | ("Other", None)
            | ("C", None)
            | ("General_Category" | "gc", Some("Other" | "C" | "Surrogate" | "Cs"))
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
    if input.len() >= PROPERTY_RANGE_THRESHOLD {
        if let Some(ranges) = property_ranges(name, value, negative) {
            return find_property_repeat_char_ranges(input, &ranges);
        }
    }
    let matcher = crate::regexp_backend::compile_property_matcher(name, value)?;
    let mut count = 0;
    for character in input.chars() {
        if !property_matches(matcher, negative, character) {
            return None;
        }
        count += character.len_utf8();
    }
    (count > 0).then_some(NativeMatch { start: 0, end: count })
}

fn find_property_repeat_char_ranges(
    input: &str,
    ranges: &[std::ops::RangeInclusive<u32>],
) -> Option<NativeMatch> {
    let mut range_index = 0;
    let mut count = 0;
    let mut previous_value = None;
    for character in input.chars() {
        let value = u32::from(character);
        if !range_contains(ranges, &mut range_index, value, previous_value) {
            return None;
        }
        previous_value = Some(value);
        count += character.len_utf8();
    }
    (count > 0).then_some(NativeMatch { start: 0, end: count })
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
    if input.len() >= PROPERTY_RANGE_THRESHOLD {
        if let Some(ranges) = property_ranges(name, value, negative) {
            return find_property_repeat_ranges(name, value, negative, input, flags, &ranges);
        }
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
    (count > 0).then_some(NativeMatch { start: 0, end: count })
}

fn property_ranges(
    name: &str,
    value: Option<&str>,
    negative: bool,
) -> Option<Rc<[RangeInclusive<u32>]>> {
    let key = PropertyRangeKey {
        name: name.to_string(),
        value: value.map(str::to_string),
        negative,
    };
    if let Some(ranges) = PROPERTY_RANGE_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return Some(ranges);
    }
    let ranges = if name == "ASCII" {
        vec![0..=0x7F]
    } else {
        crate::regexp_backend::compile_property_matcher(name, value)?.ranges()
    };
    let ranges = if negative {
        complement_ranges(ranges)
    } else {
        ranges
    };
    let ranges = Rc::from(ranges);
    PROPERTY_RANGE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= PROPERTY_RANGE_CACHE_LIMIT && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.insert(key, Rc::clone(&ranges));
    });
    Some(ranges)
}

fn complement_ranges(ranges: Vec<std::ops::RangeInclusive<u32>>) -> Vec<std::ops::RangeInclusive<u32>> {
    let mut result = Vec::new();
    let mut start = 0;
    for range in ranges {
        let end = *range.end();
        if start < *range.start() {
            result.push(start..=*range.start() - 1);
        }
        start = end.saturating_add(1);
    }
    if start <= 0x10FFFF {
        result.push(start..=0x10FFFF);
    }
    result
}

fn find_property_repeat_ranges(
    name: &str,
    value: Option<&str>,
    negative: bool,
    input: &[u16],
    flags: &str,
    ranges: &[std::ops::RangeInclusive<u32>],
) -> Option<NativeMatch> {
    let unicode = flags.contains('u') || flags.contains('v');
    let mut index = 0;
    let mut range_index = 0;
    let mut previous_value = None;
    while index < input.len() {
        let Some((character, width)) = next_code_point(input, index, unicode) else {
            let unit = *input.get(index)?;
            let matched = surrogate_property_matches(name, value);
            if if negative { matched } else { !matched } {
                return None;
            }
            previous_value = Some(u32::from(unit));
            index += 1;
            continue;
        };
        let value = u32::from(character);
        if !range_contains(ranges, &mut range_index, value, previous_value) {
            return None;
        }
        previous_value = Some(value);
        index += width;
    }
    (index > 0).then_some(NativeMatch { start: 0, end: index })
}

fn range_contains(
    ranges: &[std::ops::RangeInclusive<u32>],
    range_index: &mut usize,
    value: u32,
    previous_value: Option<u32>,
) -> bool {
    if previous_value.is_some_and(|previous| value < previous) {
        *range_index = ranges.partition_point(|range| *range.end() < value);
    } else {
        while ranges
            .get(*range_index)
            .is_some_and(|range| *range.end() < value)
        {
            *range_index += 1;
        }
    }
    ranges
        .get(*range_index)
        .is_some_and(|range| range.contains(&value))
}

fn next_code_point(input: &[u16], index: usize, unicode: bool) -> Option<(char, usize)> {
    let first = *input.get(index)?;
    if unicode && (0xD800..=0xDBFF).contains(&first) {
        let second = *input.get(index + 1)?;
        if (0xDC00..=0xDFFF).contains(&second) {
            let code = 0x1_0000
                + ((u32::from(first) - 0xD800) << 10)
                + u32::from(second)
                - 0xDC00;
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

fn find_repeat<T: Copy + PartialEq>(
    input: &[T],
    unit: T,
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
    use super::{find_str, find_units, surrogate_property_matches, test_str, test_units};

    #[test]
    fn literal_scan_respects_sticky_start() {
        assert_eq!(test_str("abc", "", "zabc", 1), Some(true));
        assert_eq!(test_str("abc", "y", "zabc", 1), Some(true));
        assert_eq!(test_str("abc", "y", "zabc", 0), Some(false));
        assert_eq!(test_str("abc", "u", "zabc", 1), Some(true));
        assert_eq!(test_str("abc", "v", "zabc", 1), Some(true));
        assert_eq!(
            test_units("abc", "u", &[b'z' as u16, b'a' as u16, b'b' as u16, b'c' as u16], 1),
            Some(true)
        );
    }

    #[test]
    fn character_class_scan_handles_utf16_surrogates() {
        assert_eq!(test_units("\\D", "", &[0xD800], 0), Some(true));
        assert_eq!(test_units("\\S", "", &[0xD800], 0), Some(true));
        assert_eq!(test_units("\\W", "", &[0xD800], 0), Some(true));
    }

    #[test]
    fn unicode_casefold_word_classes_defer_to_the_engine() {
        assert_eq!(find_str("\\W", "ui", "K", 0), None);
        assert_eq!(find_str("\\w", "ui", "K", 0), None);
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
        assert_eq!(
            find_units("abc", "", &[b'z' as u16, b'a' as u16, b'b' as u16, b'c' as u16], 0),
            Some(super::NativeMatch { start: 1, end: 4 })
        );
        assert_eq!(
            find_units("a", "", &[b'x' as u16, b'a' as u16], 0),
            Some(super::NativeMatch { start: 1, end: 2 })
        );
    }

    #[test]
    fn property_repetition_reuses_compiled_unicode_data() {
        assert!(surrogate_property_matches(
            "General_Category",
            Some("Surrogate")
        ));
        assert_eq!(test_str("^\\p{Assigned}+$", "u", "abc", 0), Some(true));
        assert_eq!(test_str("^\\p{Other}+$", "u", "\0", 0), Some(true));
        assert_eq!(test_str("^\\p{C}+$", "u", "\0", 0), Some(true));
        assert_eq!(test_str("^\\P{Assigned}+$", "u", "\u{38b}", 0), Some(true));
        assert_eq!(
            test_str("^\\p{Script_Extensions=Latin}+$", "u", "Aª", 0),
            Some(true)
        );
        assert_eq!(
            test_units("^\\p{Assigned}+$", "u", &[b'a' as u16, b'b' as u16], 0),
            Some(true)
        );
        assert_eq!(test_units("^\\p{Assigned}+$", "u", &[0xD800], 0), Some(true));
        assert_eq!(
            test_units("^\\p{General_Category=Surrogate}+$", "u", &[0xD800], 0),
            Some(true)
        );
    }

    #[test]
    fn property_ranges_allow_unsorted_input() {
        let text = format!("{}{}", '\u{00AD}', "\0".repeat(300));
        assert_eq!(test_str("^\\p{General_Category=Other}+$", "u", &text, 0), Some(true));

        let mut units = vec![0x00AD];
        units.extend(std::iter::repeat_n(0, 300));
        assert_eq!(test_units("^\\p{General_Category=Other}+$", "u", &units, 0), Some(true));
        assert_eq!(test_str("^\\p{Other}+$", "u", &text, 0), Some(true));
        assert_eq!(test_str("^\\p{C}+$", "u", &text, 0), Some(true));
    }
}
