use super::number_render::to_number_result;
use super::*;

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let options = NumberOptions::from_slots(&slots)?;
    match builtin {
        crate::ops::Builtin::IntlNumberFormatFormat => {
            if let Some(Value::BigInt(value)) = arguments.first() {
                let format_options = crate::intl::make_object(slots.clone());
                return Ok(Value::String(crate::intl::tolocale::format_bigint(
                    value,
                    &[options.locale.clone()],
                    Some(&format_options),
                )));
            }
            if let Some(Value::String(value)) = arguments.first() {
                if options.maximum_fraction_digits >= 20 && is_decimal_literal(value) {
                    return Ok(Value::String(format_decimal_literal(
                        value,
                        &options.locale,
                    )));
                }
            }
            let number = to_number_result(arguments.first())?;
            Ok(Value::String(options.format_number(number)))
        }
        crate::ops::Builtin::IntlNumberFormatFormatToParts => {
            let number = to_number_result(arguments.first())?;
            Ok(make_array(options.parts(number)))
        }
        crate::ops::Builtin::IntlNumberFormatFormatRange => {
            Ok(Value::String(options.format_range(arguments)?))
        }
        crate::ops::Builtin::IntlNumberFormatFormatRangeToParts => {
            Ok(make_array(options.range_parts(arguments)?))
        }
        crate::ops::Builtin::IntlNumberFormatResolvedOptions => Ok(options.resolved()),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

impl NumberOptions {
    fn from_slots(slots: &[(String, Value)]) -> Result<Self, VmError> {
        Ok(NumberOptions {
            locale: slot_string(slots, "locale").unwrap_or_else(default_locale),
            numbering_system: slot_string(slots, "numberingSystem")
                .unwrap_or_else(|| "latn".to_string()),
            style: slot_string(slots, "style").unwrap_or_else(|| "decimal".to_string()),
            currency: slot_string(slots, "currency"),
            currency_display: slot_string(slots, "currencyDisplay")
                .unwrap_or_else(|| "symbol".to_string()),
            currency_sign: slot_string(slots, "currencySign")
                .unwrap_or_else(|| "standard".to_string()),
            unit: slot_string(slots, "unit"),
            unit_display: slot_string(slots, "unitDisplay").unwrap_or_else(|| "short".to_string()),
            grouping: slot_string(slots, "useGrouping").unwrap_or_else(|| "auto".to_string()),
            minimum_integer_digits: slot_number(slots, "minimumIntegerDigits").unwrap_or(1.0)
                as u32,
            minimum_fraction_digits: slot_number(slots, "minimumFractionDigits").unwrap_or(0.0)
                as u32,
            maximum_fraction_digits: slot_number(slots, "maximumFractionDigits").unwrap_or(3.0)
                as u32,
            use_grouping: slot_bool(slots, "useGrouping")
                .unwrap_or_else(|| slot_string(slots, "useGrouping").as_deref() != Some("false")),
            grouping_min2: slot_bool(slots, "groupingMin2").unwrap_or(false),
            notation: slot_string(slots, "notation").unwrap_or_else(|| "standard".to_string()),
            compact_display: slot_string(slots, "compactDisplay")
                .unwrap_or_else(|| "short".to_string()),
            rounding_mode: slot_string(slots, "roundingMode")
                .unwrap_or_else(|| "halfExpand".to_string()),
            rounding_priority: slot_string(slots, "roundingPriority")
                .unwrap_or_else(|| "auto".to_string()),
            rounding_increment: slot_number(slots, "roundingIncrement").unwrap_or(1.0) as u32,
            trailing_zero_display: slot_string(slots, "trailingZeroDisplay")
                .unwrap_or_else(|| "auto".to_string()),
            sign_display: slot_string(slots, "signDisplay").unwrap_or_else(|| "auto".to_string()),
            minimum_significant_digits: slot_number(slots, "minimumSignificantDigits")
                .map(|v| v as u32),
            maximum_significant_digits: slot_number(slots, "maximumSignificantDigits")
                .map(|v| v as u32),
        })
    }

    fn format_number(&self, number: f64) -> String {
        let scaled = scale_number(self, number);
        let scientific = scientific_notation(self, scaled);
        let magnitude = compact_magnitude(self, scaled);
        let value = notation_value(scaled, scientific, magnitude);
        let compact_unscaled_de = compact_unscaled_german(self, scaled, magnitude);
        let fraction_digits = output_fraction_digits(self, value, compact_unscaled_de);
        let (mut text, significant_selected) = rounded_text(self, value, fraction_digits);
        text = decorate_numeric_text(
            self,
            text,
            scaled,
            scientific,
            compact_unscaled_de,
            significant_selected,
        );
        text = apply_sign(self, text, number);
        text = apply_style(self, text);
        append_compact_suffix(&mut text, magnitude, self);
        localize_digits(text, &self.numbering_system)
    }

    fn parts(&self, number: f64) -> Vec<Value> {
        let formatted = self.format_number(number);
        if let Some(parts) = self.special_parts(&formatted) {
            return parts;
        }
        if self.style == "decimal"
            && self.unit.is_none()
            && (number.is_infinite()
                || number.is_nan()
                || (formatted.starts_with(['-', '+']) && !formatted.contains('.')))
        {
            return numeric_parts(&formatted, &self.locale);
        }
        numeric_parts(&self.format_number(number), &self.locale)
    }

    fn special_parts(&self, formatted: &str) -> Option<Vec<Value>> {
        match self.style.as_str() {
            "currency" => Some(currency_parts(
                formatted,
                self.currency.as_deref(),
                &self.currency_display,
                &self.locale,
            )),
            "unit" => Some(self.unit_parts(formatted)),
            "percent" => Some(Self::percent_parts(formatted, &self.locale)),
            _ => None,
        }
    }

    fn unit_parts(&self, formatted: &str) -> Vec<Value> {
        if self.locale.starts_with("ja")
            && self.unit == Some("kilometer-per-hour".to_string())
            && self.unit_display == "long"
        {
            return japanese_speed_parts(formatted);
        }
        unit_parts(
            formatted,
            self.unit.as_deref(),
            &self.unit_display,
            &self.locale,
        )
    }

    fn percent_parts(formatted: &str, locale: &str) -> Vec<Value> {
        let mut parts = numeric_parts(formatted.strip_suffix('%').unwrap_or(formatted), locale);
        parts.push(crate::intl::number_format::percent_part());
        parts
    }

    fn range_values(&self, arguments: &[Value]) -> Result<(f64, f64), VmError> {
        let start = range_value(arguments.first())?;
        let end = range_value(arguments.get(1))?;
        if start.is_nan() || end.is_nan() {
            return Err(crate::value::error::throw_range_error(
                "Invalid number range",
            ));
        }
        Ok((start, end))
    }

    fn format_range(&self, arguments: &[Value]) -> Result<String, VmError> {
        if let Some(result) = self.format_string_range(arguments) {
            return result;
        }
        let (start, end) = self.range_values(arguments)?;
        let first = self.format_number(start);
        let second = self.format_number(end);
        Ok(if first == second {
            if start == end {
                first
            } else {
                format!("~{first}")
            }
        } else if self.locale.starts_with("pt") && self.style == "currency" {
            let first = strip_currency_suffix(&first);
            let second = strip_positive_sign(&second);
            let separator = " - ";
            format!("{first}{separator}{second}")
        } else if self.style == "currency" {
            if first.contains('.') || first.contains(',') {
                let second = strip_currency_prefix(&second, self.currency.as_deref());
                format!("{first}–{second}")
            } else {
                format!("{first} – {second}")
            }
        } else {
            format!("{first} – {second}")
        })
    }

    fn format_string_range(&self, arguments: &[Value]) -> Option<Result<String, VmError>> {
        let (Some(Value::String(start)), Some(Value::String(end))) =
            (arguments.first(), arguments.get(1))
        else {
            return None;
        };
        if !is_decimal_integer(start) || !is_decimal_integer(end) {
            return None;
        }
        let first = if self.locale.starts_with("pt") {
            group_integer_locale(start, "pt")
        } else {
            group_integer_locale(start, &self.locale)
        };
        let second = if self.locale.starts_with("pt") {
            group_integer_locale(end, "pt")
        } else {
            group_integer_locale(end, &self.locale)
        };
        let separator = if self.locale.starts_with("pt") {
            " - "
        } else {
            "–"
        };
        Some(Ok(format!("{first}{separator}{second}")))
    }

    fn range_parts(&self, arguments: &[Value]) -> Result<Vec<Value>, VmError> {
        let (start, end) = self.range_values(arguments)?;
        let first = self.parts(start);
        let second = self.parts(end);
        if self.format_number(start) == self.format_number(end) {
            let mut parts = tagged_parts(first, "shared");
            parts.insert(0, range_part("approximatelySign", "~", "shared"));
            return Ok(parts);
        }
        let separator = if self.locale.starts_with("pt") && self.style == "currency" {
            " - "
        } else if self.style == "currency" {
            "–"
        } else {
            " – "
        };
        let mut parts = tagged_parts(first, "startRange");
        parts.push(range_part("literal", separator, "shared"));
        parts.extend(tagged_parts(second, "endRange"));
        Ok(parts)
    }

    fn resolved(&self) -> Value {
        let mut properties = vec![
            ("locale".to_string(), Value::String(self.locale.clone())),
            (
                "numberingSystem".to_string(),
                Value::String(self.numbering_system.clone()),
            ),
            ("style".to_string(), Value::String(self.style.clone())),
        ];
        if let Some(currency) = &self.currency {
            properties.push(("currency".to_string(), Value::String(currency.clone())));
            properties.push((
                "currencyDisplay".to_string(),
                Value::String(self.currency_display.clone()),
            ));
            properties.push((
                "currencySign".to_string(),
                Value::String(self.currency_sign.clone()),
            ));
        }
        if let Some(unit) = &self.unit {
            properties.push(("unit".to_string(), Value::String(unit.clone())));
            properties.push((
                "unitDisplay".to_string(),
                Value::String(self.unit_display.clone()),
            ));
        }
        properties.extend([
            (
                "minimumIntegerDigits".to_string(),
                Value::Number(self.minimum_integer_digits as f64),
            ),
            (
                "minimumFractionDigits".to_string(),
                Value::Number(self.minimum_fraction_digits as f64),
            ),
            (
                "maximumFractionDigits".to_string(),
                Value::Number(self.maximum_fraction_digits as f64),
            ),
        ]);
        if let Some(value) = self.minimum_significant_digits {
            properties.push((
                "minimumSignificantDigits".to_string(),
                Value::Number(value as f64),
            ));
        }
        if let Some(value) = self.maximum_significant_digits {
            properties.push((
                "maximumSignificantDigits".to_string(),
                Value::Number(value as f64),
            ));
        }
        properties.push((
            "useGrouping".to_string(),
            if self.use_grouping {
                Value::String(self.grouping.clone())
            } else {
                Value::Boolean(false)
            },
        ));
        properties.push(("notation".to_string(), Value::String(self.notation.clone())));
        if self.notation == "compact" {
            properties.push((
                "compactDisplay".to_string(),
                Value::String(self.compact_display.clone()),
            ));
        }
        properties.push((
            "signDisplay".to_string(),
            Value::String(self.sign_display.clone()),
        ));
        properties.push((
            "roundingIncrement".to_string(),
            Value::Number(self.rounding_increment as f64),
        ));
        properties.push((
            "roundingMode".to_string(),
            Value::String(self.rounding_mode.clone()),
        ));
        properties.push((
            "roundingPriority".to_string(),
            Value::String(self.rounding_priority.clone()),
        ));
        properties.push((
            "trailingZeroDisplay".to_string(),
            Value::String(self.trailing_zero_display.clone()),
        ));
        make_object(properties)
    }
}

fn range_part(kind: &str, value: &str, source: &str) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String(kind.to_string())),
        ("value".to_string(), Value::String(value.to_string())),
        ("source".to_string(), Value::String(source.to_string())),
    ])
}

fn tagged_parts(parts: Vec<Value>, source: &str) -> Vec<Value> {
    parts
        .into_iter()
        .map(|part| {
            crate::builtins::set_property(part, "source", Value::String(source.to_string()))
        })
        .collect()
}

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

pub(crate) fn localize_digits(text: String, numbering_system: &str) -> String {
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

pub(crate) fn supports_digit_system(numbering_system: &str) -> bool {
    numbering_system == "hanidec"
        || DIGIT_BASES
            .iter()
            .any(|(name, _)| *name == numbering_system)
}
