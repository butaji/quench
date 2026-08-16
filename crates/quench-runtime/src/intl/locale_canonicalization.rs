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
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Array(values) => values
            .iter()
            .map(to_string_value)
            .collect::<Vec<_>>()
            .join(","),
        _ => "[object Object]".to_string(),
    }
}

/// Canonicalize a single BCP-47 language tag.
pub(crate) fn canonicalize(tag: &str) -> Result<String, VmError> {
    let tag = tag.trim();
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
    let mut parts: Vec<&str> = parts.collect();
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
    apply_armenian_variant_alias(&mut out, &mut parts);
    validate_transformed_extensions(&parts)?;
    Ok(canonicalize_subtags(parts, out, script_done)?.join("-"))
}

fn apply_armenian_variant_alias(out: &mut Vec<String>, parts: &mut Vec<&str>) {
    if out.first().map(String::as_str) != Some("hy") {
        return;
    }
    match parts.first().copied() {
        Some("arevela") => {
            let _ = parts.remove(0);
        }
        Some("arevmda") => {
            out[0] = "hyw".to_string();
            let _ = parts.remove(0);
        }
        _ => {}
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

fn canonicalize_subtags(
    parts: Vec<&str>,
    mut out: Vec<String>,
    mut script_done: bool,
) -> Result<Vec<String>, VmError> {
    validate_unicode_extension_keys(&parts)?;
    let aliased = canonicalize_unicode_aliases(&parts);
    let variant_aliased = canonicalize_variant_aliases(&aliased);
    let parts: Vec<&str> = variant_aliased.iter().map(String::as_str).collect();
    let mut region_done = false;
    let mut variant_done = false;
    let mut extension = false;
    for part in parts {
        if part.is_empty() {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        if extension {
            out.push(part.to_ascii_lowercase());
            continue;
        }
        if part.len() == 1 {
            out.push(part.to_ascii_lowercase());
            extension = true;
            continue;
        }
        if region_done && is_region_shape(part) {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        match classify_subtag(part, script_done, region_done, variant_done) {
            Subtag::Script => {
                out.push(titlecase_script(part));
                script_done = true;
            }
            Subtag::Region => {
                out.push(canonical_region(part, &out));
                region_done = true;
            }
            Subtag::Variant => {
                out.push(part.to_ascii_lowercase());
                variant_done = true;
            }
            Subtag::Extension => out.push(part.to_ascii_lowercase()),
        }
    }
    Ok(out)
}

fn is_region_shape(part: &str) -> bool {
    let alphabetic = part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic());
    let numeric = part.len() == 3 && part.chars().all(|c| c.is_ascii_digit());
    alphabetic || numeric
}

pub(crate) fn canonical_region(part: &str, emitted: &[String]) -> String {
    let region = part.to_ascii_uppercase();
    match region.as_str() {
        "CS" => "RS".to_string(),
        "NT" => "SA".to_string(),
        "554" => "NZ".to_string(),
        "SU" | "810"
            if emitted.first().is_some_and(|language| language == "hy")
                || emitted.iter().any(|subtag| subtag == "Armn") =>
        {
            "AM".to_string()
        }
        "SU" | "810" => "RU".to_string(),
        _ => region,
    }
}

fn validate_unicode_extension_keys(parts: &[&str]) -> Result<(), VmError> {
    for (index, part) in parts.iter().enumerate() {
        if !part.eq_ignore_ascii_case("u") {
            continue;
        }
        for value in &parts[index + 1..] {
            if value.len() == 1 {
                break;
            }
            if value.len() == 2
                && !value
                    .chars()
                    .nth(1)
                    .is_some_and(|character| character.is_ascii_alphabetic())
            {
                return Err(runtime_error("RangeError: invalid language tag"));
            }
        }
    }
    Ok(())
}

fn canonicalize_variant_aliases(parts: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut index = 0;
    let mut extension = false;
    while index < parts.len() {
        if parts[index].len() == 1 {
            extension = true;
        }
        if !extension
            && parts[index].eq_ignore_ascii_case("hepburn")
            && parts
                .get(index + 1)
                .is_some_and(|part| part.eq_ignore_ascii_case("heploc"))
        {
            result.push("alalc97".to_string());
            index += 2;
        } else {
            result.push(parts[index].to_ascii_lowercase());
            index += 1;
        }
    }
    result
}

fn titlecase_script(part: &str) -> String {
    let mut chars = part.chars();
    let first = chars.next().map_or(String::new(), |value| {
        value.to_ascii_uppercase().to_string()
    });
    format!("{first}{}", chars.as_str().to_ascii_lowercase())
}

enum Subtag {
    Script,
    Region,
    Variant,
    Extension,
}

fn classify_subtag(part: &str, script_done: bool, region_done: bool, variant_done: bool) -> Subtag {
    let all_alpha = part.chars().all(|c| c.is_ascii_alphabetic());
    let all_digit = part.chars().all(|c| c.is_ascii_digit());
    if !script_done && part.len() == 4 && all_alpha {
        Subtag::Script
    } else if !region_done && ((part.len() == 2 && all_alpha) || (part.len() == 3 && all_digit)) {
        Subtag::Region
    } else if !variant_done
        && ((part.len() >= 4 && all_alpha)
            || (part.len() >= 5
                && part.chars().next().is_some_and(|c| c.is_ascii_digit())
                && part[1..].chars().all(|c| c.is_ascii_alphanumeric())))
    {
        Subtag::Variant
    } else {
        Subtag::Extension
    }
}

fn language_alias(language: String) -> String {
    match language.as_str() {
        "aar" => "aa".to_string(),
        "ces" => "cs".to_string(),
        "heb" => "he".to_string(),
        "iw" => "he".to_string(),
        "in" => "id".to_string(),
        "ji" => "yi".to_string(),
        "tl" => "fil".to_string(),
        "mo" => "ro".to_string(),
        other => other.to_string(),
    }
}

