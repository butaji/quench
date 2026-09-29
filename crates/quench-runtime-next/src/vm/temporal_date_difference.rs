use super::temporal_date::{self, IsoDate};
use super::*;

const DURATION_YEARS_FIELD: usize = 0;
const DURATION_MONTHS_FIELD: usize = 1;
const DURATION_WEEKS_FIELD: usize = 2;
const DURATION_DAYS_FIELD: usize = 3;
const MONTHS_PER_YEAR: i128 = 12;
const DAYS_PER_WEEK: i64 = 7;

const DATE_DIFFERENCE_ROUNDING_INCREMENT_LIMIT: f64 = 1_000_000_000.0;
const DATE_MONTH_ROUNDING_INCREMENT_LIMIT: f64 = 100_000_000.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DateUnit {
    Auto,
    Year,
    Month,
    Week,
    Day,
}

#[derive(Clone, Copy)]
enum DateRoundingMode {
    Ceil,
    Floor,
    Expand,
    HalfCeil,
    HalfFloor,
    HalfEven,
    HalfExpand,
    HalfTrunc,
    Trunc,
}

struct DateDifferenceOptions {
    largest: DateUnit,
    smallest: DateUnit,
    increment: f64,
    rounding_mode: DateRoundingMode,
}

struct YearMonthDifferenceOptions {
    largest: DateUnit,
    smallest: DateUnit,
    increment: f64,
    rounding_mode: DateRoundingMode,
}

enum ParsedOption<T> {
    Missing,
    Valid(T),
    Invalid,
}

impl<H: Host> Vm<H> {
    pub(super) fn temporal_plain_year_month_difference(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        start: (i32, u32, u32),
        end: (i32, u32, u32),
        calendar: String,
        options: Value,
    ) -> Result<Value, JsError> {
        let settings = self.year_month_difference_options(p, options)?;
        if !matches!(calendar.as_str(), "iso8601" | "gregory")
            && matches!(settings.largest, DateUnit::Year | DateUnit::Month)
            && settings.smallest == DateUnit::Month
            && settings.increment == 1.0
            && is_trunc(settings.rounding_mode)
        {
            let largest_unit = if settings.largest == DateUnit::Year {
                quench_intl::CalendarDifferenceUnit::Years
            } else {
                quench_intl::CalendarDifferenceUnit::Months
            };
            let difference = quench_intl::calendar_date_difference(
                start,
                end,
                &calendar,
                largest_unit,
                if native == Native::TemporalPlainYearMonthSince {
                    quench_intl::CalendarDifferenceDirection::Since
                } else {
                    quench_intl::CalendarDifferenceDirection::Until
                },
            )
            .ok_or_else(|| self.range_error(p, "Invalid PlainYearMonth difference".into()))?;
            let mut fields = [0.0; 10];
            fields[DURATION_YEARS_FIELD] = difference.0 as f64;
            fields[DURATION_MONTHS_FIELD] = difference.1 as f64;
            return self.make_temporal_duration(p, fields);
        }
        let direction = if native == Native::TemporalPlainYearMonthSince {
            -1.0
        } else {
            1.0
        };
        let total = ((end.0 - start.0) * super::temporal_date::ISO_MONTHS_PER_YEAR + end.1 as i32
            - start.1 as i32) as f64
            * direction;
        if total != 0.0 && settings.increment > f64::from(super::temporal_date::ISO_MONTHS_PER_YEAR)
        {
            return Err(self.range_error(p, "Invalid roundingIncrement".into()));
        }
        let (mut years, mut months) = year_month_difference_fields(total, &settings);
        if years == 0.0 {
            years = 0.0;
        }
        if months == 0.0 {
            months = 0.0;
        }
        self.validate_year_month_difference_target(p, (start.0, start.1), total, years, months)?;
        let mut fields = [0.0; 10];
        fields[0] = years;
        fields[1] = months;
        self.make_temporal_duration(p, fields)
    }

    fn year_month_difference_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<YearMonthDifferenceOptions, JsError> {
        if options.is_undefined() {
            return Ok(YearMonthDifferenceOptions::default());
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Invalid options".into()));
        }
        let largest = self.date_unit_option(p, options, "largestUnit")?;
        let increment = self.date_increment_option(p, options)?;
        let rounding_mode = self.date_rounding_mode_option(p, options)?;
        let smallest = self.date_unit_option(p, options, "smallestUnit")?;
        year_month_difference_settings(self, p, largest, smallest, increment, rounding_mode)
    }

    fn validate_year_month_difference_target(
        &mut self,
        p: &ResidualProgram,
        start: (i32, u32),
        total: f64,
        years: f64,
        months: f64,
    ) -> Result<(), JsError> {
        if total == 0.0 {
            return Ok(());
        }
        if start.0 == super::temporal_date::MIN_ISO_YEAR && start.1 == 4 {
            return Err(self.range_error(p, "Invalid PlainYearMonth".into()));
        }
        let target = i128::from(start.0) * i128::from(super::temporal_date::ISO_MONTHS_PER_YEAR)
            + i128::from(start.1 - 1)
            + (years * f64::from(super::temporal_date::ISO_MONTHS_PER_YEAR) + months) as i128;
        let year =
            i32::try_from(target.div_euclid(i128::from(super::temporal_date::ISO_MONTHS_PER_YEAR)))
                .map_err(|_| self.range_error(p, "Invalid PlainYearMonth".into()))?;
        let month =
            i32::try_from(target.rem_euclid(i128::from(super::temporal_date::ISO_MONTHS_PER_YEAR)))
                .map_err(|_| self.range_error(p, "Invalid PlainYearMonth".into()))?
                + 1;
        if super::temporal_date::checked_iso_date(year, month, 1).is_none() {
            return Err(self.range_error(p, "Invalid PlainYearMonth".into()));
        }
        Ok(())
    }

    pub(super) fn temporal_plain_date_difference(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let constructor = self.temporal_plain_date_constructor(p)?;
        let other_args = [args.first().copied().unwrap_or(Value::UNDEFINED)];
        let other = self.temporal_plain_date_from(p, constructor, &other_args)?;
        let this = self.temporal_plain_date_slots(p, this)?;
        let other = self.temporal_plain_date_slots(p, other)?;
        if this.3 != other.3 {
            return Err(self.range_error(p, "Calendar mismatch".into()));
        }
        let calendar = this.3.clone();
        let (start, end) = (to_iso_date(this), to_iso_date(other));
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let settings = self.plain_date_difference_options(p, options)?;
        if !matches!(calendar.as_str(), "iso8601" | "gregory")
            && matches!(settings.largest, DateUnit::Year | DateUnit::Month)
            && matches!(settings.smallest, DateUnit::Auto | DateUnit::Day)
            && settings.increment == 1.0
            && is_trunc(settings.rounding_mode)
        {
            let largest_unit = if settings.largest == DateUnit::Year {
                quench_intl::CalendarDifferenceUnit::Years
            } else {
                quench_intl::CalendarDifferenceUnit::Months
            };
            let difference = quench_intl::calendar_date_difference(
                (start.year, start.month, start.day),
                (end.year, end.month, end.day),
                &calendar,
                largest_unit,
                if native == Native::TemporalPlainDateSince {
                    quench_intl::CalendarDifferenceDirection::Since
                } else {
                    quench_intl::CalendarDifferenceDirection::Until
                },
            )
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate difference".into()))?;
            let mut fields = [0.0; 10];
            fields[DURATION_YEARS_FIELD] = difference.0 as f64;
            fields[DURATION_MONTHS_FIELD] = difference.1 as f64;
            fields[DURATION_WEEKS_FIELD] = difference.2 as f64;
            fields[DURATION_DAYS_FIELD] = difference.3 as f64;
            return self.make_temporal_duration(p, fields);
        }
        let mut fields = iso_date_difference(start, end, settings.largest)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate difference".into()))?;
        let date_sign = (temporal_date::days_from_iso_date(end)
            - temporal_date::days_from_iso_date(start))
        .signum() as f64;
        let is_since = native == Native::TemporalPlainDateSince;
        if is_since {
            fields
                .iter_mut()
                .filter(|value| **value != 0.0)
                .for_each(|value| *value = -*value);
        }
        let duration_sign = if is_since { -date_sign } else { date_sign };
        round_date_difference(start, end, &mut fields, &settings, date_sign, duration_sign);
        self.make_temporal_duration(p, fields)
    }

    fn plain_date_difference_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<DateDifferenceOptions, JsError> {
        if options.is_undefined() {
            return Ok(DateDifferenceOptions::default());
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Options must be an object".into()));
        }
        let largest = self.date_unit_option(p, options, "largestUnit")?;
        let increment = self.date_increment_option(p, options)?;
        let rounding_mode = self.date_rounding_mode_option(p, options)?;
        let smallest = self.date_unit_option(p, options, "smallestUnit")?;
        validate_date_difference_options(self, p, largest, smallest, increment, rounding_mode)
    }

    fn date_unit_option(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        name: &str,
    ) -> Result<ParsedOption<DateUnit>, JsError> {
        let atom = self.intern_atom(name);
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(ParsedOption::Missing);
        }
        let name = self.to_string(p, value)?.to_string();
        Ok(match parse_date_unit(&name) {
            Some(unit) => ParsedOption::Valid(unit),
            None => ParsedOption::Invalid,
        })
    }

    fn date_increment_option(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<f64, JsError> {
        let atom = self.intern_atom("roundingIncrement");
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(1.0);
        }
        Ok(self.to_number(p, value)?.trunc())
    }

    fn date_rounding_mode_option(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<ParsedOption<DateRoundingMode>, JsError> {
        let atom = self.intern_atom("roundingMode");
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(ParsedOption::Missing);
        }
        let mode = self.to_string(p, value)?.to_string();
        Ok(match parse_date_rounding_mode(&mode) {
            Some(mode) => ParsedOption::Valid(mode),
            None => ParsedOption::Invalid,
        })
    }
}

impl Default for DateDifferenceOptions {
    fn default() -> Self {
        Self {
            largest: DateUnit::Day,
            smallest: DateUnit::Auto,
            increment: 1.0,
            rounding_mode: DateRoundingMode::Trunc,
        }
    }
}

impl Default for YearMonthDifferenceOptions {
    fn default() -> Self {
        Self {
            largest: DateUnit::Year,
            smallest: DateUnit::Month,
            increment: 1.0,
            rounding_mode: DateRoundingMode::Trunc,
        }
    }
}

fn year_month_difference_settings<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    largest: ParsedOption<DateUnit>,
    smallest: ParsedOption<DateUnit>,
    increment: f64,
    rounding_mode: ParsedOption<DateRoundingMode>,
) -> Result<YearMonthDifferenceOptions, JsError> {
    let largest = match largest {
        ParsedOption::Missing | ParsedOption::Valid(DateUnit::Auto) => DateUnit::Year,
        ParsedOption::Valid(unit @ (DateUnit::Year | DateUnit::Month)) => unit,
        ParsedOption::Valid(_) | ParsedOption::Invalid => {
            return Err(vm.range_error(p, "Invalid largestUnit".into()));
        }
    };
    let smallest = match smallest {
        ParsedOption::Missing | ParsedOption::Valid(DateUnit::Auto) => DateUnit::Month,
        ParsedOption::Valid(unit @ (DateUnit::Year | DateUnit::Month)) => unit,
        ParsedOption::Valid(_) | ParsedOption::Invalid => {
            return Err(vm.range_error(p, "Invalid smallestUnit".into()));
        }
    };
    let rounding_mode = match rounding_mode {
        ParsedOption::Missing => DateRoundingMode::Trunc,
        ParsedOption::Valid(mode) => mode,
        ParsedOption::Invalid => return Err(vm.range_error(p, "Invalid roundingMode".into())),
    };
    if !increment.is_finite()
        || !(1.0..=DATE_DIFFERENCE_ROUNDING_INCREMENT_LIMIT).contains(&increment)
        || !valid_date_unit_order(largest, smallest)
    {
        return Err(vm.range_error(p, "Invalid difference options".into()));
    }
    Ok(YearMonthDifferenceOptions {
        largest,
        smallest,
        increment,
        rounding_mode,
    })
}

fn year_month_difference_fields(
    total_months: f64,
    settings: &YearMonthDifferenceOptions,
) -> (f64, f64) {
    if settings.smallest == DateUnit::Year {
        return (
            round_date_scalar(
                total_months / MONTHS_PER_YEAR as f64,
                settings.increment,
                settings.rounding_mode,
            ),
            0.0,
        );
    }
    if settings.largest == DateUnit::Month {
        let months = round_date_scalar(total_months, settings.increment, settings.rounding_mode);
        return (0.0, months);
    }
    let years = (total_months / MONTHS_PER_YEAR as f64).trunc();
    let months = round_date_scalar(
        total_months - years * MONTHS_PER_YEAR as f64,
        settings.increment,
        settings.rounding_mode,
    );
    if months.abs() >= MONTHS_PER_YEAR as f64 {
        (
            years + months.signum(),
            months - months.signum() * MONTHS_PER_YEAR as f64,
        )
    } else {
        (years, months)
    }
}

fn to_iso_date((year, month, day, _): (i32, u32, u32, String)) -> IsoDate {
    IsoDate { year, month, day }
}

fn parse_date_unit(value: &str) -> Option<DateUnit> {
    match value {
        "auto" => Some(DateUnit::Auto),
        "year" | "years" => Some(DateUnit::Year),
        "month" | "months" => Some(DateUnit::Month),
        "week" | "weeks" => Some(DateUnit::Week),
        "day" | "days" => Some(DateUnit::Day),
        _ => None,
    }
}

fn parse_date_rounding_mode(value: &str) -> Option<DateRoundingMode> {
    match value {
        "ceil" => Some(DateRoundingMode::Ceil),
        "floor" => Some(DateRoundingMode::Floor),
        "expand" => Some(DateRoundingMode::Expand),
        "halfCeil" => Some(DateRoundingMode::HalfCeil),
        "halfFloor" => Some(DateRoundingMode::HalfFloor),
        "halfEven" => Some(DateRoundingMode::HalfEven),
        "halfExpand" => Some(DateRoundingMode::HalfExpand),
        "halfTrunc" => Some(DateRoundingMode::HalfTrunc),
        "trunc" => Some(DateRoundingMode::Trunc),
        _ => None,
    }
}

fn validate_date_difference_options<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    largest: ParsedOption<DateUnit>,
    smallest: ParsedOption<DateUnit>,
    increment: f64,
    rounding_mode: ParsedOption<DateRoundingMode>,
) -> Result<DateDifferenceOptions, JsError> {
    let largest = match largest {
        ParsedOption::Missing => DateUnit::Auto,
        ParsedOption::Valid(unit) => unit,
        ParsedOption::Invalid => return Err(vm.range_error(p, "Invalid largestUnit".into())),
    };
    let smallest = match smallest {
        ParsedOption::Missing => DateUnit::Auto,
        ParsedOption::Valid(unit) => unit,
        ParsedOption::Invalid => return Err(vm.range_error(p, "Invalid smallestUnit".into())),
    };
    let rounding_mode = match rounding_mode {
        ParsedOption::Missing => DateRoundingMode::Trunc,
        ParsedOption::Valid(mode) => mode,
        ParsedOption::Invalid => return Err(vm.range_error(p, "Invalid roundingMode".into())),
    };
    if !increment.is_finite()
        || !(1.0..=DATE_DIFFERENCE_ROUNDING_INCREMENT_LIMIT).contains(&increment)
    {
        return Err(vm.range_error(p, "Invalid roundingIncrement".into()));
    }
    if !valid_date_unit_order(largest, smallest) {
        return Err(vm.range_error(p, "smallestUnit is larger than largestUnit".into()));
    }
    let smallest = if smallest == DateUnit::Auto && (increment != 1.0 || !is_trunc(rounding_mode)) {
        DateUnit::Day
    } else {
        smallest
    };
    let largest = if largest == DateUnit::Auto {
        if smallest == DateUnit::Auto {
            DateUnit::Day
        } else {
            smallest
        }
    } else {
        largest
    };
    if smallest == DateUnit::Month && increment >= DATE_MONTH_ROUNDING_INCREMENT_LIMIT {
        return Err(vm.range_error(p, "Rounded PlainDate is out of range".into()));
    }
    Ok(DateDifferenceOptions {
        largest,
        smallest,
        increment,
        rounding_mode,
    })
}

fn valid_date_unit_order(largest: DateUnit, smallest: DateUnit) -> bool {
    if largest == DateUnit::Auto || smallest == DateUnit::Auto {
        return true;
    }
    date_unit_rank(smallest) >= date_unit_rank(largest)
}

fn date_unit_rank(unit: DateUnit) -> u8 {
    match unit {
        DateUnit::Year => 0,
        DateUnit::Month => 1,
        DateUnit::Week => 2,
        DateUnit::Day => 3,
        DateUnit::Auto => 0,
    }
}

fn is_trunc(mode: DateRoundingMode) -> bool {
    matches!(mode, DateRoundingMode::Trunc)
}

fn iso_date_difference(start: IsoDate, end: IsoDate, largest: DateUnit) -> Option<[f64; 10]> {
    let difference =
        temporal_date::days_from_iso_date(end) - temporal_date::days_from_iso_date(start);
    let sign = difference.signum();
    let mut cursor = start;
    let mut fields = [0.0; 10];
    if sign == 0 {
        return Some(fields);
    }
    if matches!(largest, DateUnit::Year) {
        let years = i128::from(end.year - start.year);
        let (count, next) = fit_largest(cursor, end, years, MONTHS_PER_YEAR)?;
        fields[DURATION_YEARS_FIELD] = count as f64;
        cursor = next;
    }
    if matches!(largest, DateUnit::Year | DateUnit::Month) {
        let months = i128::from(end.year - cursor.year) * MONTHS_PER_YEAR
            + i128::from(end.month as i32 - cursor.month as i32);
        let (count, next) = fit_largest(cursor, end, months, 1)?;
        fields[DURATION_MONTHS_FIELD] = count as f64;
        cursor = next;
    }
    let remaining_days =
        temporal_date::days_from_iso_date(end) - temporal_date::days_from_iso_date(cursor);
    if matches!(largest, DateUnit::Week) {
        let weeks = remaining_days / DAYS_PER_WEEK;
        fields[DURATION_WEEKS_FIELD] = weeks as f64;
        cursor = temporal_date::shift_iso_days(cursor, weeks * DAYS_PER_WEEK)?;
    }
    fields[DURATION_DAYS_FIELD] =
        (temporal_date::days_from_iso_date(end) - temporal_date::days_from_iso_date(cursor)) as f64;
    Some(fields)
}

fn round_date_difference(
    start: IsoDate,
    end: IsoDate,
    fields: &mut [f64; 10],
    settings: &DateDifferenceOptions,
    date_sign: f64,
    duration_sign: f64,
) {
    if settings.smallest == DateUnit::Auto {
        return;
    }
    let scalar = match settings.smallest {
        DateUnit::Year => rounded_year_scalar(start, end, fields, date_sign, duration_sign),
        DateUnit::Month => rounded_month_scalar(start, end, fields, date_sign, duration_sign),
        DateUnit::Week => fields[DURATION_WEEKS_FIELD] + fields[DURATION_DAYS_FIELD] / 7.0,
        DateUnit::Day => fields[DURATION_DAYS_FIELD],
        DateUnit::Auto => return,
    };
    let rounded = round_date_scalar(scalar, settings.increment, settings.rounding_mode);
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    match settings.smallest {
        DateUnit::Year => {
            fields[DURATION_YEARS_FIELD] = rounded;
            fields[DURATION_MONTHS_FIELD] = 0.0;
            fields[DURATION_WEEKS_FIELD] = 0.0;
            fields[DURATION_DAYS_FIELD] = 0.0;
        }
        DateUnit::Month => {
            fields[DURATION_MONTHS_FIELD] = rounded;
            fields[DURATION_WEEKS_FIELD] = 0.0;
            fields[DURATION_DAYS_FIELD] = 0.0;
            if settings.largest == DateUnit::Year {
                let years = (rounded / MONTHS_PER_YEAR as f64).trunc();
                fields[DURATION_YEARS_FIELD] = years;
                fields[DURATION_MONTHS_FIELD] = rounded - years * MONTHS_PER_YEAR as f64;
            }
        }
        DateUnit::Week => {
            fields[DURATION_WEEKS_FIELD] = rounded;
            fields[DURATION_DAYS_FIELD] = 0.0;
        }
        DateUnit::Day | DateUnit::Auto => fields[DURATION_DAYS_FIELD] = rounded,
    }
    fields.iter_mut().for_each(|field| {
        if *field == 0.0 {
            *field = 0.0;
        }
    });
}

fn rounded_year_scalar(
    start: IsoDate,
    end: IsoDate,
    fields: &[f64; 10],
    date_sign: f64,
    duration_sign: f64,
) -> f64 {
    let years = fields[DURATION_YEARS_FIELD] * duration_sign;
    let Some(anchor) = temporal_date::shift_iso_months(
        start,
        (years * date_sign * MONTHS_PER_YEAR as f64) as i128,
    ) else {
        return fields[DURATION_YEARS_FIELD];
    };
    let remainder = (temporal_date::days_from_iso_date(end)
        - temporal_date::days_from_iso_date(anchor))
    .unsigned_abs() as f64;
    let Some(next_year) =
        temporal_date::shift_iso_months(anchor, if date_sign < 0.0 { -12 } else { 12 })
    else {
        return fields[DURATION_YEARS_FIELD];
    };
    let year_length = (temporal_date::days_from_iso_date(next_year)
        - temporal_date::days_from_iso_date(anchor))
    .unsigned_abs() as f64;
    (years + remainder / year_length) * duration_sign
}

fn rounded_month_scalar(
    start: IsoDate,
    end: IsoDate,
    fields: &[f64; 10],
    date_sign: f64,
    duration_sign: f64,
) -> f64 {
    let months = fields[DURATION_MONTHS_FIELD] * duration_sign
        + fields[DURATION_YEARS_FIELD] * duration_sign * MONTHS_PER_YEAR as f64;
    let Some(anchor) = temporal_date::shift_iso_months(start, (months * date_sign) as i128) else {
        return fields[DURATION_MONTHS_FIELD];
    };
    let remainder = (temporal_date::days_from_iso_date(end)
        - temporal_date::days_from_iso_date(anchor))
    .unsigned_abs() as f64;
    let neighbor = temporal_date::shift_iso_months(anchor, if date_sign < 0.0 { -1 } else { 1 });
    let Some(neighbor) = neighbor else {
        return fields[DURATION_MONTHS_FIELD];
    };
    let month_length = (temporal_date::days_from_iso_date(neighbor)
        - temporal_date::days_from_iso_date(anchor))
    .unsigned_abs() as f64;
    (months + remainder / month_length) * duration_sign
}

fn round_date_scalar(value: f64, increment: f64, mode: DateRoundingMode) -> f64 {
    let scaled = value / increment;
    let rounded = match mode {
        DateRoundingMode::Ceil => scaled.ceil(),
        DateRoundingMode::Floor => scaled.floor(),
        DateRoundingMode::Expand => {
            if scaled.is_sign_negative() {
                scaled.floor()
            } else {
                scaled.ceil()
            }
        }
        DateRoundingMode::HalfCeil => (scaled + 0.5).floor(),
        DateRoundingMode::HalfFloor => (scaled - 0.5).ceil(),
        DateRoundingMode::HalfEven => {
            let floor = scaled.floor();
            let fraction = scaled - floor;
            if (fraction - 0.5).abs() < f64::EPSILON {
                if (floor as i64) % 2 == 0 {
                    floor
                } else {
                    floor + 1.0
                }
            } else if fraction < 0.5 {
                floor
            } else {
                floor + 1.0
            }
        }
        DateRoundingMode::HalfExpand => {
            if scaled.is_sign_negative() {
                (scaled - 0.5).ceil()
            } else {
                (scaled + 0.5).floor()
            }
        }
        DateRoundingMode::HalfTrunc => {
            let truncated = scaled.trunc();
            if scaled.abs() - truncated.abs() > 0.5 {
                truncated + scaled.signum()
            } else {
                truncated
            }
        }
        DateRoundingMode::Trunc => scaled.trunc(),
    };
    rounded * increment
}

fn fit_largest(
    start: IsoDate,
    end: IsoDate,
    estimate: i128,
    month_scale: i128,
) -> Option<(i128, IsoDate)> {
    let direction = (temporal_date::days_from_iso_date(end)
        - temporal_date::days_from_iso_date(start))
    .signum();
    let mut count = estimate;
    let mut candidate = temporal_date::shift_iso_months(start, count * month_scale)?;
    if !reached_target(candidate, end, direction) {
        count -= i128::from(direction);
        candidate = temporal_date::shift_iso_months(start, count * month_scale)?;
    }
    let next_count = count + i128::from(direction);
    if let Some(next) = temporal_date::shift_iso_months(start, next_count * month_scale)
        .filter(|next| reached_target(*next, end, direction))
    {
        count = next_count;
        candidate = next;
    }
    Some((count, candidate))
}

fn reached_target(candidate: IsoDate, target: IsoDate, direction: i64) -> bool {
    if direction > 0 {
        (candidate.year, candidate.month, candidate.day) <= (target.year, target.month, target.day)
    } else {
        (candidate.year, candidate.month, candidate.day) >= (target.year, target.month, target.day)
    }
}
