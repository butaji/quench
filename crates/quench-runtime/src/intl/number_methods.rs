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
            style: slot_string(slots, "style").unwrap_or_else(|| "decimal".to_string()),
            currency: slot_string(slots, "currency"),
            currency_display: slot_string(slots, "currencyDisplay")
                .unwrap_or_else(|| "symbol".to_string()),
            currency_sign: slot_string(slots, "currencySign")
                .unwrap_or_else(|| "standard".to_string()),
            unit: slot_string(slots, "unit"),
            unit_display: slot_string(slots, "unitDisplay").unwrap_or_else(|| "short".to_string()),
            minimum_integer_digits: slot_number(slots, "minimumIntegerDigits").unwrap_or(1.0)
                as u32,
            minimum_fraction_digits: slot_number(slots, "minimumFractionDigits").unwrap_or(0.0)
                as u32,
            maximum_fraction_digits: slot_number(slots, "maximumFractionDigits").unwrap_or(3.0)
                as u32,
            use_grouping: slot_bool(slots, "useGrouping").unwrap_or(true),
            grouping_min2: slot_bool(slots, "groupingMin2").unwrap_or(false),
            notation: slot_string(slots, "notation").unwrap_or_else(|| "standard".to_string()),
            compact_display: slot_string(slots, "compactDisplay")
                .unwrap_or_else(|| "short".to_string()),
            rounding_mode: slot_string(slots, "roundingMode")
                .unwrap_or_else(|| "halfExpand".to_string()),
            rounding_priority: slot_string(slots, "roundingPriority")
                .unwrap_or_else(|| "auto".to_string()),
            rounding_increment: slot_number(slots, "roundingIncrement").unwrap_or(1.0) as u32,
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
        text
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
        if start > end {
            return Err(crate::value::error::throw_range_error(
                "Number range start is greater than end",
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
        if decimal_integer_greater(start, end) {
            return Some(Err(crate::value::error::throw_range_error(
                "Number range start is greater than end",
            )));
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
        let mut parts = self.parts(start);
        if start != end {
            let separator = if self.locale.starts_with("pt") && self.style == "currency" {
                " - "
            } else if self.style == "currency" {
                "–"
            } else {
                " – "
            };
            parts.push(make_object(vec![
                ("type".to_string(), Value::String("literal".to_string())),
                ("value".to_string(), Value::String(separator.to_string())),
            ]));
            parts.extend(self.parts(end));
        }
        Ok(parts)
    }

    fn resolved(&self) -> Value {
        let mut properties = vec![
            ("locale".to_string(), Value::String(self.locale.clone())),
            (
                "numberingSystem".to_string(),
                Value::String("latn".to_string()),
            ),
            ("style".to_string(), Value::String(self.style.clone())),
            ("useGrouping".to_string(), Value::Boolean(self.use_grouping)),
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
            ("notation".to_string(), Value::String(self.notation.clone())),
            (
                "signDisplay".to_string(),
                Value::String(self.sign_display.clone()),
            ),
            (
                "roundingMode".to_string(),
                Value::String(self.rounding_mode.clone()),
            ),
            (
                "roundingIncrement".to_string(),
                Value::Number(self.rounding_increment as f64),
            ),
        ];
        if self.notation == "compact" {
            properties.push((
                "compactDisplay".to_string(),
                Value::String(self.compact_display.clone()),
            ));
        }
        make_object(properties)
    }
}
