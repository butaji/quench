fn locale_slot(locale: &Locale) -> Value {
    let mut properties = vec![("language".to_string(), Value::String(locale.language.clone())),
        ("base".to_string(), Value::String(locale.base_name()))];
    if let Some(script) = &locale.script { properties.push(("script".to_string(), Value::String(script.clone()))); }
    if let Some(region) = &locale.region { properties.push(("region".to_string(), Value::String(region.clone()))); }
    if !locale.variants.is_empty() { properties.push(("variants".to_string(), Value::String(locale.variants.join("-")))); }
    if let Some(calendar) = &locale.calendar { properties.push(("calendar".to_string(), Value::String(calendar.clone()))); }
    if let Some(collation) = &locale.collation { properties.push(("collation".to_string(), Value::String(collation.clone()))); }
    if let Some(hour_cycle) = &locale.hour_cycle { properties.push(("hourCycle".to_string(), Value::String(hour_cycle.clone()))); }
    if let Some(case_first) = &locale.case_first { properties.push(("caseFirst".to_string(), Value::String(case_first.clone()))); }
    if let Some(numbering) = &locale.numbering_system { properties.push(("numberingSystem".to_string(), Value::String(numbering.clone()))); }
    if let Some(first_day) = &locale.first_day_of_week { properties.push(("firstDayOfWeek".to_string(), Value::String(first_day.clone()))); }
    properties.push(("numeric".to_string(), Value::Boolean(locale.numeric)));
    properties.push(("full".to_string(), Value::String(slot_full(locale))));
    make_object(properties)
}

fn slot_full(locale: &Locale) -> String {
    let mut full = locale.base_name();
    let mut first = true;
    for (key, item) in slot_keys(locale) {
        if first { full.push_str("-u"); first = false; }
        full.push('-'); full.push_str(key);
        if item != "true" { full.push('-'); full.push_str(&item); }
    }
    full
}

fn slot_keys(locale: &Locale) -> Vec<(&'static str, String)> {
    let mut keys = Vec::new();
    if let Some(value) = &locale.calendar { keys.push(("ca", value.clone())); }
    if let Some(value) = &locale.collation { keys.push(("co", value.clone())); }
    if let Some(value) = &locale.case_first { keys.push(("kf", value.clone())); }
    if let Some(value) = &locale.hour_cycle { keys.push(("hc", value.clone())); }
    if let Some(value) = &locale.numbering_system { keys.push(("nu", value.clone())); }
    if let Some(value) = &locale.first_day_of_week { keys.push(("fw", value.clone())); }
    if locale.numeric_explicit { keys.push(("kn", if locale.numeric { "true" } else { "false" }.to_string())); }
    keys
}

fn locale_slot_value(slots: &[(String, Value)], key: &str) -> Value {
    slots.iter().find(|(name, _)| name == key).map_or(Value::Undefined, |(_, value)| value.clone())
}
