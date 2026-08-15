use super::Part;

pub(super) fn parts(value: f64, unit: &str, style: &str) -> Vec<Part> {
    let negative = value < 0.0 || (value == 0.0 && value.is_sign_negative());
    let text = polish_number(value.abs());
    let mut parts = super::number_parts(&text);
    for part in &mut parts {
        part.unit = true;
    }
    let word = polish_word(unit, style, value.abs());
    let prefix = if negative { "" } else { "za " };
    let suffix = if negative { " temu" } else { "" };
    let mut result = Vec::new();
    if !prefix.is_empty() {
        result.push(Part {
            ty: "literal",
            value: prefix.to_string(),
            unit: false,
        });
    }
    result.extend(parts);
    result.push(Part {
        ty: "literal",
        value: format!(" {word}{suffix}"),
        unit: false,
    });
    result
}

fn polish_number(value: f64) -> String {
    let grouped = super::grouped_number(value)
        .replace(',', "\u{a0}")
        .replace('.', "|");
    if value < 10_000.0 {
        grouped.replace('\u{a0}', "")
    } else {
        grouped
    }
}

fn polish_word(unit: &str, style: &str, value: f64) -> String {
    if value.fract() != 0.0 {
        return fractional_word(unit, style);
    }
    let plural = polish_plural(value);
    if style != "long" {
        return polish_short_word(unit, style, plural);
    }
    let words = match unit {
        "second" => ["sekundę", "sekundy", "sekund"],
        "minute" => ["minutę", "minuty", "minut"],
        "hour" => ["godzinę", "godziny", "godzin"],
        "day" => ["dzień", "dni", "dni"],
        "week" => ["tydzień", "tygodnie", "tygodni"],
        "month" => ["miesiąc", "miesiące", "miesięcy"],
        "quarter" => ["kwartał", "kwartały", "kwartałów"],
        _ => ["rok", "lata", "lat"],
    };
    words[plural].to_string()
}

fn fractional_word(unit: &str, style: &str) -> String {
    if style != "long" {
        return match unit {
            "second" if style == "narrow" => "s",
            "second" => "sek.",
            "minute" => "min",
            "hour" if style == "narrow" => "g.",
            "hour" => "godz.",
            "day" => "dnia",
            "week" => "tyg.",
            "month" => "mies.",
            "quarter" => "kw.",
            _ => "roku",
        }
        .to_string();
    }
    match unit {
        "day" => "dnia",
        "week" => "tygodnia",
        "month" => "miesiąca",
        "quarter" => "kwartału",
        "year" => "roku",
        "second" if style == "narrow" => "s",
        "second" if style == "short" => "sek.",
        "second" => "sekundy",
        "minute" => "minuty",
        "hour" if style == "narrow" => "g.",
        "hour" if style == "short" => "godz.",
        "hour" => "godziny",
        _ => "lat",
    }
    .to_string()
}

fn polish_short_word(unit: &str, style: &str, plural: usize) -> String {
    let words = match unit {
        "second" if style == "narrow" => ["s", "s", "s"],
        "second" => ["sek.", "sek.", "sek."],
        "minute" => ["min", "min", "min"],
        "hour" if style == "narrow" => ["g.", "g.", "g."],
        "hour" => ["godz.", "godz.", "godz."],
        "day" => ["dzień", "dni", "dni"],
        "week" => ["tydz.", "tyg.", "tyg."],
        "month" => ["mies.", "mies.", "mies."],
        "quarter" => ["kw.", "kw.", "kw."],
        _ => ["rok", "lata", "lat"],
    };
    words[plural].to_string()
}

fn polish_plural(value: f64) -> usize {
    if value == 1.0 {
        return 0;
    }
    let integer = value as u64;
    if value.fract() == 0.0
        && (2..=4).contains(&(integer % 10))
        && !(12..=14).contains(&(integer % 100))
    {
        1
    } else {
        2
    }
}

pub(super) fn unit_word(unit: &str, style: &str, plural: bool) -> String {
    if style == "short" || style == "narrow" {
        short_word(unit, plural)
    } else {
        long_word(unit, plural)
    }
}

fn short_word(unit: &str, plural: bool) -> String {
    const WORDS: &[(&str, bool, &str)] = &[
        ("second", false, "sec."),
        ("second", true, "sec."),
        ("minute", false, "min."),
        ("minute", true, "min."),
        ("hour", false, "hr."),
        ("hour", true, "hr."),
        ("week", false, "wk."),
        ("week", true, "wk."),
        ("month", false, "mo."),
        ("month", true, "mo."),
        ("year", false, "yr."),
        ("year", true, "yr."),
        ("day", false, "day"),
        ("day", true, "days"),
        ("quarter", false, "qtr."),
        ("quarter", true, "qtrs."),
    ];
    WORDS
        .iter()
        .find(|(name, is_plural, _)| *name == unit && *is_plural == plural)
        .map_or_else(|| long_word(unit, plural), |(_, _, word)| word.to_string())
}

fn long_word(unit: &str, plural: bool) -> String {
    let (single, multi) = match unit {
        "second" => ("second", "seconds"),
        "minute" => ("minute", "minutes"),
        "hour" => ("hour", "hours"),
        "day" => ("day", "days"),
        "week" => ("week", "weeks"),
        "month" => ("month", "months"),
        "quarter" => ("quarter", "quarters"),
        "year" => ("year", "years"),
        _ => (unit, unit),
    };
    if plural {
        multi.to_string()
    } else {
        single.to_string()
    }
}
