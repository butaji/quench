fn locale_slot(locale: &Locale) -> Value {
    let mut properties = vec![
        (
            "language".to_string(),
            Value::String(locale.language.clone()),
        ),
        ("base".to_string(), Value::String(locale.base_name())),
    ];
    push_optional_property(&mut properties, "script", locale.script.as_ref());
    push_optional_property(&mut properties, "region", locale.region.as_ref());
    if !locale.variants.is_empty() {
        properties.push((
            "variants".to_string(),
            Value::String(locale.variants.join("-")),
        ));
    }
    push_optional_property(&mut properties, "calendar", locale.calendar.as_ref());
    push_optional_property(&mut properties, "collation", locale.collation.as_ref());
    push_optional_property(&mut properties, "hourCycle", locale.hour_cycle.as_ref());
    push_optional_property(&mut properties, "caseFirst", locale.case_first.as_ref());
    push_optional_property(
        &mut properties,
        "numberingSystem",
        locale.numbering_system.as_ref(),
    );
    push_optional_property(
        &mut properties,
        "firstDayOfWeek",
        locale.first_day_of_week.as_ref(),
    );
    properties.push(("numeric".to_string(), Value::Boolean(locale.numeric)));
    properties.push(("full".to_string(), Value::String(slot_full(locale))));
    make_object(properties)
}

fn push_optional_property(
    properties: &mut Vec<(String, Value)>,
    name: &str,
    value: Option<&String>,
) {
    if let Some(value) = value {
        properties.push((name.to_string(), Value::String(value.clone())));
    }
}

fn slot_full(locale: &Locale) -> String {
    let mut full = locale.base_name();
    let mut extensions = locale.other_extensions.clone();
    if !locale.unicode_extensions.is_empty() {
        extensions.push(OtherExtension {
            singleton: "u".to_string(),
            subtags: unicode_subtags(locale),
        });
    } else if !slot_keys(locale).is_empty() {
        extensions.push(OtherExtension {
            singleton: "u".to_string(),
            subtags: slot_keys(locale)
                .into_iter()
                .flat_map(|(key, value)| {
                    let mut subtags = vec![key.to_string()];
                    if value != "true" {
                        subtags.push(value);
                    }
                    subtags
                })
                .collect(),
        });
    }
    extensions.sort_by(|left, right| {
        (left.singleton == "x")
            .cmp(&(right.singleton == "x"))
            .then_with(|| left.singleton.cmp(&right.singleton))
    });
    for extension in extensions {
        full.push('-');
        full.push_str(&extension.singleton);
        for subtag in &extension.subtags {
            full.push('-');
            full.push_str(subtag);
        }
    }
    full
}

fn unicode_subtags(locale: &Locale) -> Vec<String> {
    let mut subtags = Vec::new();
    for extension in &locale.unicode_extensions {
        subtags.extend(extension.attributes.iter().cloned());
        if !extension.key.is_empty() {
            subtags.push(extension.key.clone());
        }
        subtags.extend(
            extension
                .types
                .iter()
                .filter(|value| !value.is_empty() && value.as_str() != "true")
                .cloned(),
        );
    }
    subtags
}

fn slot_keys(locale: &Locale) -> Vec<(&'static str, String)> {
    let mut keys: Vec<(&'static str, String)> = Vec::new();
    if let Some(calendar) = &locale.calendar {
        keys.push(("ca", calendar.clone()));
    }
    if let Some(collation) = &locale.collation {
        keys.push(("co", collation.clone()));
    }
    if let Some(case_first) = &locale.case_first {
        keys.push(("kf", case_first.clone()));
    }
    if let Some(hour_cycle) = &locale.hour_cycle {
        keys.push(("hc", hour_cycle.clone()));
    }
    if let Some(numbering) = &locale.numbering_system {
        keys.push(("nu", numbering.clone()));
    }
    if let Some(first_day) = &locale.first_day_of_week {
        keys.push(("fw", first_day.clone()));
    }
    if locale.numeric_explicit {
        keys.push((
            "kn",
            if locale.numeric {
                "true".to_string()
            } else {
                "false".to_string()
            },
        ));
    }
    keys
}

fn locale_slot_value(slots: &[(String, Value)], key: &str) -> Value {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .map_or(Value::Undefined, |(_, value)| value.clone())
}
