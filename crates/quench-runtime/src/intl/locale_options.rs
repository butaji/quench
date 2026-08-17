fn apply_options(mut locale: Locale, options: Option<&Value>) -> Result<Locale, VmError> {
    if matches!(options, Some(Value::Null)) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null or undefined to object",
        ));
    }
    let Some(options) = options.filter(|value| crate::value::is_object(value)) else {
        return Ok(locale);
    };
    for key in [
        "language",
        "script",
        "region",
        "variants",
        "calendar",
        "collation",
        "hourCycle",
        "firstDayOfWeek",
        "caseFirst",
        "numeric",
        "numberingSystem",
    ] {
        let value = crate::execute::get_property_result(options, key)?;
        if matches!(value, Value::Undefined) {
            continue;
        }
        let text = crate::conversion::to_string(&value)?;
        match key {
            "calendar" => {
                let value = option_value(&text, "calendar")?;
                locale.calendar = Some(calendar_alias(&calendar_option(&value)?));
            }
            "collation" => locale.collation = Some(keyword_option(&text, "collation")?),
            "caseFirst" => locale.case_first = Some(normalize_case_first(&text)?),
            "hourCycle" => locale.hour_cycle = Some(normalize_hour_cycle(&text)?),
            "numberingSystem" => {
                let value = option_value(&text, "numberingSystem")?;
                if !value.split('-').all(|part| {
                    (3..=8).contains(&part.len())
                        && part
                            .chars()
                            .all(|character| character.is_ascii_alphanumeric())
                }) {
                    return Err(runtime_error("RangeError: invalid numberingSystem"));
                }
                locale.numbering_system = Some(value);
            }
            "numeric" => {
                locale.numeric = normalize_numeric(&value, &text)?;
                locale.numeric_explicit = true;
            }
            "firstDayOfWeek" => {
                let value = option_value(&text, "firstDayOfWeek")?;
                if value.len() < 3
                    && !matches!(
                        value.as_str(),
                        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7"
                    )
                {
                    return Err(runtime_error("RangeError: invalid firstDayOfWeek"));
                }
                if !matches!(
                    value.as_str(),
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7"
                ) && value.split('-').any(|part| {
                    !(3..=8).contains(&part.len())
                        || !part
                            .chars()
                            .all(|character| character.is_ascii_alphanumeric())
                }) {
                    return Err(runtime_error("RangeError: invalid firstDayOfWeek"));
                }
                locale.first_day_of_week = Some(normalize_first_day(&value));
            }
            "language" if text.contains('-') => {
                return Err(runtime_error("RangeError: invalid language"));
            }
            "language" => {
                let language = option_value(&text, key)?;
                if !valid_language_subtag(&language) {
                    return Err(runtime_error("RangeError: invalid language"));
                }
                locale.language = super::language_alias(language.to_ascii_lowercase());
            }
            "script" => {
                let script = option_value(&text, key)?;
                if script.len() != 4 || !script.chars().all(|c| c.is_ascii_alphabetic()) {
                    return Err(runtime_error("RangeError: invalid script"));
                }
                locale.script = Some(super::titlecase_script(&script));
            }
            "region" => {
                validate_region(&text)?;
                locale.region = Some(super::canonical_region(&text, &[locale.language.clone()]));
            }
            "variants" => {
                let value = option_value(&text, key)?;
                locale.variants = variants_option(&value)?;
            }
            _ => {}
        }
    }
    if locale.language == "cel" && locale.variants == ["gaulish"] {
        locale.language = "xtg".to_string();
        locale.variants.clear();
    }
    sync_unicode_extensions(&mut locale);
    Ok(locale)
}

fn sync_unicode_extensions(locale: &mut Locale) {
    let known = [
        ("ca", locale.calendar.clone()),
        ("co", locale.collation.clone()),
        ("kf", locale.case_first.clone()),
        ("hc", locale.hour_cycle.clone()),
        ("nu", locale.numbering_system.clone()),
        ("fw", locale.first_day_of_week.clone()),
    ];
    for (key, value) in known {
        sync_unicode_key(&mut locale.unicode_extensions, key, value);
    }
    sync_unicode_key(
        &mut locale.unicode_extensions,
        "kn",
        locale
            .numeric_explicit
            .then(|| if locale.numeric { "true" } else { "false" }.to_string()),
    );
    sort_unicode_extensions(&mut locale.unicode_extensions);
}

fn sort_unicode_extensions(extensions: &mut Vec<UnicodeExtension>) {
    let attributes = extensions
        .iter_mut()
        .flat_map(|extension| std::mem::take(&mut extension.attributes))
        .collect::<Vec<_>>();
    extensions.sort_by(|left, right| left.key.cmp(&right.key));
    if let Some(first) = extensions.first_mut() {
        first.attributes = attributes;
    }
}

fn sync_unicode_key(extensions: &mut Vec<UnicodeExtension>, key: &str, value: Option<String>) {
    let position = extensions.iter().position(|extension| extension.key == key);
    match (position, value) {
        (Some(index), Some(value)) => extensions[index].types = vec![value],
        (Some(index), None) => {
            extensions.remove(index);
        }
        (None, Some(value)) => extensions.push(UnicodeExtension {
            attributes: Vec::new(),
            key: key.to_string(),
            types: vec![value],
        }),
        (None, None) => {}
    }
}

fn normalize_first_day(value: &str) -> String {
    match value {
        "0" | "sun" => "sun".to_string(),
        "1" | "mon" => "mon".to_string(),
        "2" | "tue" => "tue".to_string(),
        "3" | "wed" => "wed".to_string(),
        "4" | "thu" => "thu".to_string(),
        "5" | "fri" => "fri".to_string(),
        "6" | "sat" => "sat".to_string(),
        "7" => "sun".to_string(),
        other => other.to_string(),
    }
}

fn validate_region(value: &str) -> Result<(), VmError> {
    let valid = (value.len() == 2 && value.chars().all(|c| c.is_ascii_alphabetic()))
        || (value.len() == 3 && value.chars().all(|c| c.is_ascii_digit()));
    valid
        .then_some(())
        .ok_or_else(|| runtime_error("RangeError: invalid region"))
}

fn variants_option(value: &str) -> Result<Vec<String>, VmError> {
    let mut variants = value
        .split('-')
        .map(|variant| variant.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if variants.iter().any(|variant| {
        !((5..=8).contains(&variant.len()) && variant.chars().all(|c| c.is_ascii_alphanumeric())
            || variant.len() == 4 && variant.chars().next().is_some_and(|c| c.is_ascii_digit()))
    }) {
        return Err(runtime_error("RangeError: invalid variants"));
    }
    variants.sort();
    if variants.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(runtime_error("RangeError: invalid variants"));
    }
    Ok(variants)
}

fn option_value(value: &str, name: &str) -> Result<String, VmError> {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(runtime_error(&format!("RangeError: invalid {name}")));
    }
    Ok(value.to_string())
}

pub(crate) fn calendar_option(value: &str) -> Result<String, VmError> {
    keyword_option(value, "calendar")
}

fn keyword_option(value: &str, name: &str) -> Result<String, VmError> {
    if value.split('-').all(|part| {
        (3..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
    }) {
        Ok(value.to_string())
    } else {
        Err(runtime_error(&format!("RangeError: invalid {name}")))
    }
}

fn normalize_case_first(value: &str) -> Result<String, VmError> {
    match value {
        "upper" | "lower" | "false" => Ok(value.to_string()),
        _ => Err(runtime_error("RangeError: invalid caseFirst")),
    }
}

fn normalize_hour_cycle(value: &str) -> Result<String, VmError> {
    match value {
        "h11" | "h12" | "h23" | "h24" => Ok(value.to_string()),
        _ => Err(runtime_error("RangeError: invalid hourCycle")),
    }
}

fn normalize_numeric(value: &Value, text: &str) -> Result<bool, VmError> {
    let truthy = match value {
        Value::Undefined => return Ok(false),
        Value::Null => false,
        Value::Boolean(value) => *value,
        Value::Number(value) => *value != 0.0 && !value.is_nan(),
        _ => !text.is_empty(),
    };
    Ok(truthy)
}
