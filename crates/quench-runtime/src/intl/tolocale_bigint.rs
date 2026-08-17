pub(crate) fn format_bigint(value: &str, locales: &[String], options: Option<&Value>) -> String {
    let (sign, digits) = bigint_digits(value);
    let style = option_string(options.unwrap_or(&Value::Undefined), "style").unwrap_or_default();
    let digits = significant_round(&scaled_digits(digits, &style), options);
    add_fraction_and_style(group_bigint(sign, &digits, locales), locales, options, &style)
}
fn bigint_digits(value: &str) -> (&str, &str) { value.strip_prefix('-').map_or(("", value), |digits| ("-", digits)) }
fn scaled_digits(digits: &str, style: &str) -> String { if style == "percent" { format!("{digits}00") } else { digits.to_string() } }
fn group_bigint(sign: &str, digits: &str, locales: &[String]) -> String {
    let separator = locales.first().map_or(',', |locale| if locale.starts_with("de") || locale.starts_with("es") { '.' } else { ',' });
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() { if index > 0 && (digits.len() - index) % 3 == 0 { grouped.push(separator); } grouped.push(digit); }
    format!("{sign}{grouped}")
}
fn add_fraction_and_style(mut formatted: String, locales: &[String], options: Option<&Value>, style: &str) -> String {
    if style != "percent" { add_minimum_fraction(&mut formatted, locales, options); }
    if style == "percent" && locales.first().is_some_and(|locale| locale.starts_with("de")) { format!("{formatted}\u{a0}%") } else if style == "percent" { format!("{formatted}%") } else { formatted }
}
fn add_minimum_fraction(formatted: &mut String, locales: &[String], options: Option<&Value>) {
    let Some(digits) = options.and_then(|value| option_string(value, "minimumFractionDigits")).and_then(|value| value.parse::<usize>().ok()) else { return; };
    if digits == 0 { return; }
    let separator = locales.first().is_some_and(|locale| locale.starts_with("de")).then_some(',').unwrap_or('.');
    formatted.push(separator); formatted.extend(std::iter::repeat('0').take(digits));
}
fn option_string(value: &Value, key: &str) -> Option<String> {
    let Value::Object(properties) = value else { return None; };
    properties.iter().find(|(name, _)| name == key).map(|(_, value)| super::to_string_value(value))
}
fn significant_round(digits: &str, options: Option<&Value>) -> String {
    let Some(limit) = options.and_then(|value| option_string(value, "maximumSignificantDigits")).and_then(|value| value.parse::<usize>().ok()) else { return digits.to_string(); };
    if limit == 0 || digits.len() <= limit { return digits.to_string(); }
    let mut kept = digits.as_bytes()[..limit].to_vec(); if digits.as_bytes()[limit] >= b'5' { round_digits(&mut kept); }
    let mut result = String::from_utf8_lossy(&kept).into_owned(); result.extend(std::iter::repeat('0').take(digits.len() - limit)); result
}
fn round_digits(digits: &mut [u8]) { for digit in digits.iter_mut().rev() { if *digit < b'9' { *digit += 1; return; } *digit = b'0'; } }
