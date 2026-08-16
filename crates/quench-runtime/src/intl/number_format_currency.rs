pub(crate) fn format_currency(
    text: &str,
    currency: Option<&str>,
    display: &str,
    locale: &str,
    currency_sign: &str,
) -> String {
    let (sign, text) = text.strip_prefix('-').map_or_else(
        || {
            text.strip_prefix('+')
                .map_or(("", text), |rest| ("+", rest))
        },
        |rest| ("-", rest),
    );
    let text = if locale.starts_with("de") || locale.starts_with("pt") {
        text.replace('.', ",")
    } else {
        text.to_string()
    };
    let symbol = currency_symbol(currency, display, locale);
    let formatted = if locale.starts_with("de") || locale.starts_with("pt") {
        format!("{text}\u{a0}{symbol}")
    } else {
        format!("{symbol}{text}")
    };
    if sign == "-" && currency_sign == "accounting" && !locale.starts_with("de") {
        format!("({formatted})")
    } else {
        format!("{sign}{formatted}")
    }
}

fn currency_symbol<'a>(currency: Option<&'a str>, display: &str, locale: &str) -> &'a str {
    let symbol = match display {
        "code" | "name" => currency.unwrap_or("USD"),
        _ => match currency {
            Some("USD") => "$",
            Some("EUR") => "€",
            Some("JPY") => "¥",
            Some("GBP") => "£",
            Some("CNY") => "¥",
            Some("INR") => "₹",
            Some("RUB") => "₽",
            Some("KRW") => "₩",
            _ => currency.unwrap_or("USD"),
        },
    };
    if (locale.starts_with("ko") || locale.starts_with("zh")) && currency == Some("USD") {
        "US$"
    } else {
        symbol
    }
}
