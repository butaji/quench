pub(crate) fn format_bigint(value: &str, locales: &[String], options: Option<&Value>) -> String {
    quench_intl::format_bigint(
        value,
        &quench_intl::BigIntFormatOptions {
            locale: locales.first().map_or("en-US", String::as_str),
            style: option_string(options.unwrap_or(&Value::Undefined), "style")
                .as_deref()
                .unwrap_or("decimal"),
            minimum_fraction_digits: option_string(
                options.unwrap_or(&Value::Undefined),
                "minimumFractionDigits",
            )
            .and_then(|value| value.parse().ok())
            .unwrap_or_default(),
            maximum_significant_digits: option_string(
                options.unwrap_or(&Value::Undefined),
                "maximumSignificantDigits",
            )
            .and_then(|value| value.parse().ok()),
        },
    )
}

fn option_string(value: &Value, key: &str) -> Option<String> {
    let Value::Object(properties) = value else {
        return None;
    };
    properties
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| super::to_string_value(&value))
}
