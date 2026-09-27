use super::*;

const ISO_MONTH_CODE_DIGITS: usize = 2;
const DEFAULT_REFERENCE_ISO_YEAR: i32 = 1972;
const DEFAULT_REFERENCE_ISO_DAY: u32 = 1;

impl<H: Host> Vm<H> {
    pub(super) fn install_temporal_calendar_projections(
        &mut self,
        p: &ResidualProgram,
        temporal: Value,
    ) -> Result<(), JsError> {
        for (name, native, to_string) in [
            (
                "PlainMonthDay",
                Native::TemporalPlainMonthDay,
                Native::TemporalPlainMonthDayToString,
            ),
            (
                "PlainYearMonth",
                Native::TemporalPlainYearMonth,
                Native::TemporalPlainYearMonthToString,
            ),
        ] {
            let constructor = self.native_with_realm(native, temporal, self.realm.globals);
            self.set_builtin_function_name(constructor, name)?;
            let prototype = self.object();
            self.set_builtin_value_named(constructor, "prototype", prototype)?;
            let prototype_atom = self.intern_atom("prototype");
            self.set_property_attributes(
                constructor,
                PropertyKey::string(prototype_atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
            self.set_builtin_value_named(prototype, "constructor", constructor)?;
            self.set_builtin_named(p, prototype, "toString", to_string)?;
            self.set_builtin_value_named(temporal, name, constructor)?;
        }
        Ok(())
    }
}

pub(super) fn to_plain_month_day<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    this: Value,
) -> Result<Value, JsError> {
    let (_year, month, day, calendar) = vm.temporal_plain_date_slots(p, this)?;
    let value = projection_object(vm, p, "PlainMonthDay")?;
    let code = month_code(vm, month);
    set_field(vm, value, "monthCode", code)?;
    set_field(vm, value, "day", Value::number(f64::from(day)))?;
    let calendar = string(vm, calendar);
    set_field(vm, value, "calendarId", calendar)?;
    set_field(
        vm,
        value,
        "referenceISODay",
        Value::number(f64::from(DEFAULT_REFERENCE_ISO_YEAR)),
    )?;
    Ok(value)
}

pub(super) fn to_plain_year_month<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    this: Value,
) -> Result<Value, JsError> {
    let (year, month, _day, calendar) = vm.temporal_plain_date_slots(p, this)?;
    let value = projection_object(vm, p, "PlainYearMonth")?;
    set_field(vm, value, "year", Value::number(f64::from(year)))?;
    set_field(vm, value, "month", Value::number(f64::from(month)))?;
    let code = month_code(vm, month);
    set_field(vm, value, "monthCode", code)?;
    let calendar = string(vm, calendar);
    set_field(vm, value, "calendarId", calendar)?;
    set_field(
        vm,
        value,
        "referenceISODay",
        Value::number(f64::from(DEFAULT_REFERENCE_ISO_DAY)),
    )?;
    Ok(value)
}

pub(super) fn native<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    native: Native,
    this: Value,
) -> Result<Value, JsError> {
    let calendar_atom = vm.intern_atom("calendarId");
    let calendar_value = vm.get_property(p, this, calendar_atom)?;
    let calendar = vm.to_string(p, calendar_value)?;
    let (year, month, day) = match native {
        Native::TemporalPlainMonthDayToString => {
            let reference_year = read_field(vm, p, this, "referenceISODay")?;
            let month_code = read_field(vm, p, this, "monthCode")?;
            let day = read_field(vm, p, this, "day")?;
            let month_code = vm.to_string(p, month_code)?;
            let month = month_code
                .trim_start_matches('M')
                .trim_end_matches('L')
                .parse::<u32>()
                .map_err(|_| vm.range_error(p, "Invalid PlainMonthDay".into()))?;
            (
                vm.to_number(p, reference_year)? as i32,
                month,
                vm.to_number(p, day)? as u32,
            )
        }
        Native::TemporalPlainYearMonthToString => {
            let year = read_field(vm, p, this, "year")?;
            let month = read_field(vm, p, this, "month")?;
            let day = read_field(vm, p, this, "referenceISODay")?;
            (
                vm.to_number(p, year)? as i32,
                vm.to_number(p, month)? as u32,
                vm.to_number(p, day)? as u32,
            )
        }
        _ => unreachable!("not a Temporal calendar projection native"),
    };
    let iso = super::temporal_date::format_iso_date(year, month, day);
    Ok(vm
        .heap
        .alloc(Cell::String(format!("{iso}[u-ca={calendar}]").into())))
}

fn read_field<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    object: Value,
    name: &str,
) -> Result<Value, JsError> {
    let atom = vm.intern_atom(name);
    vm.get_property(p, object, atom)
}

fn projection_object<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    name: &str,
) -> Result<Value, JsError> {
    let temporal_atom = vm.intern_atom("Temporal");
    let temporal = vm.get_property(p, vm.realm.globals, temporal_atom)?;
    let constructor_atom = vm.intern_atom(name);
    let constructor = vm.get_property(p, temporal, constructor_atom)?;
    let prototype_atom = vm.intern_atom("prototype");
    let prototype = vm.get_property(p, constructor, prototype_atom)?;
    let prototype = if vm.is_object_like(prototype) {
        prototype
    } else {
        vm.object_proto
    };
    Ok(vm
        .heap
        .alloc(Cell::Object(Vm::<H>::empty_object(prototype))))
}

fn set_field<H: Host>(
    vm: &mut Vm<H>,
    object: Value,
    name: &str,
    value: Value,
) -> Result<(), JsError> {
    let atom = vm.intern_atom(name);
    vm.set_property(object, atom, value)
}

fn month_code<H: Host>(vm: &mut Vm<H>, month: u32) -> Value {
    vm.heap.alloc(Cell::String(
        format!("M{:0width$}", month, width = ISO_MONTH_CODE_DIGITS).into(),
    ))
}

fn string<H: Host>(vm: &mut Vm<H>, value: String) -> Value {
    vm.heap.alloc(Cell::String(value.into()))
}
