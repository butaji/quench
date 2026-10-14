//! Node-compatible legacy `Date.parse` fallback, following V8's `DateParser`
//! legacy loop (src/date/dateparser-inl.h): a token stream feeds day, time and
//! time-zone composers, which then validate and write date components.

use super::{
    DATE_COMPONENT_COUNT, DAY_COMPONENT, HOUR_COMPONENT, MILLISECOND_COMPONENT, MINUTE_COMPONENT,
    MONTH_COMPONENT, SECOND_COMPONENT, YEAR_COMPONENT,
};

/// Keyword lookup compares this many lowercased leading characters.
const KEYWORD_PREFIX_LENGTH: usize = 3;
/// Numerals keep only this many significant digits.
const MAX_SIGNIFICANT_DIGITS: usize = 9;
const DECIMAL_RADIX: u32 = 10;
/// Fractional seconds are read as this many digits of milliseconds.
const MILLISECOND_DIGITS: usize = 3;
/// Two-digit years: 0..=49 map into the 2000s, 50..=99 into the 1900s.
const TWO_DIGIT_YEAR_PIVOT: i64 = 49;
const TWO_DIGIT_YEAR_MAX: i64 = 99;
const TWENTY_FIRST_CENTURY: i64 = 2000;
const TWENTIETH_CENTURY: i64 = 1900;
/// Day and month components default to the first day and month.
const DEFAULT_DAY_COMPONENT: i64 = 1;
const MAX_MONTH: i64 = 12;
const MAX_DAY: i64 = 31;
const MAX_HOUR: i64 = 23;
const MAX_HOUR_12: i64 = 12;
const HOURS_PER_HALF_DAY: i64 = 12;
/// Hour 24 is accepted only as exactly 24:00:00.000.
const END_OF_DAY_HOUR: i64 = 24;
const MAX_MINUTE: i64 = 59;
const MAX_SECOND: i64 = 59;
const MAX_MILLISECOND: i64 = 999;
/// A four-digit zone offset such as `0800` is `hhmm`.
const ZONE_HHMM_DIVISOR: i64 = 100;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3600;
const MILLISECONDS_PER_SECOND: i64 = 1000;
/// V8 rejects zone offsets beyond its small-integer range.
const MAX_ZONE_OFFSET_SECONDS: i64 = i32::MAX as i64;
/// Offset widths: one or two digits are hours, three or four are `hhmm`.
const ZONE_HOUR_DIGITS: std::ops::RangeInclusive<usize> = 1..=2;
const ZONE_HHMM_DIGITS: std::ops::RangeInclusive<usize> = 3..=4;
const TIME_COMPONENT_COUNT: usize = 4;
const DAY_COMPONENT_COUNT: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Keyword {
    Month(i64),
    /// Hour offset added after reducing the hour modulo twelve.
    AmPm(i64),
    /// Zone offset in hours.
    Zone(i64),
    TimeSeparator,
    Unknown,
}

const KEYWORDS: &[(&str, Keyword)] = &[
    ("jan", Keyword::Month(1)),
    ("feb", Keyword::Month(2)),
    ("mar", Keyword::Month(3)),
    ("apr", Keyword::Month(4)),
    ("may", Keyword::Month(5)),
    ("jun", Keyword::Month(6)),
    ("jul", Keyword::Month(7)),
    ("aug", Keyword::Month(8)),
    ("sep", Keyword::Month(9)),
    ("oct", Keyword::Month(10)),
    ("nov", Keyword::Month(11)),
    ("dec", Keyword::Month(12)),
    ("am", Keyword::AmPm(0)),
    ("pm", Keyword::AmPm(HOURS_PER_HALF_DAY)),
    ("ut", Keyword::Zone(0)),
    ("utc", Keyword::Zone(0)),
    ("z", Keyword::Zone(0)),
    ("gmt", Keyword::Zone(0)),
    ("cdt", Keyword::Zone(-5)),
    ("cst", Keyword::Zone(-6)),
    ("edt", Keyword::Zone(-4)),
    ("est", Keyword::Zone(-5)),
    ("mdt", Keyword::Zone(-6)),
    ("mst", Keyword::Zone(-7)),
    ("pdt", Keyword::Zone(-7)),
    ("pst", Keyword::Zone(-8)),
    ("t", Keyword::TimeSeparator),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    Number { value: i64, length: usize },
    Symbol(char),
    Word { keyword: Keyword, length: usize },
    WhiteSpace,
    Unknown,
    End,
}

impl Token {
    fn is_sign(self) -> Option<i64> {
        match self {
            Token::Symbol('+') => Some(1),
            Token::Symbol('-') => Some(-1),
            _ => None,
        }
    }
}

fn is_word_char(ch: char) -> bool {
    ch >= 'A' && !is_date_white_space(ch)
}

fn is_date_white_space(ch: char) -> bool {
    ch.is_whitespace() || ch == '\u{feff}'
}

struct Tokenizer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    next: Token,
}

impl<'a> Tokenizer<'a> {
    fn new(text: &'a str) -> Self {
        let mut tokenizer = Self {
            chars: text.chars().peekable(),
            next: Token::End,
        };
        tokenizer.next = tokenizer.read();
        tokenizer
    }

    fn next(&mut self) -> Token {
        let token = self.next;
        self.next = self.read();
        token
    }

    fn peek(&self) -> Token {
        self.next
    }

    fn skip_symbol(&mut self, symbol: char) -> bool {
        if self.next == Token::Symbol(symbol) {
            self.next();
            true
        } else {
            false
        }
    }

    fn read(&mut self) -> Token {
        let Some(&ch) = self.chars.peek() else {
            return Token::End;
        };
        if ch.is_ascii_digit() {
            return self.read_number();
        }
        if matches!(ch, ':' | '-' | '+' | '.' | ')') {
            self.chars.next();
            return Token::Symbol(ch);
        }
        if is_word_char(ch) {
            return self.read_word();
        }
        if is_date_white_space(ch) {
            while self.chars.next_if(|&ch| is_date_white_space(ch)).is_some() {}
            return Token::WhiteSpace;
        }
        if ch == '(' {
            let mut balance = 0usize;
            while let Some(ch) = self.chars.next() {
                match ch {
                    '(' => balance += 1,
                    ')' => balance -= 1,
                    _ => {}
                }
                if balance == 0 {
                    break;
                }
            }
            return Token::Unknown;
        }
        self.chars.next();
        Token::Unknown
    }

    fn read_number(&mut self) -> Token {
        let mut length = 0;
        while self.chars.next_if_eq(&'0').is_some() {
            length += 1;
        }
        let mut value = 0;
        let mut significant = 0;
        while let Some(digit) = self.chars.peek().and_then(|ch| ch.to_digit(DECIMAL_RADIX)) {
            if significant < MAX_SIGNIFICANT_DIGITS {
                value = value * i64::from(DECIMAL_RADIX) + i64::from(digit);
            }
            significant += 1;
            length += 1;
            self.chars.next();
        }
        Token::Number { value, length }
    }

    fn read_word(&mut self) -> Token {
        let mut prefix = String::new();
        let mut length = 0;
        while let Some(ch) = self.chars.next_if(|&ch| is_word_char(ch)) {
            if length < KEYWORD_PREFIX_LENGTH {
                prefix.push(ch.to_ascii_lowercase());
            }
            length += 1;
        }
        let keyword = KEYWORDS
            .iter()
            .find(|(name, keyword)| {
                *name == prefix
                    && (length <= KEYWORD_PREFIX_LENGTH || matches!(keyword, Keyword::Month(_)))
            })
            .map_or(Keyword::Unknown, |&(_, keyword)| keyword);
        Token::Word { keyword, length }
    }
}

/// Scales a fractional-second numeral to its three most significant digits.
fn read_milliseconds(value: i64, length: usize) -> i64 {
    let length = length.min(MAX_SIGNIFICANT_DIGITS);
    let scale = i64::from(DECIMAL_RADIX).pow(MILLISECOND_DIGITS.abs_diff(length) as u32);
    if length < MILLISECOND_DIGITS {
        value * scale
    } else {
        value / scale
    }
}

#[derive(Default)]
struct DayComposer {
    components: Vec<i64>,
    named_month: Option<i64>,
}

impl DayComposer {
    fn add(&mut self, value: i64) -> bool {
        let fits = self.components.len() < DAY_COMPONENT_COUNT;
        if fits {
            self.components.push(value);
        }
        fits
    }

    fn write(mut self, output: &mut [f64; DATE_COMPONENT_COUNT]) -> bool {
        if self.components.is_empty() {
            return false;
        }
        self.components
            .resize(DAY_COMPONENT_COUNT, DEFAULT_DAY_COMPONENT);
        let [first, second, third] = [self.components[0], self.components[1], self.components[2]];
        let (year, month, day) = match self.named_month {
            None if !is_day(first) => (first, second, third),
            None => (third, first, second),
            Some(month) if !is_day(first) => (first, month, second),
            Some(month) => (second, month, first),
        };
        let year = match year {
            0..=TWO_DIGIT_YEAR_PIVOT => year + TWENTY_FIRST_CENTURY,
            ..=TWO_DIGIT_YEAR_MAX => year + TWENTIETH_CENTURY,
            _ => year,
        };
        if !(1..=MAX_MONTH).contains(&month) || !is_day(day) {
            return false;
        }
        output[YEAR_COMPONENT] = year as f64;
        output[MONTH_COMPONENT] = (month - 1) as f64;
        output[DAY_COMPONENT] = day as f64;
        true
    }
}

fn is_day(value: i64) -> bool {
    (1..=MAX_DAY).contains(&value)
}

fn is_minute(value: i64) -> bool {
    (0..=MAX_MINUTE).contains(&value)
}

#[derive(Default)]
struct TimeComposer {
    components: Vec<i64>,
    hour_offset: Option<i64>,
}

impl TimeComposer {
    fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    fn is_expecting(&self, value: i64) -> bool {
        match self.components.len() {
            1 => is_minute(value),
            2 => (0..=MAX_SECOND).contains(&value),
            3 => (0..=MAX_MILLISECOND).contains(&value),
            _ => false,
        }
    }

    fn add(&mut self, value: i64) -> bool {
        let fits = self.components.len() < TIME_COMPONENT_COUNT;
        if fits {
            self.components.push(value);
        }
        fits
    }

    fn add_final(&mut self, value: i64) -> bool {
        let added = self.add(value);
        if added {
            self.components.resize(TIME_COMPONENT_COUNT, 0);
        }
        added
    }

    fn write(mut self, output: &mut [f64; DATE_COMPONENT_COUNT]) -> bool {
        self.components.resize(TIME_COMPONENT_COUNT, 0);
        let [mut hour, minute, second, millisecond] = [
            self.components[0],
            self.components[1],
            self.components[2],
            self.components[3],
        ];
        if let Some(offset) = self.hour_offset {
            if !(0..=MAX_HOUR_12).contains(&hour) {
                return false;
            }
            hour = hour % HOURS_PER_HALF_DAY + offset;
        }
        let in_range = (0..=MAX_HOUR).contains(&hour)
            && is_minute(minute)
            && (0..=MAX_SECOND).contains(&second)
            && (0..=MAX_MILLISECOND).contains(&millisecond);
        if !in_range && (hour, minute, second, millisecond) != (END_OF_DAY_HOUR, 0, 0, 0) {
            return false;
        }
        output[HOUR_COMPONENT] = hour as f64;
        output[MINUTE_COMPONENT] = minute as f64;
        output[SECOND_COMPONENT] = second as f64;
        output[MILLISECOND_COMPONENT] = millisecond as f64;
        true
    }
}

#[derive(Default)]
struct ZoneComposer {
    sign: Option<i64>,
    hour: Option<i64>,
    minute: Option<i64>,
}

impl ZoneComposer {
    fn set_hours(&mut self, offset_hours: i64) {
        self.sign = Some(if offset_hours < 0 { -1 } else { 1 });
        self.hour = Some(offset_hours.abs());
        self.minute = Some(0);
    }

    fn is_expecting(&self, value: i64) -> bool {
        self.hour.is_some() && self.minute.is_none() && is_minute(value)
    }

    fn is_utc(&self) -> bool {
        self.hour == Some(0) && self.minute == Some(0)
    }

    /// The UTC offset in milliseconds, or `None` for local time.
    fn write(self) -> Option<Option<i64>> {
        let Some(sign) = self.sign else {
            return Some(None);
        };
        let seconds = self.hour.unwrap_or(0) * SECONDS_PER_HOUR
            + self.minute.unwrap_or(0) * SECONDS_PER_MINUTE;
        (seconds <= MAX_ZONE_OFFSET_SECONDS)
            .then_some(Some(sign * seconds * MILLISECONDS_PER_SECOND))
    }
}

/// Parses a legacy date string into date components and an optional UTC
/// offset in milliseconds; `None` offset means the components are local time.
pub(super) fn parse_legacy_date(text: &str) -> Option<([f64; DATE_COMPONENT_COUNT], Option<i64>)> {
    let mut scanner = Tokenizer::new(text);
    let mut day = DayComposer::default();
    let mut time = TimeComposer::default();
    let mut zone = ZoneComposer::default();
    let mut has_read_number = false;
    loop {
        let token = scanner.next();
        match token {
            Token::End => break,
            Token::Number { value, .. } => {
                has_read_number = true;
                if scanner.skip_symbol(':') {
                    if scanner.skip_symbol(':') {
                        if !time.is_empty() {
                            return None;
                        }
                        time.add(value);
                        time.add(0);
                    } else {
                        if !time.add(value) {
                            return None;
                        }
                        scanner.skip_symbol('.');
                    }
                } else if scanner.peek() == Token::Symbol('.') && time.is_expecting(value) {
                    scanner.next();
                    time.add(value);
                    let Token::Number { value, length } = scanner.next() else {
                        return None;
                    };
                    time.add_final(read_milliseconds(value, length));
                } else {
                    // V8 consumes a '.' after a number that is not a time part.
                    scanner.skip_symbol('.');
                    if !add_unattached_number(&mut scanner, &mut day, &mut time, &mut zone, value) {
                        return None;
                    }
                }
            }
            Token::Word { keyword, .. } => match keyword {
                Keyword::AmPm(offset) if !time.is_empty() => time.hour_offset = Some(offset),
                Keyword::Month(month) => {
                    day.named_month = Some(month);
                    scanner.skip_symbol('-');
                }
                Keyword::Zone(offset) if has_read_number => zone.set_hours(offset),
                _ => {
                    // Garbage words are illegal after a number, and must be
                    // separated from a following number.
                    if has_read_number || matches!(scanner.peek(), Token::Number { .. }) {
                        return None;
                    }
                }
            },
            _ if token.is_sign().is_some() && (zone.is_utc() || !time.is_empty()) => {
                zone.sign = token.is_sign();
                let (value, length) = match scanner.peek() {
                    Token::Number { value, length } => {
                        scanner.next();
                        (value, length)
                    }
                    _ => (0, 0),
                };
                has_read_number = true;
                if scanner.peek() == Token::Symbol(':') {
                    zone.hour = Some(value);
                    zone.minute = None;
                } else if ZONE_HOUR_DIGITS.contains(&length) {
                    zone.hour = Some(value);
                    zone.minute = Some(0);
                } else if ZONE_HHMM_DIGITS.contains(&length) {
                    zone.hour = Some(value / ZONE_HHMM_DIVISOR);
                    zone.minute = Some(value % ZONE_HHMM_DIVISOR);
                } else {
                    return None;
                }
            }
            _ if (token.is_sign().is_some() || token == Token::Symbol(')')) && has_read_number => {
                return None;
            }
            _ => {}
        }
    }
    let mut components = [0.0; DATE_COMPONENT_COUNT];
    if !day.write(&mut components) || !time.write(&mut components) {
        return None;
    }
    Some((components, zone.write()?))
}

/// A number not attached to `:`-separated time syntax: a zone minute, the
/// final time part, or a day component.
fn add_unattached_number(
    scanner: &mut Tokenizer<'_>,
    day: &mut DayComposer,
    time: &mut TimeComposer,
    zone: &mut ZoneComposer,
    value: i64,
) -> bool {
    if zone.is_expecting(value) {
        zone.minute = Some(value);
    } else if time.is_expecting(value) {
        time.add_final(value);
        let follows = scanner.peek();
        let terminated = matches!(follows, Token::End | Token::WhiteSpace)
            || matches!(
                follows,
                Token::Word {
                    keyword: Keyword::Zone(0),
                    length: 1
                }
            )
            || follows.is_sign().is_some();
        if !terminated {
            return false;
        }
    } else {
        if !day.add(value) {
            return false;
        }
        scanner.skip_symbol('-');
    }
    true
}
