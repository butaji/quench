pub const CALENDARS: &[&str] = &[
    "buddhist",
    "chinese",
    "coptic",
    "dangi",
    "ethioaa",
    "ethiopic",
    "gregory",
    "hebrew",
    "indian",
    "islamic-civil",
    "islamic-tbla",
    "islamic-umalqura",
    "iso8601",
    "japanese",
    "persian",
    "roc",
];

pub const NUMBERING_SYSTEMS: &[&str] = &[
    "adlm", "ahom", "arab", "arabext", "bali", "beng", "bhks", "brah", "cakm", "cham", "deva",
    "diak", "fullwide", "gara", "gong", "gonm", "gujr", "gukh", "guru", "hanidec", "hmng", "hmnp",
    "java", "kali", "kawi", "khmr", "knda", "krai", "lana", "lanatham", "laoo", "latn", "lepc",
    "limb", "mathbold", "mathdbl", "mathmono", "mathsanb", "mathsans", "mlym", "modi", "mong",
    "mroo", "mtei", "mymr", "mymrepka", "mymrpao", "mymrshan", "mymrtlng", "nagm", "newa", "nkoo",
    "olck", "onao", "orya", "osma", "outlined", "rohg", "saur", "segment", "shrd", "sind", "sinh",
    "sora", "sund", "sunu", "takr", "talu", "tamldec", "telu", "thai", "tibt", "tirh", "tnsa",
    "tols", "vaii", "wara", "wcho",
];

const DEFAULT_NUMBERING_SYSTEMS: &[(&str, &str)] = &[
    ("ar", "arab"),
    ("bn", "beng"),
    ("fa", "arabext"),
    ("gu", "gujr"),
    ("hi", "deva"),
    ("mr", "deva"),
    ("my", "mymr"),
    ("ne", "deva"),
    ("pa", "guru"),
    ("ta", "tamldec"),
    ("te", "telu"),
    ("th", "thai"),
    ("ur", "arabext"),
];

const UNICODE_EXTENSION_KEY_LENGTH: usize = 2;

pub fn unicode_extension_value(locale: &str, key: &str) -> Option<String> {
    let (_, extension) = locale.split_once("-u-")?;
    let parts = extension.split('-').collect::<Vec<_>>();
    let position = parts.iter().position(|part| *part == key)?;
    let value = parts[position + 1..]
        .iter()
        .take_while(|part| part.len() != UNICODE_EXTENSION_KEY_LENGTH)
        .copied()
        .collect::<Vec<_>>();
    (!value.is_empty()).then(|| value.join("-"))
}

pub fn default_numbering_system(locale: &str) -> &'static str {
    let language = locale.split(['-', '_']).next().unwrap_or_default();
    DEFAULT_NUMBERING_SYSTEMS
        .iter()
        .find_map(|(candidate, numbering)| (*candidate == language).then_some(*numbering))
        .unwrap_or("latn")
}

pub fn calendar_alias(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "islamicc" | "islamic" | "islamic-rgsa" => "islamic-civil".into(),
        "ethiopic-amete-alem" => "ethioaa".into(),
        other => other.into(),
    }
}

pub fn valid_unicode_type(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            (3..=8).contains(&part.len())
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
}

pub fn valid_numbering_system(value: &str) -> bool {
    NUMBERING_SYSTEMS.contains(&value)
}

pub fn valid_calendar(value: &str) -> bool {
    CALENDARS.contains(&value)
}

pub fn sanitize_datetime_locale(locale: &str) -> String {
    let Some((base, extension)) = locale.split_once("-u-") else {
        return locale.to_string();
    };
    let parts = extension.split('-').collect::<Vec<_>>();
    let mut retained = Vec::new();
    let mut position = 0;
    while position < parts.len() {
        if parts[position].len() != 2 {
            position += 1;
            continue;
        }
        let key = parts[position];
        position += 1;
        let start = position;
        while position < parts.len() && parts[position].len() != 2 {
            position += 1;
        }
        let value = parts[start..position].join("-");
        let value = match key {
            "ca" => {
                let value = calendar_alias(&value);
                valid_calendar(&value).then_some(value)
            }
            "nu" => {
                let value = value.to_ascii_lowercase();
                valid_numbering_system(&value).then_some(value)
            }
            "hc" if matches!(value.as_str(), "h11" | "h12" | "h23" | "h24") => Some(value),
            _ => None,
        };
        if let Some(value) = value {
            retained.extend([key.to_string(), value]);
        }
    }
    if retained.is_empty() {
        base.to_string()
    } else {
        format!("{base}-u-{}", retained.join("-"))
    }
}
