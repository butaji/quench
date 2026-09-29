use super::temporal_date::{self, IsoDate};
use super::*;

pub(super) const MAX_CALENDAR_DIFFERENCE_ROUNDING_INCREMENT: i128 = 100_000_000;
const MAX_PLAIN_DATE_TIME_ROUNDING_INCREMENT: f64 = 1_000_000_000.0;

const MONTHS_PER_YEAR: i128 = 12;
const UNITS: [&str; 10] = [
    "year",
    "month",
    "week",
    "day",
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];
const ROUNDING_MODES: [&str; 9] = [
    "ceil",
    "floor",
    "expand",
    "halfCeil",
    "halfFloor",
    "halfEven",
    "halfExpand",
    "halfTrunc",
    "trunc",
];

struct DifferenceOptions {
    largest: &'static str,
    smallest: &'static str,
    increment: i128,
    rounding_mode: String,
}

#[derive(Clone, Copy)]
pub(super) enum DifferenceDomain {
    PlainDateTime,
    ZonedDateTime,
}

impl<H: Host> Vm<H> {
    pub(super) fn temporal_plain_date_time_difference(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let constructor = self.temporal_plain_date_time_constructor(p)?;
        let other = self.temporal_plain_date_time_from(
            p,
            constructor,
            &[args.first().copied().unwrap_or(Value::UNDEFINED)],
        )?;
        let (left_date, left_time, left_calendar) = self.temporal_plain_date_time_slots(p, this)?;
        let (right_date, right_time, right_calendar) =
            self.temporal_plain_date_time_slots(p, other)?;
        if left_calendar != right_calendar {
            return Err(self.range_error(p, "Calendar mismatch".into()));
        }
        let options =
            self.difference_options(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        if !matches!(left_calendar.as_str(), "iso8601" | "gregory")
            && matches!(options.largest, "year" | "month")
            && left_time == right_time
            && options.smallest == "nanosecond"
            && options.increment == 1
            && options.rounding_mode == "trunc"
        {
            let largest_unit = if options.largest == "year" {
                quench_intl::CalendarDifferenceUnit::Years
            } else {
                quench_intl::CalendarDifferenceUnit::Months
            };
            let difference = quench_intl::calendar_date_difference(
                (left_date.year, left_date.month, left_date.day),
                (right_date.year, right_date.month, right_date.day),
                &left_calendar,
                largest_unit,
                if native == Native::TemporalPlainDateTimeSince {
                    quench_intl::CalendarDifferenceDirection::Since
                } else {
                    quench_intl::CalendarDifferenceDirection::Until
                },
            )
            .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime difference".into()))?;
            let mut fields = [0.0; 10];
            fields[super::temporal_date_arithmetic::DURATION_YEARS_FIELD] =
                difference.0 as f64;
            fields[super::temporal_date_arithmetic::DURATION_MONTHS_FIELD] =
                difference.1 as f64;
            fields[super::temporal_date_arithmetic::DURATION_WEEKS_FIELD] =
                difference.2 as f64;
            fields[super::temporal_date_arithmetic::DURATION_DAYS_FIELD] =
                difference.3 as f64;
            return self.make_temporal_duration(p, fields);
        }
        let direction = if native == Native::TemporalPlainDateTimeSince {
            -1_i128
        } else {
            1_i128
        };
        let left_total = datetime_nanos(left_date, left_time);
        let right_total = datetime_nanos(right_date, right_time);
        let signed_total = (right_total - left_total) * direction;
        let sign = signed_total.signum();
        let receiver_is_end = left_total >= right_total;
        let mut fields = [0.0; 10];
        if options.largest_rank() >= unit_rank("hour") {
            self.balance_time_difference(signed_total, &options, &mut fields)?;
        } else {
            self.balance_calendar_difference(
                p,
                (left_date, left_time),
                (right_date, right_time),
                sign,
                receiver_is_end,
                &options,
                &mut fields,
            )?;
        }
        self.make_temporal_duration(p, fields)
    }

    fn difference_options(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<DifferenceOptions, JsError> {
        if value.is_undefined() {
            return Ok(DifferenceOptions::default());
        }
        if !self.is_object_like(value) {
            return Err(self.type_error(p, "Invalid options".into()));
        }
        let largest = self.difference_unit_option(p, value, "largestUnit", true)?;
        let increment_atom = self.intern_atom("roundingIncrement");
        let increment_value = self.get_property(p, value, increment_atom)?;
        let increment = if increment_value.is_undefined() {
            1.0
        } else {
            self.to_number(p, increment_value)?.trunc()
        };
        let mode_atom = self.intern_atom("roundingMode");
        let mode_value = self.get_property(p, value, mode_atom)?;
        let rounding_mode = if mode_value.is_undefined() {
            "trunc".to_owned()
        } else {
            self.to_string(p, mode_value)?.to_string()
        };
        if !ROUNDING_MODES.contains(&rounding_mode.as_str()) {
            return Err(self.range_error(p, "Invalid roundingMode".into()));
        }
        let smallest = self.difference_unit_option(p, value, "smallestUnit", false)?;
        let smallest = smallest.unwrap_or("nanosecond");
        let largest = match largest {
            Some("auto") | None if unit_rank(smallest) < unit_rank("day") => smallest,
            Some("auto") | None => "day",
            Some(unit) => unit,
        };
        if unit_rank(smallest) < unit_rank(largest) {
            return Err(self.range_error(p, "smallestUnit larger than largestUnit".into()));
        }
        if !difference_increment_is_valid(increment, smallest, DifferenceDomain::PlainDateTime) {
            return Err(self.range_error(p, "Invalid roundingIncrement".into()));
        }
        Ok(DifferenceOptions {
            largest,
            smallest,
            increment: increment as i128,
            rounding_mode,
        })
    }

    fn difference_unit_option(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        name: &str,
        allow_auto: bool,
    ) -> Result<Option<&'static str>, JsError> {
        let atom = self.intern_atom(name);
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(None);
        }
        let value = self.to_string(p, value)?.to_string();
        let value = value.strip_suffix('s').unwrap_or(&value);
        if allow_auto && value == "auto" {
            return Ok(Some("auto"));
        }
        UNITS
            .iter()
            .copied()
            .find(|unit| *unit == value)
            .map(Some)
            .ok_or_else(|| self.range_error(p, format!("Invalid {name}")))
    }

    fn balance_time_difference(
        &mut self,
        total: i128,
        options: &DifferenceOptions,
        fields: &mut [f64; 10],
    ) -> Result<(), JsError> {
        let quantum = unit_nanos(options.smallest) * options.increment;
        let rounded = super::temporal_zoned_date_time::round_temporal_nanoseconds(
            total,
            quantum,
            &options.rounding_mode,
        ) * quantum;
        let sign = rounded.signum();
        let mut remainder = rounded.unsigned_abs() as i128;
        let mut values = [0_i128; 6];
        for (index, scale) in super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
            .into_iter()
            .enumerate()
        {
            values[index] = remainder / scale;
            remainder %= scale;
        }
        let first_unit = unit_rank(options.largest) - unit_rank("hour");
        if first_unit > 0 {
            values[first_unit] += values[..first_unit]
                .iter()
                .zip(
                    super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
                        .iter()
                        .take(first_unit),
                )
                .map(|(value, scale)| value.saturating_mul(*scale))
                .sum::<i128>()
                / super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES[first_unit];
            values[..first_unit].fill(0);
        }
        for (index, value) in values.into_iter().enumerate() {
            fields[index + 4] = (value * sign) as f64;
        }
        Ok(())
    }

    fn balance_calendar_difference(
        &mut self,
        p: &ResidualProgram,
        left: (IsoDate, [u32; 6]),
        right: (IsoDate, [u32; 6]),
        sign: i128,
        receiver_is_end: bool,
        options: &DifferenceOptions,
        fields: &mut [f64; 10],
    ) -> Result<(), JsError> {
        let (start, end) = if datetime_nanos(left.0, left.1) <= datetime_nanos(right.0, right.1) {
            (left, right)
        } else {
            (right, left)
        };
        let (receiver, target) = if receiver_is_end {
            (end, start)
        } else {
            (start, end)
        };
        let mut days =
            temporal_date::days_from_iso_date(end.0) - temporal_date::days_from_iso_date(start.0);
        let mut time = if receiver_is_end {
            time_nanos(receiver.1) - time_nanos(target.1)
        } else {
            time_nanos(target.1) - time_nanos(receiver.1)
        };
        if time < 0 {
            days -= 1;
            time += super::temporal_date_arithmetic::NANOS_PER_DAY;
        }
        let largest = options.largest;
        let mut months = 0_i128;
        let mut years = 0_i128;
        if matches!(largest, "year" | "month") {
            months = i128::from(end.0.year - start.0.year) * MONTHS_PER_YEAR
                + i128::from(end.0.month as i32 - start.0.month as i32);
            let receiver_anchor = if receiver_is_end {
                shift_months_clamped(receiver.0, -months)
            } else {
                shift_months_clamped(receiver.0, months)
            };
            let anchor_from_receiver = receiver_anchor.is_some();
            let anchor = receiver_anchor
                .or_else(|| shift_months_clamped(start.0, months))
                .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
            let anchor_total = datetime_nanos(anchor, receiver.1);
            let target_total = datetime_nanos(target.0, target.1);
            if (anchor_from_receiver && receiver_is_end && anchor_total < target_total)
                || (anchor_from_receiver && !receiver_is_end && anchor_total > target_total)
                || (!anchor_from_receiver
                    && datetime_nanos(anchor, start.1) > datetime_nanos(end.0, end.1))
            {
                months -= 1;
            }
            let anchor = if anchor_from_receiver && receiver_is_end {
                shift_months_clamped(receiver.0, -months)
            } else if anchor_from_receiver {
                shift_months_clamped(receiver.0, months)
            } else {
                shift_months_clamped(start.0, months)
            }
            .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
            days = if receiver_is_end {
                temporal_date::days_from_iso_date(anchor)
                    - temporal_date::days_from_iso_date(target.0)
            } else {
                temporal_date::days_from_iso_date(target.0)
                    - temporal_date::days_from_iso_date(anchor)
            };
            time = if receiver_is_end {
                time_nanos(receiver.1) - time_nanos(target.1)
            } else {
                time_nanos(target.1) - time_nanos(receiver.1)
            };
            if time < 0 {
                days -= 1;
                time += super::temporal_date_arithmetic::NANOS_PER_DAY;
            }
            if largest == "year" {
                years = months.div_euclid(MONTHS_PER_YEAR);
                months = months.rem_euclid(MONTHS_PER_YEAR);
            }
        }
        let mut weeks = 0_i128;
        if largest == "week" {
            let days_per_week = i64::try_from(super::temporal_date_arithmetic::DAYS_PER_WEEK)
                .expect("days per week fits i64");
            weeks = i128::from(days / days_per_week);
            days %= days_per_week;
        }
        let nanos_per_day = super::temporal_date_arithmetic::NANOS_PER_DAY;
        let subday = time_nanos(end.1) - time_nanos(start.1);
        let receiver_unit_direction = if receiver_is_end {
            -MONTHS_PER_YEAR.signum()
        } else {
            MONTHS_PER_YEAR.signum()
        };
        let rounded_calendar_unit = match options.smallest {
            "year" => {
                let receiver_year_shift = receiver_unit_direction * MONTHS_PER_YEAR;
                let year_anchor = shift_months_clamped(receiver.0, receiver_year_shift * years)
                    .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
                let year_boundary =
                    shift_months_clamped(year_anchor, receiver_unit_direction * MONTHS_PER_YEAR)
                        .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
                let year_nanos = datetime_nanos(year_boundary, receiver.1)
                    .abs_diff(datetime_nanos(year_anchor, receiver.1))
                    .try_into()
                    .unwrap_or(i128::MAX);
                let remainder = datetime_nanos(target.0, target.1)
                    .abs_diff(datetime_nanos(year_anchor, receiver.1))
                    .try_into()
                    .unwrap_or(i128::MAX);
                Some((years * year_nanos + remainder, year_nanos))
            }
            "month" => {
                let total_months = years * MONTHS_PER_YEAR + months;
                let receiver_month_shift = receiver_unit_direction;
                let month_anchor =
                    shift_months_clamped(receiver.0, receiver_month_shift * total_months)
                        .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
                let month_boundary = shift_months_clamped(month_anchor, receiver_month_shift)
                    .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
                let month_nanos = datetime_nanos(month_boundary, receiver.1)
                    .abs_diff(datetime_nanos(month_anchor, receiver.1))
                    .try_into()
                    .unwrap_or(i128::MAX);
                let remainder = datetime_nanos(target.0, target.1)
                    .abs_diff(datetime_nanos(month_anchor, receiver.1))
                    .try_into()
                    .unwrap_or(i128::MAX);
                Some((total_months * month_nanos + remainder, month_nanos))
            }
            "week" => {
                if days == 0 && time == 0 {
                    None
                } else {
                    let days_per_week = super::temporal_date_arithmetic::DAYS_PER_WEEK;
                    Some((
                        (weeks * days_per_week + i128::from(days)) * nanos_per_day + subday,
                        days_per_week * nanos_per_day,
                    ))
                }
            }
            _ => None,
        };
        if let Some((value, unit_nanos)) = rounded_calendar_unit {
            let rounded = super::temporal_zoned_date_time::round_temporal_nanoseconds(
                value * sign,
                unit_nanos * options.increment,
                &options.rounding_mode,
            )
            .abs()
                * options.increment;
            years = 0;
            months = 0;
            weeks = 0;
            days = 0;
            time = 0;
            match options.smallest {
                "year" => years = rounded,
                "month" if largest == "month" => months = rounded,
                "month" => {
                    years = rounded.div_euclid(MONTHS_PER_YEAR);
                    months = rounded.rem_euclid(MONTHS_PER_YEAR);
                }
                "week" if largest == "week" => weeks = rounded,
                "week" => {
                    days = i64::try_from(rounded * super::temporal_date_arithmetic::DAYS_PER_WEEK)
                        .map_err(|_| self.range_error(p, "Invalid PlainDateTime".into()))?;
                }
                _ => {}
            }
        }
        if largest == "year" && !matches!(options.smallest, "year" | "month" | "week" | "day") {
            let anchor = shift_months_clamped(start.0, years * MONTHS_PER_YEAR)
                .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
            let year_days = i128::from(temporal_date::iso_days_in_year(anchor.year));
            let residual_days = temporal_date::days_from_iso_date(end.0)
                - temporal_date::days_from_iso_date(anchor);
            let residual = i128::from(residual_days) * nanos_per_day + subday;
            let quantum = unit_nanos(options.smallest) * options.increment;
            let rounded_residual = super::temporal_zoned_date_time::round_temporal_nanoseconds(
                residual * sign,
                quantum,
                &options.rounding_mode,
            )
            .abs()
                * quantum;
            if rounded_residual >= year_days * nanos_per_day {
                years += 1;
                months = 0;
                weeks = 0;
                days = 0;
                time = 0;
            }
        }
        if options.smallest == "day" {
            let day = super::temporal_date_arithmetic::NANOS_PER_DAY;
            let quantity = days as f64 + time as f64 / day as f64;
            let rounded = super::temporal_zoned_date_time::round_temporal_nanoseconds(
                (quantity * day as f64) as i128 * sign,
                day * options.increment,
                &options.rounding_mode,
            )
            .abs();
            days = i64::try_from(rounded * options.increment)
                .map_err(|_| self.range_error(p, "Invalid PlainDateTime".into()))?;
            time = 0;
        } else {
            let quantum = unit_nanos(options.smallest) * options.increment;
            time = super::temporal_zoned_date_time::round_temporal_nanoseconds(
                time * sign,
                quantum,
                &options.rounding_mode,
            ) * quantum;
            time = time.abs();
            if time >= super::temporal_date_arithmetic::NANOS_PER_DAY {
                days += 1;
                time -= super::temporal_date_arithmetic::NANOS_PER_DAY;
            }
        }
        fields[0] = (years * sign) as f64;
        fields[1] = (months * sign) as f64;
        fields[2] = (weeks * sign) as f64;
        fields[3] = (i128::from(days) * sign) as f64;
        let mut remainder = time;
        for (index, scale) in super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
            .into_iter()
            .enumerate()
        {
            fields[index + 4] = (remainder / scale * sign) as f64;
            remainder %= scale;
        }
        Ok(())
    }
}

pub(super) fn difference_increment_is_valid(
    increment: f64,
    smallest: &str,
    domain: DifferenceDomain,
) -> bool {
    if !increment.is_finite() || increment < 1.0 {
        return false;
    }
    let maximum = match smallest {
        "year" | "week" | "day" => {
            let maximum = match domain {
                DifferenceDomain::PlainDateTime => MAX_PLAIN_DATE_TIME_ROUNDING_INCREMENT,
                DifferenceDomain::ZonedDateTime => {
                    MAX_CALENDAR_DIFFERENCE_ROUNDING_INCREMENT as f64
                }
            };
            return increment <= maximum;
        }
        "month" => 12.0,
        "hour" => 24.0,
        "minute" | "second" => 60.0,
        _ => 1_000.0,
    };
    increment < maximum && maximum % increment == 0.0
}

impl DifferenceOptions {
    fn largest_rank(&self) -> usize {
        unit_rank(self.largest)
    }
}

impl Default for DifferenceOptions {
    fn default() -> Self {
        Self {
            largest: "day",
            smallest: "nanosecond",
            increment: 1,
            rounding_mode: "trunc".into(),
        }
    }
}

fn unit_rank(unit: &str) -> usize {
    UNITS
        .iter()
        .position(|candidate| *candidate == unit)
        .unwrap_or(usize::MAX)
}

fn unit_nanos(unit: &str) -> i128 {
    match unit {
        "day" => super::temporal_date_arithmetic::NANOS_PER_DAY,
        "hour" | "minute" | "second" | "millisecond" | "microsecond" | "nanosecond" => {
            super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
                [unit_rank(unit) - unit_rank("hour")]
        }
        _ => 1,
    }
}

fn time_nanos(time: [u32; 6]) -> i128 {
    time.iter()
        .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
        .map(|(value, scale)| i128::from(*value) * scale)
        .sum()
}

fn datetime_nanos(date: IsoDate, time: [u32; 6]) -> i128 {
    i128::from(temporal_date::days_from_iso_date(date))
        * super::temporal_date_arithmetic::NANOS_PER_DAY
        + time_nanos(time)
}

fn shift_months_clamped(date: IsoDate, months: i128) -> Option<IsoDate> {
    let mut shifted = temporal_date::shift_iso_months(date, months)?;
    shifted.day = shifted
        .day
        .min(temporal_date::iso_days_in_month(shifted.year, shifted.month as i32)? as u32);
    Some(shifted)
}
