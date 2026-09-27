use super::temporal_date::{self, IsoDate};
use super::*;

const DURATION_YEARS_FIELD: usize = 0;
const DURATION_MONTHS_FIELD: usize = 1;
const DURATION_WEEKS_FIELD: usize = 2;
const DURATION_DAYS_FIELD: usize = 3;
const DAYS_PER_WEEK: i128 = 7;
const NANOS_PER_DAY: i128 = 86_400_000_000_000;
const TIME_UNIT_NANOSECOND_SCALES: [i128; 6] = [
    3_600_000_000_000,
    60_000_000_000,
    1_000_000_000,
    1_000_000,
    1_000,
    1,
];

impl<H: Host> Vm<H> {
    pub(super) fn temporal_plain_date_arithmetic(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let mut duration =
            self.duration_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if native == Native::TemporalPlainDateSubtract {
            duration.iter_mut().for_each(|field| *field = -*field);
        }
        self.validate_duration_fields(p, &duration)?;
        let overflow = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let constrain = self.plain_date_overflow(p, overflow)?;
        let (year, month, day, calendar) = self.temporal_plain_date_slots(p, this)?;
        let start = IsoDate { year, month, day };
        let month_delta =
            duration[DURATION_YEARS_FIELD] as i128 * 12 + duration[DURATION_MONTHS_FIELD] as i128;
        let month_shifted = temporal_date::shift_iso_months(start, month_delta)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
        if !constrain && month_shifted.day != day {
            return Err(self.range_error(p, "Invalid PlainDate".into()));
        }
        let subday_nanos = duration[4..]
            .iter()
            .zip(TIME_UNIT_NANOSECOND_SCALES)
            .map(|(value, scale)| *value as i128 * scale)
            .sum::<i128>();
        let calendar_days = duration[DURATION_WEEKS_FIELD] as i128 * DAYS_PER_WEEK
            + duration[DURATION_DAYS_FIELD] as i128
            + subday_nanos / NANOS_PER_DAY;
        let days = i64::try_from(calendar_days)
            .map_err(|_| self.range_error(p, "Invalid PlainDate".into()))?;
        let result = temporal_date::shift_iso_days(month_shifted, days)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
        let constructor = self.temporal_plain_date_constructor(p)?;
        self.make_temporal_plain_date(p, result, calendar, constructor)
    }

    pub(super) fn temporal_plain_date_constructor(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<Value, JsError> {
        let temporal_atom = self.intern_atom("Temporal");
        let plain_date_atom = self.intern_atom("PlainDate");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        self.get_property(p, temporal, plain_date_atom)
    }
}
