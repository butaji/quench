fn dedupe(locales: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    locales
        .into_iter()
        .filter(|tag| seen.insert(tag.clone()))
        .collect()
}

pub(crate) fn to_string_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::StringUnits(value) => String::from_utf16_lossy(value),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Array(values) => values
            .snapshot()
            .iter()
            .map(to_string_value)
            .collect::<Vec<_>>()
            .join(","),
        _ => "[object Object]".to_string(),
    }
}

/// Canonicalize a single BCP-47 language tag.
pub(crate) fn canonicalize(tag: &str) -> Result<String, VmError> {
    match tag.to_ascii_lowercase().as_str() {
        "art-lojban" => return Ok("jbo".to_string()),
        "cel-gaulish" => return Ok("xtg".to_string()),
        "zh-guoyu" => return Ok("zh".to_string()),
        "zh-hakka" => return Ok("hak".to_string()),
        "zh-xiang" => return Ok("hsn".to_string()),
        "en-gb-oed" | "zh-min" | "i-default" => {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        _ => {}
    }
    if tag.is_empty()
        || tag.eq_ignore_ascii_case("nan")
        || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    let mut parts = tag.split('-');
    let language = parts
        .next()
        .ok_or_else(|| runtime_error("RangeError: invalid language tag"))?;
    if language.is_empty()
        || language.len() < 2
        || language.len() > 8
        || !language.chars().all(|c| c.is_ascii_alphabetic())
    {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    let parts: Vec<&str> = parts.collect();
    let mut out = Vec::new();
    let mut script_done = false;
    if language.eq_ignore_ascii_case("sh") {
        out.push("sr".to_string());
        if !parts.first().is_some_and(|part| {
            part.len() == 4
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
        }) {
            out.push("Latn".to_string());
            script_done = true;
        }
    } else if language.eq_ignore_ascii_case("cnr") {
        out.push("sr".to_string());
        if !parts.first().is_some_and(|part| {
            (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
                || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
        }) {
            out.push("ME".to_string());
        }
    } else {
        out.push(language_alias(language.to_ascii_lowercase()));
    }
    let (parts, replacement) = apply_armenian_variant_alias(&out, &parts);
    if let Some(value) = replacement {
        out[0] = value;
    }
    let parts = parts.to_vec();
    validate_transformed_extensions(&parts)?;
    Ok(canonicalize_subtags(parts, out, script_done)?.join("-"))
}

fn apply_armenian_variant_alias<'a>(
    out: &[String],
    parts: &'a [&'a str],
) -> (&'a [&'a str], Option<String>) {
    if out.first().map(String::as_str) != Some("hy") {
        return (parts, None);
    }
    match parts.first().copied() {
        Some("arevela") => (&parts[1..], None),
        Some("arevmda") => (&parts[1..], Some("hyw".to_string())),
        _ => (parts, None),
    }
}

fn validate_transformed_extensions(parts: &[&str]) -> Result<(), VmError> {
    for (index, part) in parts.iter().enumerate() {
        if part.eq_ignore_ascii_case("x") {
            break;
        }
        if part.eq_ignore_ascii_case("t") {
            let end = parts[index + 1..]
                .iter()
                .position(|part| part.len() == 1)
                .map_or(parts.len(), |offset| index + 1 + offset);
            if end < parts.len() && end + 1 == parts.len() {
                return Err(runtime_error("RangeError: invalid language tag"));
            }
            validate_transformed_fields(&parts[index + 1..end])?;
        }
    }
    Ok(())
}

fn validate_transformed_fields(parts: &[&str]) -> Result<(), VmError> {
    if parts.is_empty() {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    let mut index = if is_transformed_language(parts[0]) {
        transformed_language_length(parts)?
    } else {
        0
    };
    validate_transformed_variants(&parts[..index])?;
    while index < parts.len() {
        let key = parts[index];
        if key.len() != 2 || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        index += 1;
        let start = index;
        while index < parts.len() && parts[index].len() != 2 {
            if !(3..=8).contains(&parts[index].len())
                || !parts[index].chars().all(|c| c.is_ascii_alphanumeric())
            {
                return Err(runtime_error("RangeError: invalid language tag"));
            }
            index += 1;
        }
        if start == index {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
    }
    Ok(())
}

fn validate_transformed_variants(parts: &[&str]) -> Result<(), VmError> {
    let mut seen = std::collections::HashSet::new();
    for part in parts {
        let variant = (5..=8).contains(&part.len())
            || (part.len() == 4 && part.as_bytes()[0].is_ascii_digit());
        if variant && !seen.insert(part.to_ascii_lowercase()) {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
    }
    Ok(())
}

fn is_transformed_language(part: &str) -> bool {
    (2..=3).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphabetic())
        || (5..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
}

fn transformed_language_length(parts: &[&str]) -> Result<usize, VmError> {
    let mut index = 1;
    if parts
        .get(index)
        .is_some_and(|part| part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()))
    {
        index += 1;
    }
    if parts.get(index).is_some_and(|part| {
        (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
            || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
    }) {
        index += 1;
    }
    while parts.get(index).is_some_and(|part| {
        (5..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
            || part.len() == 4
                && part.chars().next().is_some_and(|c| c.is_ascii_digit())
                && part.chars().skip(1).all(|c| c.is_ascii_alphanumeric())
    }) {
        index += 1;
    }
    Ok(index)
}

fn canonicalize_unicode_aliases(parts: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        result.push(parts[index].to_ascii_lowercase());
        if parts[index].eq_ignore_ascii_case("u") {
            index += 1;
            append_unicode_extension(parts, &mut index, &mut result);
        } else {
            index += 1;
        }
    }
    result
}

fn append_unicode_extension(parts: &[&str], index: &mut usize, result: &mut Vec<String>) {
    while *index < parts.len() && parts[*index].len() != 1 {
        let key = parts[*index].to_ascii_lowercase();
        result.push(key.clone());
        *index += 1;
        if key.len() != 2 {
            continue;
        }
        let start = *index;
        while *index < parts.len() && parts[*index].len() != 2 && parts[*index].len() != 1 {
            *index += 1;
        }
        let values = &parts[start..*index];
        if is_true_alias(&key, values) {
            continue;
        }
        if let Some(alias) = unicode_alias(&key, values) {
            result.push(alias.to_string());
        } else {
            result.extend(values.iter().map(|value| value.to_ascii_lowercase()));
        }
    }
}

fn is_true_alias(key: &str, values: &[&str]) -> bool {
    matches!(key, "kb" | "kc" | "kh" | "kk" | "kn") && values == ["yes"]
}

fn unicode_alias(key: &str, values: &[&str]) -> Option<&'static str> {
    match (key, values) {
        ("ca", ["ethiopic", "amete", "alem"]) => Some("ethioaa"),
        ("ca", ["islamicc"]) => Some("islamic-civil"),
        ("ks", ["primary"]) => Some("level1"),
        ("ks", ["secondary"]) => Some("level2"),
        ("ks", ["tertiary"]) => Some("level3"),
        ("ks", ["quaternary" | "quarternary"]) => Some("level4"),
        ("ks", ["identical"]) => Some("identic"),
        ("ms", ["imperial"]) => Some("uksystem"),
        ("rg", ["no23"]) | ("sd", ["no23"]) => Some("no50"),
        ("rg", ["cn11"]) | ("sd", ["cn11"]) => Some("cnbj"),
        ("rg", ["cz10a"]) | ("sd", ["cz10a"]) => Some("cz110"),
        ("rg", ["fra"]) | ("sd", ["fra"]) => Some("frges"),
        ("rg", ["frg"]) | ("sd", ["frg"]) => Some("frges"),
        ("rg", ["lud"]) | ("sd", ["lud"]) => Some("lucl"),
        ("tz", ["cnckg"]) => Some("cnsha"),
        ("tz", ["eire"]) => Some("iedub"),
        ("tz", ["est"]) => Some("papty"),
        ("tz", ["gmt0"]) => Some("gmt"),
        ("tz", ["uct" | "zulu"]) => Some("utc"),
        _ => None,
    }
}

include!("locale_canonicalization_subtags.rs");
