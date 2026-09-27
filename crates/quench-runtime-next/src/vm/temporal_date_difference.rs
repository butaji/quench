use super::temporal_date::{self, IsoDate};
use super::*;

const DURATION_YEARS_FIELD: usize = 0;
const DURATION_MONTHS_FIELD: usize = 1;
const DURATION_WEEKS_FIELD: usize = 2;
const DURATION_DAYS_FIELD: usize = 3;
const MONTHS_PER_YEAR: i128 = 12;
const DAYS_PER_WEEK: i64 = 7;

#[derive(Clone, Copy)]
enum LargestDateUnit {
    Year,
    Month,
    Week,
    Day,
}

impl<H: Host> Vm<H> {
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
        let (start, end) = (to_iso_date(this), to_iso_date(other));
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let largest = self.plain_date_largest_unit(p, options)?;
        let mut fields = iso_date_difference(start, end, largest)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate difference".into()))?;
        if native == Native::TemporalPlainDateSince {
            fields
                .iter_mut()
                .filter(|value| **value != 0.0)
                .for_each(|value| *value = -*value);
        }
        self.make_temporal_duration(p, fields)
    }

    fn plain_date_largest_unit(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<LargestDateUnit, JsError> {
        if options.is_undefined() {
            return Ok(LargestDateUnit::Day);
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Options must be an object".into()));
        }
        let atom = self.intern_atom("largestUnit");
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(LargestDateUnit::Day);
        }
        let unit = self.to_string(p, value)?.to_string();
        match unit.as_str() {
            "year" | "years" => Ok(LargestDateUnit::Year),
            "month" | "months" => Ok(LargestDateUnit::Month),
            "week" | "weeks" => Ok(LargestDateUnit::Week),
            "day" | "days" => Ok(LargestDateUnit::Day),
            _ => Err(self.range_error(p, "Invalid largestUnit".into())),
        }
    }
}

fn to_iso_date((year, month, day, _): (i32, u32, u32, String)) -> IsoDate {
    IsoDate { year, month, day }
}

fn iso_date_difference(
    start: IsoDate,
    end: IsoDate,
    largest: LargestDateUnit,
) -> Option<[f64; 10]> {
    let difference =
        temporal_date::days_from_iso_date(end) - temporal_date::days_from_iso_date(start);
    let sign = difference.signum();
    let mut cursor = start;
    let mut fields = [0.0; 10];
    if sign == 0 {
        return Some(fields);
    }
    if matches!(largest, LargestDateUnit::Year) {
        let years = i128::from(end.year - start.year);
        let (count, next) = fit_largest(cursor, end, years, MONTHS_PER_YEAR)?;
        fields[DURATION_YEARS_FIELD] = count as f64;
        cursor = next;
    }
    if matches!(largest, LargestDateUnit::Year | LargestDateUnit::Month) {
        let months = i128::from(end.year - cursor.year) * MONTHS_PER_YEAR
            + i128::from(end.month as i32 - cursor.month as i32);
        let (count, next) = fit_largest(cursor, end, months, 1)?;
        fields[DURATION_MONTHS_FIELD] = count as f64;
        cursor = next;
    }
    let remaining_days =
        temporal_date::days_from_iso_date(end) - temporal_date::days_from_iso_date(cursor);
    if matches!(largest, LargestDateUnit::Week) {
        let weeks = remaining_days / DAYS_PER_WEEK;
        fields[DURATION_WEEKS_FIELD] = weeks as f64;
        cursor = temporal_date::shift_iso_days(cursor, weeks * DAYS_PER_WEEK)?;
    }
    fields[DURATION_DAYS_FIELD] =
        (temporal_date::days_from_iso_date(end) - temporal_date::days_from_iso_date(cursor)) as f64;
    Some(fields)
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
