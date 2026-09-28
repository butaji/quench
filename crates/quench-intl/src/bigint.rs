const DECIMAL_GROUP_SIZE: usize = 3;
const PERCENT_SCALE_SUFFIX: &str = "00";
const PERCENT_SIGN: &str = "%";
const GERMAN_PERCENT_SIGN: &str = "\u{a0}%";

pub struct BigIntFormatOptions<'a> {
    pub locale: &'a str,
    pub style: &'a str,
    pub minimum_fraction_digits: usize,
    pub maximum_significant_digits: Option<usize>,
}

pub fn format_bigint(value: &str, options: &BigIntFormatOptions<'_>) -> String {
    let (sign, digits) = value
        .strip_prefix('-')
        .map_or(("", value), |digits| ("-", digits));
    let scaled = if options.style == "percent" {
        format!("{digits}{PERCENT_SCALE_SUFFIX}")
    } else {
        digits.to_owned()
    };
    let rounded = significant_round(&scaled, options.maximum_significant_digits);
    let grouped = group_integer(sign, &rounded, options.locale);
    add_fraction_and_style(grouped, options)
}

fn group_integer(sign: &str, digits: &str, locale: &str) -> String {
    let separator = if locale.starts_with("de") || locale.starts_with("es") {
        '.'
    } else {
        ','
    };
    let mut grouped = String::with_capacity(digits.len() + digits.len() / DECIMAL_GROUP_SIZE);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(DECIMAL_GROUP_SIZE) {
            grouped.push(separator);
        }
        grouped.push(digit);
    }
    format!("{sign}{grouped}")
}

fn add_fraction_and_style(mut formatted: String, options: &BigIntFormatOptions<'_>) -> String {
    if options.style == "percent" {
        return formatted
            + if options.locale.starts_with("de") {
                GERMAN_PERCENT_SIGN
            } else {
                PERCENT_SIGN
            };
    }
    add_minimum_fraction(
        &mut formatted,
        options.minimum_fraction_digits,
        options.locale,
    );
    formatted
}

fn add_minimum_fraction(formatted: &mut String, minimum: usize, locale: &str) {
    if minimum == 0 {
        return;
    }
    let separator = if locale.starts_with("de") { ',' } else { '.' };
    formatted.push(separator);
    formatted.extend(std::iter::repeat_n('0', minimum));
}

fn significant_round(digits: &str, limit: Option<usize>) -> String {
    let Some(limit) = limit.filter(|limit| *limit > 0 && digits.len() > *limit) else {
        return digits.to_owned();
    };
    let mut kept = digits.as_bytes()[..limit].to_vec();
    if digits.as_bytes()[limit] >= b'5' {
        round_digits(&mut kept);
    }
    let mut result = String::from_utf8_lossy(&kept).into_owned();
    result.extend(std::iter::repeat_n('0', digits.len() - limit));
    result
}

fn round_digits(digits: &mut [u8]) {
    for digit in digits.iter_mut().rev() {
        if *digit < b'9' {
            *digit += 1;
            return;
        }
        *digit = b'0';
    }
}
