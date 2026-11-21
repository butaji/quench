use super::wtf16::JsString;
use super::*;
use oxc_ast::ast::UnaryOperator;

#[path = "switch_selection.rs"]
mod switch_selection;
#[path = "strict_equality.rs"]
mod strict_equality;

/// V8's TypeError for ToNumber of a BigInt, unary plus included.
const BIGINT_TO_NUMBER: &str = "Cannot convert a BigInt value to a number";
const UNARY_PLUS: u32 = UnaryOperator::UnaryPlus as u32;
const UNARY_NEGATION: u32 = UnaryOperator::UnaryNegation as u32;
pub(super) const LOGICAL_NOT: u32 = UnaryOperator::LogicalNot as u32;
const BITWISE_NOT: u32 = UnaryOperator::BitwiseNot as u32;
pub(super) const TYPEOF: u32 = UnaryOperator::Typeof as u32;
const VOID: u32 = UnaryOperator::Void as u32;

/// The unary operators that convert their operand to a numeric value; on a Number they
/// run no guest code.
#[inline] pub(super) fn numeric_unary(op: u32) -> bool {
    matches!(op, UNARY_PLUS | UNARY_NEGATION | BITWISE_NOT)
}

impl Vm {
    pub(super) fn typeof_value(&mut self, value: Value) -> Value {
        let mut stage = crate::staged::Concrete::new(std::ptr::null());
        let names = self.typeof_names.as_ptr() as u64;
        match crate::staged::value::immediate_typeof_string(&mut stage, value.to_bits(), names, ()) {
            Some(name) => Value::from_bits(name),
            None => self.typeof_name(self.typeof_kind(value)),
        }
    }

    /// The kind whose name `typeof value` yields: an `[[IsHTMLDDA]]` object reads as
    /// undefined (ECMA-262 B.3.6.3).
    #[inline]
    pub(super) fn typeof_kind(&self, value: Value) -> crate::ValueType {
        if self.is_html_dda(value) { crate::ValueType::Undefined } else { self.value_type(value) }
    }

    /// The integer a `UNARY_TYPEOF_INDEX` operator yields for `value`: the index in
    /// `bytecode::TYPEOF_NAMES` of the name `typeof value` is.
    #[inline]
    pub(super) fn typeof_index_value(&self, value: Value) -> Value {
        let index = immediate_typeof_index(value).unwrap_or_else(|| typeof_name_index(self.typeof_kind(value)));
        Value::integer(index as i32)
    }

    /// The boolean a `UNARY_TYPEOF_IS` operator testing the name at `index` in
    /// `bytecode::TYPEOF_NAMES` yields for `value`.
    #[inline]
    pub(super) fn typeof_test_value(&self, value: Value, index: usize, negated: bool) -> Value {
        let named = immediate_typeof_index(value).unwrap_or_else(|| typeof_name_index(self.typeof_kind(value)));
        if (named == index) != negated { Value::TRUE } else { Value::FALSE }
    }

    /// The one string `typeof` yields for values of `kind`: every result and every string
    /// constant spelling one (`Vm::materialize_constant`) is this value, so `===` between two
    /// of them is identity. `null` and objects share the object's.
    fn typeof_name(&self, kind: crate::ValueType) -> Value {
        self.typeof_names[typeof_name_index(kind)]
    }

    /// Interns the eight `typeof` result strings of a fresh heap, in the order of `bytecode::TYPEOF_NAMES`.
    pub(super) fn intern_typeof_names(&mut self) {
        self.typeof_names = crate::bytecode::TYPEOF_NAMES.map(|text| self.intern_dynamic_ascii(text));
    }

    /// The `typeof` name spelled by `text`, if it spells one (`Vm::typeof_name`).
    pub(super) fn typeof_constant(&self, text: &str) -> Option<Value> {
        crate::bytecode::typeof_name_index(text).map(|index| self.typeof_names[index])
    }

    #[expect(clippy::wrong_self_convention, reason = "named for the spec abstract operation, which may run guest code")]
    pub(super) fn to_numeric(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        // ToNumeric of a number is that number.
        if value.as_number().is_some() {
            return Ok(value);
        }
        let primitive = if self.is_object_like(value) {
            self.to_primitive(p, value, "number")?
        } else {
            value
        };
        if matches!(self.heap.get(primitive), Some(Cell::BigInt(_))) {
            Ok(primitive)
        } else {
            Ok(Value::number(self.to_number(p, primitive)?))
        }
    }

    #[expect(clippy::wrong_self_convention, reason = "named for the spec abstract operation, which may run guest code")]
    pub(super) fn to_bigint(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<num_bigint::BigInt, JsError> {
        let primitive = if self.is_object_like(value) {
            self.to_primitive(p, value, "number")?
        } else {
            value
        };
        match self.heap.get(primitive) {
            Some(Cell::BigInt(value)) => Ok(value.to_num()),
            _ if let Some(value) = self.heap.string(primitive)  => {
                let Some(value) = crate::bigint::parse_string(value.host_string()) else {
                    let message = self.bigint_conversion_message(primitive);
                    return self
                        .syntax_error_result(p, &message)
                        .map(|_| num_bigint::BigInt::default());
                };
                Ok(value)
            }
            _ => match primitive.as_bool() {
                Some(value) => Ok(num_bigint::BigInt::from(i32::from(value))),
                None => Err(self.type_error(p, self.bigint_conversion_message(primitive))),
            },
        }
    }

    /// V8's text for a primitive that does not convert to a BigInt (ToBigInt and the
    /// `BigInt` constructor): the value as V8 details it (`detail_string`).
    pub(super) fn bigint_conversion_message(&self, primitive: Value) -> String {
        format!("Cannot convert {} to a BigInt", self.detail_string(primitive))
    }

    #[expect(clippy::wrong_self_convention, reason = "named for the spec abstract operation, which may run guest code")]
    pub(super) fn to_property_key(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let primitive = if self.is_object_like(value) {
            self.to_primitive(program, value, "string")?
        } else {
            value
        };
        if self.heap.is_string(primitive) || self.heap.is_symbol(primitive) {
            return Ok(primitive);
        }
        // An array index's key, as a native's loop over a proxy or an array-like makes one per
        // element: the string its atom keeps, which a later ToPropertyKey of it reads back as
        // that atom (`string_key_atom`) instead of parsing it.
        if let Some(index) = primitive.as_int().and_then(|index| usize::try_from(index).ok()) {
            let atom = self.index_key_atom(index);
            return Ok(self.atom_string(atom));
        }
        let text = self.coerce_js_string(program, primitive)?;
        Ok(self.heap.alloc_string(text))
    }

    /// The string value of `text`, which ToString produced from `argument`. ToString is the
    /// identity on strings, so a string argument is reused rather than copied into a new cell.
    pub(super) fn string_value_from(&mut self, argument: Value, text: &JsString) -> Value {
        if self.heap.is_string(argument) {
            argument
        } else {
            self.heap.alloc_string(text.clone())
        }
    }

    pub(super) fn coerce_js_string(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<JsString, JsError> {
        let primitive = if self.is_object_like(value) {
            self.to_primitive(program, value, "string")?
        } else {
            value
        };
        if let Some(text) = self.heap.string(primitive) {
            return Ok(text.clone());
        }
        match primitive.as_number() {
            Some(number) => Ok(JsString::from_number(number)),
            None => self.to_string(program, primitive).map(JsString::from),
        }
    }

    /// `!value`: the negated truthiness.
    #[inline(always)]
    pub(super) fn logical_not(&self, value: Value) -> Value {
        if self.truthy(value) {
            Value::FALSE
        } else {
            Value::TRUE
        }
    }

    pub(super) fn unary(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        value: Value,
    ) -> Result<Value, JsError> {
        let numeric = numeric_unary(op);
        let value = if numeric && self.is_object_like(value) {
            self.to_primitive(p, value, "number")?
        } else {
            value
        };
        if numeric && let Some(Cell::BigInt(value)) = self.heap.get(value) {
            if op == UNARY_PLUS {
                return Err(self.type_error(p, BIGINT_TO_NUMBER.into()));
            }
            if op == UNARY_NEGATION {
                let negated = value.negate();
                return Ok(self.heap.alloc_bigint(negated));
            }
            if op == BITWISE_NOT {
                let complement = value.not();
                return Ok(self.heap.alloc_bigint(complement));
            }
        }
        Ok(match op {
            op if super::operations::NumberUnary::from_operator(op).is_some() => {
                let number = self.to_number(p, value)?;
                Value::number(
                    super::operations::number_unary_result(op, number)
                        .expect("selected Number unary operation"),
                )
            }
            LOGICAL_NOT => self.logical_not(value),
            BITWISE_NOT => {
                Value::number((!(number_to_u32(self.to_number(p, value)?) as i32)) as f64)
            }
            TYPEOF => return Ok(self.typeof_value(value)),
            VOID => Value::UNDEFINED,
            op if let Some((index, negated)) = crate::bytecode::typeof_test(op) => {
                self.typeof_test_value(value, index, negated)
            }
            crate::bytecode::UNARY_TYPEOF_INDEX => self.typeof_index_value(value),
            op if let Some(elements) = crate::bytecode::array_pattern_test(op) => {
                Value::boolean(self.array_pattern_fits(value, elements))
            }
            op @ (crate::bytecode::UNARY_TO_STRING_PRIMITIVE | crate::bytecode::UNARY_ADD_OPERAND_STRING) => return self.string_primitive(p, op, value),
            _ => return Err(JsError("unsupported unary operator".into())),
        })
    }

    /// ToNumber of `value` where it runs no guest code and raises nothing: an immediate
    /// primitive or a string. `None` for any other value.
    #[inline] pub(super) fn primitive_to_number(&self, value: Value) -> Option<f64> {
        value
            .immediate_number()
            .or_else(|| self.heap.string(value).map(|text| text.read_units(super::number::string_to_number_units)))
    }

    #[expect(clippy::wrong_self_convention, reason = "named for the spec abstract operation, which may run guest code")]
    pub(super) fn to_number(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<f64, JsError> {
        if let Some(number) = self.primitive_to_number(value) {
            return Ok(number);
        }
        if self.object_data(value).is_some() {
            let primitive = self.to_primitive(program, value, "number")?;
            return self.to_number(program, primitive);
        }
        match self.heap.get(value) {
            Some(Cell::Date { milliseconds, .. }) => return Ok(*milliseconds),
            _ if self.heap.is_symbol(value) => {
                return Err(
                    self.type_error(program, "Cannot convert a Symbol value to a number".into())
                );
            }
            Some(Cell::BigInt(_)) => {
                return Err(
                    self.type_error(program, BIGINT_TO_NUMBER.into())
                );
            }
            _ => {}
        }
        let function = self.get_property(program, value, self.runtime_atoms.value_of)?;
        if !function.is_undefined() {
            let result = self.call_value(program, function, value, &[])?;
            return self.to_number(program, result);
        }
        Ok(f64::NAN)
    }

    #[expect(clippy::wrong_self_convention)]
    pub(super) fn to_string(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<String, JsError> {
        if let Some(text) = immediate_to_string(value) {
            return Ok(text.into());
        }
        if self.object_data(value).is_some() {
            let primitive = self.to_primitive(program, value, "string")?;
            return self.to_string(program, primitive);
        }
        match self.heap.get(value) {
            _ if self.heap.is_function_value(value) => return Ok("function () { [native code] }".into()),
            _ if let Some(value) = self.heap.string(value)  => return Ok(value.to_host_owned()),
            Some(Cell::Error(value)) => return Ok(value.clone()),
            Some(Cell::BigInt(value)) => return Ok(value.to_string()),
            _ if self.heap.is_symbol(value) => {
                return Err(
                    self.type_error(program, "Cannot convert a Symbol value to a string".into())
                );
            }
            Some(Cell::Date { milliseconds, .. }) => return Ok(milliseconds.to_string()),
            _ => {}
        }
        let function = self.get_property(program, value, self.runtime_atoms.to_string)?;
        if self.is_function(function) {
            let result = self.call_value(program, function, value, &[])?;
            return self.to_string(program, result);
        }
        let function = self.get_property(program, value, self.runtime_atoms.value_of)?;
        if self.is_function(function) {
            let result = self.call_value(program, function, value, &[])?;
            if !result.is_heap() || self.heap.is_string(result) {
                return self.to_string(program, result);
            }
        }
        Ok("[object Object]".into())
    }

    #[expect(clippy::wrong_self_convention, reason = "named for the spec abstract operation, which may run guest code")]
    pub(super) fn to_primitive(
        &mut self,
        program: &ResidualProgram,
        value: Value,
        hint: &str,
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        if !self.is_object_like(value) {
            return Ok(value);
        }
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::ToPrimitive) {
            let method = self.get_index(program, value, symbol)?;
            if !method.is_undefined() && !method.is_null() {
                if !self.is_function(method) {
                    return Err(
                        self.type_error(program, "Symbol.toPrimitive is not callable".into())
                    );
                }
                let hint = self.heap.alloc_string(hint.into());
                let result = self.call_value(program, method, value, &[hint])?;
                if !self.is_object_like(result) {
                    return Ok(result);
                }
                return Err(
                    self.type_error(program, "Cannot convert object to primitive value".into())
                );
            }
        }
        self.ordinary_to_primitive(program, value, hint)
    }

    pub(super) fn ordinary_to_primitive(
        &mut self,
        program: &ResidualProgram,
        value: Value,
        hint: &str,
    ) -> Result<Value, JsError> {
        let names = if hint == "string" {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for name in names {
            let atom = self.intern_atom(name);
            let method = self.get_property(program, value, atom)?;
            if self.is_function(method) {
                let result = self.call_value(program, method, value, &[])?;
                if !self.is_object_like(result) {
                    return Ok(result);
                }
            }
        }
        Err(self.type_error(program, "Cannot convert object to primitive value".into()))
    }

    pub(super) fn strict_equal(&self, a: Value, b: Value) -> bool {
        if let (Some(left), Some(right)) = (a.as_number(), b.as_number()) {
            return left == right;
        }
        a == b || self.heap_values_strictly_equal(a, b)
    }

    #[inline(always)]
    pub(super) fn truthy(&self, v: Value) -> bool {
        v.truthiness().unwrap_or_else(|| self.heap_cell_truthy(v))
    }

    /// ToBoolean of a heap value `Value::truthiness` leaves to its cell: one cell read
    /// decides every falsy heap value, "", 0n and the IsHTMLDDA object.
    #[inline(never)]
    pub(super) fn heap_cell_truthy(&self, v: Value) -> bool {
        heap_cell_truthy_in(&self.heap, v)
    }

    pub(super) fn is_html_dda(&self, value: Value) -> bool {
        matches!(
            self.heap.function(value),
            Some(FunctionRecord {
                kind: FunctionKind::Native(Native::IsHTMLDDA),
                ..
            })
        )
    }
}

/// The index in `bytecode::TYPEOF_NAMES` of the name `typeof` yields for a value of `kind`.
#[inline] pub(crate) fn typeof_name_index(kind: crate::ValueType) -> usize {
    use crate::ValueType as T;
    match kind {
        T::Undefined => 0,
        T::Null | T::Object => 1,
        T::Boolean => 2,
        T::Number => 3,
        T::String => 4,
        T::BigInt => 5,
        T::Symbol => 6,
        T::Function => 7,
    }
}

/// The text `typeof` yields for a value of `kind`.
pub(super) fn typeof_text(kind: crate::ValueType) -> &'static str {
    crate::bytecode::TYPEOF_NAMES[typeof_name_index(kind)]
}

/// ToString where the value alone decides it: `undefined` (the deleted marker reads as it),
/// `null`, a Boolean or a Number. `None` for a heap value, whose conversion reads its cell and
/// may run guest code. A Number's digits are written straight into the string.
pub(super) fn immediate_to_string(value: Value) -> Option<JsString> {
    match value.as_bool() {
        _ if value.is_undefined() || value.is_deleted() => Some(JsString::from_str("undefined")),
        _ if value.is_null() => Some(JsString::from_str("null")),
        Some(value) => Some(JsString::from_str(if value { "true" } else { "false" })),
        None => value.as_number().map(JsString::from_number),
    }
}

/// The index in `bytecode::TYPEOF_NAMES` of `typeof value` for a value outside the heap,
/// which its tag alone decides (`staged::value::immediate_typeof_index`, which the tiers run as
/// well); `None` for a heap value. None of these kinds is the `[[IsHTMLDDA]]` object's.
#[inline(always)]
pub(super) fn immediate_typeof_index(value: Value) -> Option<usize> {
    crate::staged::value::concrete_immediate_typeof_index(value)
}

/// `Vm::heap_cell_truthy` from the heap alone, so a lane frame (which names no host) decides a
/// string's or another cell's ToBoolean in place: a string's length from its record, a BigInt's
/// zero, the IsHTMLDDA object, and every other cell truthy.
#[inline(never)]
pub(crate) fn heap_cell_truthy_in(heap: &crate::heap::Heap, v: Value) -> bool {
    if let Some(length) = heap.string_record_base().and_then(|records| crate::staged::string_length::concrete_string_length(records, v)) {
        return length != Value::integer(0);
    }
    if let Some(text) = heap.string(v) {
        return text.len() != 0;
    }
    match heap.get(v) {
        Some(Cell::BigInt(value)) => !value.is_zero(),
        _ => !matches!(heap.function(v), Some(FunctionRecord { kind: FunctionKind::Native(Native::IsHTMLDDA), .. })),
    }
}
