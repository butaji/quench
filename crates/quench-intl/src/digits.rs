const DIGIT_BASES: &[(&str, u32)] = &[
    ("adlm", 0x1e950),
    ("ahom", 0x11730),
    ("arab", 0x660),
    ("arabext", 0x6f0),
    ("bali", 0x1b50),
    ("beng", 0x9e6),
    ("bhks", 0x11c50),
    ("brah", 0x11066),
    ("cakm", 0x11136),
    ("cham", 0xaa50),
    ("deva", 0x966),
    ("diak", 0x11950),
    ("fullwide", 0xff10),
    ("gara", 0x10d40),
    ("gong", 0x11da0),
    ("gonm", 0x11d50),
    ("gujr", 0xae6),
    ("gukh", 0x16130),
    ("guru", 0xa66),
    ("hmng", 0x16b50),
    ("hmnp", 0x1e140),
    ("java", 0xa9d0),
    ("kali", 0xa900),
    ("kawi", 0x11f50),
    ("khmr", 0x17e0),
    ("knda", 0xce6),
    ("krai", 0x16d70),
    ("lana", 0x1a80),
    ("lanatham", 0x1a90),
    ("laoo", 0xed0),
    ("latn", 0x30),
    ("lepc", 0x1c40),
    ("limb", 0x1946),
    ("mathbold", 0x1d7ce),
    ("mathdbl", 0x1d7d8),
    ("mathmono", 0x1d7f6),
    ("mathsanb", 0x1d7ec),
    ("mathsans", 0x1d7e2),
    ("mlym", 0xd66),
    ("modi", 0x11650),
    ("mong", 0x1810),
    ("mroo", 0x16a60),
    ("mymr", 0x1040),
    ("mtei", 0xabf0),
    ("mymrepka", 0x116da),
    ("mymrpao", 0x116d0),
    ("mymrshan", 0x1090),
    ("mymrtlng", 0xa9f0),
    ("nagm", 0x1e4f0),
    ("onao", 0x1e5f1),
    ("outlined", 0x1ccf0),
    ("shrd", 0x111d0),
    ("sind", 0x112f0),
    ("newa", 0x11450),
    ("nkoo", 0x7c0),
    ("olck", 0x1c50),
    ("orya", 0xb66),
    ("osma", 0x104a0),
    ("rohg", 0x10d30),
    ("saur", 0xa8d0),
    ("segment", 0x1fbf0),
    ("sinh", 0xde6),
    ("sora", 0x110f0),
    ("sund", 0x1bb0),
    ("sunu", 0x11bf0),
    ("takr", 0x116c0),
    ("talu", 0x19d0),
    ("tamldec", 0xbe6),
    ("telu", 0xc66),
    ("thai", 0xe50),
    ("tibt", 0xf20),
    ("tirh", 0x114d0),
    ("tnsa", 0x16ac0),
    ("tols", 0x11de0),
    ("vaii", 0xa620),
    ("wara", 0x118e0),
    ("wcho", 0x1e2f0),
];

pub fn localize_digits(text: String, numbering_system: &str) -> String {
    if numbering_system == "hanidec" {
        return text
            .chars()
            .map(|character| {
                "〇一二三四五六七八九"
                    .chars()
                    .nth(character.to_digit(10).unwrap_or(10) as usize)
                    .unwrap_or(character)
            })
            .collect();
    }
    let Some((_, base)) = DIGIT_BASES
        .iter()
        .find(|(name, _)| *name == numbering_system)
    else {
        return text;
    };
    text.chars()
        .map(|character| {
            character
                .to_digit(10)
                .and_then(|digit| char::from_u32(base + digit))
                .unwrap_or(character)
        })
        .collect()
}
