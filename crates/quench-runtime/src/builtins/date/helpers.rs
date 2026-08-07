//! Date built-in and global utility functions - parseInt and parseFloat helpers.

fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn is_digit_in_radix(c: char, radix: u32) -> Option<u32> {
    let c = c.to_ascii_lowercase();
    let val = if c.is_ascii_digit() {
        c.to_digit(10)?
    } else if c.is_ascii_lowercase() {
        c.to_digit(36)?
    } else {
        return None;
    };
    if val < radix {
        Some(val)
    } else {
        None
    }
}

pub fn spec_parse_int(string: &str, mut radix: u32) -> f64 {
    let s = string.trim_start_matches(is_whitespace);
    if s.is_empty() {
        return f64::NAN;
    }

    let mut chars = s.chars();
    let mut sign = 1f64;

    let first = chars.next().unwrap();
    if first == '-' {
        sign = -1.0;
    } else if first == '+' {
        // positive, sign stays 1.0
    } else {
        let remaining: String = std::iter::once(first).chain(chars).collect();
        if radix == 10 && (remaining.starts_with("0x") || remaining.starts_with("0X")) {
            radix = 16;
        }
        return parse_int_value(&remaining, radix, sign);
    }

    let remaining: String = chars.collect();
    if radix == 10 && (remaining.starts_with("0x") || remaining.starts_with("0X")) {
        radix = 16;
    }
    parse_int_value(&remaining, radix, sign)
}

fn parse_int_value(s: &str, radix: u32, sign: f64) -> f64 {
    let chars = s.chars().peekable();

    let chars: Vec<char> = if radix == 16 {
        let mut c = chars;
        let prefix_chars: Vec<_> = c.by_ref().take(2).collect();
        if prefix_chars.len() == 2
            && prefix_chars[0] == '0'
            && prefix_chars[1].eq_ignore_ascii_case(&'x')
        {
            c.collect()
        } else {
            prefix_chars.into_iter().chain(c).collect()
        }
    } else {
        chars.collect()
    };

    let mut result: f64 = 0.0;
    let mut any_digit = false;

    for c in chars {
        if let Some(val) = is_digit_in_radix(c, radix) {
            result = result * (radix as f64) + (val as f64);
            any_digit = true;
        } else {
            break;
        }
    }

    if !any_digit {
        f64::NAN
    } else {
        result * sign
    }
}

pub fn spec_parse_float(string: &str) -> f64 {
    let s = string.trim_start_matches(is_whitespace);
    if s.is_empty() {
        return f64::NAN;
    }

    let mut chars = s.chars().peekable();
    let sign = parse_float_sign(&mut chars);

    let rest: String = chars.clone().collect();
    if rest == "Infinity" {
        return f64::INFINITY * sign;
    }

    if let Some(val) = try_parse_hex_float(&mut chars) {
        return val * sign;
    }

    let (significand, has_digit) = parse_decimal_significand(&mut chars);
    if !has_digit {
        return f64::NAN;
    }

    let significand = apply_exponent(&mut chars, significand);
    significand * sign
}

fn parse_float_sign(chars: &mut std::iter::Peekable<std::str::Chars>) -> f64 {
    if chars.peek() == Some(&'-') {
        chars.next();
        -1.0
    } else if chars.peek() == Some(&'+') {
        chars.next();
        1.0
    } else {
        1.0
    }
}

fn try_parse_hex_float(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<f64> {
    if chars.peek() != Some(&'0') {
        return None;
    }
    let mut c = chars.clone();
    c.next();
    if !c.peek()?.eq_ignore_ascii_case(&'x') {
        return None;
    }
    chars.next();
    let mut significand = 0.0;
    let mut has_digit = false;

    while let Some(&ch) = chars.peek() {
        if let Some(d) = ch.to_digit(16) {
            significand = significand * 16.0 + (d as f64);
            has_digit = true;
            chars.next();
        } else {
            break;
        }
    }
    if !has_digit {
        return Some(f64::NAN);
    }

    if chars.peek().map(|c| c.to_ascii_lowercase()) == Some('p') {
        chars.next();
        let exp_sign = if chars.peek() == Some(&'-') {
            chars.next();
            -1.0
        } else if chars.peek() == Some(&'+') {
            chars.next();
            1.0
        } else {
            1.0
        };
        let exp = parse_exponent(chars);
        significand *= 10.0_f64.powf(exp * exp_sign);
    }
    Some(significand)
}

fn parse_decimal_significand(chars: &mut std::iter::Peekable<std::str::Chars>) -> (f64, bool) {
    let mut significand = 0.0;
    let mut has_digit = false;
    let mut frac_digits: Vec<u32> = Vec::new();

    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            significand = significand * 10.0 + (c.to_digit(10).unwrap() as f64);
            has_digit = true;
            chars.next();
        } else {
            break;
        }
    }

    if chars.peek() == Some(&'.') {
        chars.next();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                frac_digits.push(c.to_digit(10).unwrap());
                has_digit = true;
                chars.next();
            } else {
                break;
            }
        }
    }
    if !frac_digits.is_empty() {
        for &d in &frac_digits {
            significand = significand * 10.0 + (d as f64);
        }
        significand /= 10f64.powi(frac_digits.len() as i32);
    }
    (significand, has_digit)
}

fn apply_exponent(chars: &mut std::iter::Peekable<std::str::Chars>, significand: f64) -> f64 {
    if chars.peek().map(|c| c.to_ascii_lowercase()) != Some('e') {
        return significand;
    }
    chars.next();
    let exp_sign = if chars.peek() == Some(&'-') {
        chars.next();
        -1.0
    } else if chars.peek() == Some(&'+') {
        chars.next();
        1.0
    } else {
        1.0
    };
    let exp = parse_exponent(chars);
    significand * 10.0_f64.powf(exp * exp_sign)
}

fn parse_exponent(chars: &mut std::iter::Peekable<std::str::Chars>) -> f64 {
    let mut exp: f64 = 0.0;
    while let Some(&c) = chars.peek() {
        if let Some(d) = c.to_digit(10) {
            exp = exp * 10.0 + (d as f64);
            chars.next();
        } else {
            break;
        }
    }
    exp
}
