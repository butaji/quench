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
        let (mut text, significant_selected, scientific, magnitude, scaled, compact_unscaled_de) =
            self.prepare_number(number);
        if scientific.is_none()
            && self.use_grouping
            && (!self.grouping_min2 || scaled.abs() >= 10_000.0)
            && (self.notation != "compact" || (compact_unscaled_de && scaled.abs() >= 10_000.0))
        {
            text = group_integer_locale(&text, &self.locale);
        } else if self.notation == "compact" && self.locale.starts_with("de") {
            text = text.replace('.', ",");
        }
        if let Some((_, exponent)) = scientific.filter(|(value, _)| value.is_finite()) {
            if self.locale.starts_with("de") {
                text = text.replace('.', ",");
            }
            let exponent = format!("E{exponent}");
            text.push_str(&exponent);
        }
        text = apply_minimum_integer(&text, self.minimum_integer_digits);
        if self.minimum_fraction_digits > 0 && !significant_selected {
            text = pad_locale_fraction(&text, self.minimum_fraction_digits, &self.locale);
        }
        text = self.apply_sign(text, number);
        match self.style.as_str() {
            "percent" => text.push('%'),
            "currency" => {
                text = format_currency(
                    &text,
                    self.currency.as_deref(),
                    &self.currency_display,
                    &self.locale,
                    &self.currency_sign,
                )
            }
            "unit" => {
                text = format_localized_unit(
                    &text,
                    self.unit.as_deref(),
                    &self.unit_display,
                    &self.locale,
                )
            }
            _ => {}
        }
        if magnitude > 0 {
            text.push_str(compact_suffix(
                magnitude,
                &self.locale,
                &self.compact_display,
            ));
        }
        text
    }

    fn apply_sign(&self, mut text: String, number: f64) -> String {
        let negative = text.starts_with('-');
        if number.is_nan() && self.locale.starts_with("zh") {
            text = "非數值".to_string();
        }
        let zero = number == 0.0;
        let rounded_zero = text
            .trim_start_matches('-')
            .chars()
            .all(|character| matches!(character, '0' | '.' | ','));
        let hide_negative = self.sign_display == "never"
            || (self.sign_display == "auto"
                && zero
                && self.style == "currency"
                && self.currency_sign != "accounting")
            || (self.sign_display == "exceptZero" && rounded_zero)
            || (self.sign_display == "negative" && rounded_zero);
        if hide_negative && negative {
            text.remove(0);
        } else if !negative
            && (!number.is_nan() || self.sign_display == "always")
            && (self.sign_display == "always"
                || (self.sign_display == "exceptZero" && !rounded_zero))
        {
            text.insert(0, '+');
        }
        text
    }

    fn prepare_number(&self, number: f64) -> (String, bool, Option<(f64, i32)>, i32, f64, bool) {
        let scaled = if self.style == "percent" {
            number * 100.0
        } else {
            number
        };
        let scientific = match self.notation.as_str() {
            "scientific" => Some(scientific_parts(scaled, false)),
            "engineering" => Some(scientific_parts(scaled, true)),
            _ => None,
        };
        let magnitude = compact_magnitude(self, scaled);
        let value = scientific.map_or(scaled, |(coefficient, _)| coefficient);
        let compact_unscaled_de = self.notation == "compact"
            && self.locale.starts_with("de")
            && magnitude == 0
            && scaled.abs() >= 1_000.0;
        let fraction_digits = if self.notation == "compact" && !compact_unscaled_de {
            compact_fraction_digits(value)
        } else {
            self.maximum_fraction_digits
        };
        let fraction_text = format_number_rounded(value, fraction_digits, self.rounding_increment);
        let (text, selected) = self.select_precision(value, fraction_text);
        (
            text,
            selected,
            scientific,
            magnitude,
            scaled,
            compact_unscaled_de,
        )
    }

    fn select_precision(&self, value: f64, fraction_text: String) -> (String, bool) {
        let Some(maximum) = self.maximum_significant_digits else {
            return (fraction_text, false);
        };
        let significant_text = format_significant(
            value,
            self.minimum_significant_digits.unwrap_or(1),
            maximum,
            &self.rounding_mode,
        );
        match self.rounding_priority.as_str() {
            "morePrecision"
                if decimal_places(&fraction_text) > decimal_places(&significant_text) =>
            {
                (fraction_text, false)
            }
            "lessPrecision"
                if decimal_places(&fraction_text) < decimal_places(&significant_text) =>
            {
                (fraction_text, false)
            }
            _ => (significant_text, true),
        }
    }

    fn parts(&self, number: f64) -> Vec<Value> {
        let formatted = self.format_number(number);
        if self.style == "currency" {
            return currency_parts(
                &formatted,
                self.currency.as_deref(),
                &self.currency_display,
                &self.locale,
            );
        }
        if self.style == "unit" {
            if self.locale.starts_with("ja")
                && self.unit == Some("kilometer-per-hour".to_string())
                && self.unit_display == "long"
            {
                return japanese_speed_parts(&formatted);
            }
            return unit_parts(
                &formatted,
                self.unit.as_deref(),
                &self.unit_display,
                &self.locale,
            );
        }
        if self.style == "percent" {
            let mut parts = numeric_parts(
                formatted.strip_suffix('%').unwrap_or(&formatted),
                &self.locale,
            );
            parts.push(crate::intl::number_format::percent_part());
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
        make_object(vec![
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
                "compactDisplay".to_string(),
                Value::String(self.compact_display.clone()),
            ),
            (
                "signDisplay".to_string(),
                Value::String(self.sign_display.clone()),
            ),
            (
                "roundingMode".to_string(),
                Value::String(self.rounding_mode.clone()),
            ),
        ])
    }
}


