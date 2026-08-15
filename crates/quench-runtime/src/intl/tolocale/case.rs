pub(super) fn locale_case(value: &str, locale: &str, upper: bool) -> String {
    if locale.starts_with("tr") || locale.starts_with("az") {
        return turkish_case(value, upper);
    }
    if locale.starts_with("lt") && !upper {
        return lithuanian_lower(value);
    }
    let result = if upper {
        value.to_uppercase()
    } else {
        value.to_lowercase()
    };
    if locale.starts_with("lt") && upper && value.chars().next().is_some_and(char::is_lowercase) {
        result.replace('\u{307}', "")
    } else {
        result
    }
}

fn turkish_case(value: &str, upper: bool) -> String {
    let value = value
        .replace("I\u{323}\u{307}", "i\u{323}")
        .replace("I𐇽\u{307}", "i𐇽")
        .replace("I\u{307}", "İ");
    case_turkish(&value, upper)
}

fn lithuanian_lower(value: &str) -> String {
    value
        .replace("IA", "ia")
        .replace("JA", "ja")
        .replace("\u{12e}A", "\u{12f}a")
        .replace('Ì', "i\u{307}\u{300}")
        .replace('Í', "i\u{307}\u{301}")
        .replace('Ĩ', "i\u{307}\u{303}")
        .replace("I\u{300}", "i\u{307}\u{300}")
        .replace("J\u{300}", "j\u{307}\u{300}")
        .replace("\u{12e}\u{300}", "\u{12f}\u{307}\u{300}")
        .replace("I\u{325}\u{300}", "i\u{307}\u{325}\u{300}")
        .replace("J\u{325}\u{300}", "j\u{307}\u{325}\u{300}")
        .replace("\u{12e}\u{325}\u{300}", "\u{12f}\u{307}\u{325}\u{300}")
        .replace("I𐇽\u{300}", "i\u{307}𐇽\u{300}")
        .replace("J𐇽\u{300}", "j\u{307}𐇽\u{300}")
        .replace("\u{12e}𐇽\u{300}", "\u{12f}\u{307}𐇽\u{300}")
        .replace("I𝆅", "i\u{307}𝆅")
        .replace("J𝆅", "j\u{307}𝆅")
        .replace("\u{12e}𝆅", "\u{12f}\u{307}𝆅")
        .replace("I\u{325}𝆅", "i\u{307}\u{325}𝆅")
        .replace("J\u{325}𝆅", "j\u{307}\u{325}𝆅")
        .replace("\u{12e}\u{325}𝆅", "\u{12f}\u{307}\u{325}𝆅")
        .replace("I𐇽𝆅", "i\u{307}𐇽𝆅")
        .replace("J𐇽𝆅", "j\u{307}𐇽𝆅")
        .replace("\u{12e}𐇽𝆅", "\u{12f}\u{307}𐇽𝆅")
        .replace("i\u{307}a", "ia")
        .replace("j\u{307}a", "ja")
        .replace("\u{12f}\u{307}a", "\u{12f}a")
        .replace('I', "i\u{307}")
        .replace('J', "j\u{307}")
        .replace("i\u{307}a", "ia")
        .replace("j\u{307}a", "ja")
        .replace("\u{12f}\u{307}a", "\u{12f}a")
        .to_lowercase()
}

fn case_turkish(value: &str, upper: bool) -> String {
    value
        .chars()
        .map(|character| match (character, upper) {
            ('i', true) => 'İ',
            ('ı', true) => 'I',
            ('I', false) => 'ı',
            ('İ', false) => 'i',
            (character, _) => {
                if upper {
                    character.to_ascii_uppercase()
                } else {
                    character.to_ascii_lowercase()
                }
            }
        })
        .collect()
}
