use super::wtf16::JsString;
use super::*;
use oxc_ast::ast::UnaryOperator;

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
pub(super) fn numeric_unary(op: u32) -> bool {
    matches!(op, UNARY_PLUS | UNARY_NEGATION | BITWISE_NOT)
}

impl<H: Host> Vm<H> {
    /// The ECMAScript language type of `value`, with callable objects distinguished.
    pub(crate) fn value_type(&self, value: Value) -> crate::ValueType {
        use crate::ValueType as T;
        if value.is_undefined() || value.is_deleted() {
            T::Undefined
        } else if value.is_null() {
            T::Null
        } else if value.as_bool().is_some() {
            T::Boolean
        } else if value.as_number().is_some() {
            T::Number
        } else {
            match self.heap.get(value) {
                _ if self.heap.is_string(value) => T::String,
                Some(Cell::BigInt(_)) => T::BigInt,
                Some(Cell::Symbol(_)) => T::Symbol,
                _ if self.is_function(value) => T::Function,
                _ => T::Object,
            }
        }
    }

    pub(super) fn typeof_value(&mut self, value: Value) -> Value {
        let kind = self.typeof_kind(value);
        self.typeof_name(kind)
    }

    /// The kind whose name `typeof value` yields: an `[[IsHTMLDDA]]` object reads as
    /// undefined (ECMA-262 B.3.6.3).
    #[inline]
    fn typeof_kind(&self, value: Value) -> crate::ValueType {
        if self.is_html_dda(value) {
            crate::ValueType::Undefined
        } else {
            self.value_type(value)
        }
    }

    /// The integer a `UNARY_TYPEOF_INDEX` operator yields for `value`: the index in
    /// `bytecode::TYPEOF_NAMES` of the name `typeof value` is.
    #[inline]
    pub(super) fn typeof_index_value(&self, value: Value) -> Value {
        Value::integer(typeof_name_index(self.typeof_kind(value)) as i32)
    }

    /// The boolean a `UNARY_TYPEOF_IS` operator testing the name at `index` in
    /// `bytecode::TYPEOF_NAMES` yields for `value`.
    #[inline]
    pub(super) fn typeof_test_value(&self, value: Value, index: usize, negated: bool) -> Value {
        if (typeof_name_index(self.typeof_kind(value)) == index) != negated {
            Value::TRUE
        } else {
            Value::FALSE
        }
    }

    /// The one string `typeof` yields for values of `kind`: every result and every string
    /// constant spelling one (`Vm::materialize_constant`) is this value, so `===` between two
    /// of them is identity (`Vm::is_typeof_name`).
    fn typeof_name(&mut self, kind: crate::ValueType) -> Value {
        // `null` and objects share one name, kept in the object kind's slot.
        let kind = if kind == crate::ValueType::Null {
            crate::ValueType::Object
        } else {
            kind
        };
        let cached = self.typeof_names[kind as usize];
        if !cached.is_undefined() {
            return cached;
        }
        let name = self.intern_dynamic_ascii(typeof_text(kind));
        self.typeof_names[kind as usize] = name;
        name
    }

    /// The `typeof` name spelled by `text`, if it spells one (`Vm::typeof_name`).
    pub(super) fn typeof_constant(&mut self, text: &str) -> Option<Value> {
        TYPEOF_KINDS
            .into_iter()
            .find(|kind| typeof_text(*kind) == text)
            .map(|kind| self.typeof_name(kind))
    }

    /// Whether `value` is one of the `typeof` names (`Vm::typeof_name`).
    #[inline(always)]
    pub(super) fn is_typeof_name(&self, value: Value) -> bool {
        self.typeof_names.contains(&value)
    }

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
            Some(Cell::BigInt(value)) => Ok(value.as_ref().clone()),
            _ if let Some(value) = self.heap.string(primitive) => {
                let Some(value) = crate::bigint::parse_string(&value.host_string()) else {
                    return self
                        .syntax_error_result(p, "invalid BigInt value")
                        .map(|_| num_bigint::BigInt::default());
                };
                Ok(value)
            }
            _ => primitive
                .as_bool()
                .map(|value| num_bigint::BigInt::from(i32::from(value)))
                .ok_or_else(|| self.type_error(p, "cannot convert value to BigInt".into())),
        }
    }

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
        if self.heap.is_string(primitive)
            || matches!(self.heap.get(primitive), Some(Cell::Symbol(_)))
        {
            return Ok(primitive);
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
        self.to_string(program, primitive).map(JsString::from)
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
                let negated = -value.as_ref();
                return Ok(self.heap.alloc(Cell::bigint(negated)));
            }
            if op == BITWISE_NOT {
                let complement = !value.as_ref();
                return Ok(self.heap.alloc(Cell::bigint(complement)));
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
            crate::bytecode::UNARY_TO_STRING => {
                let text = self.coerce_js_string(p, value)?;
                self.string_value_from(value, &text)
            }
            _ => return Err(JsError("unsupported unary operator".into())),
        })
    }

    #[expect(clippy::wrong_self_convention)]
    /// ToNumber of `value` where it runs no guest code and raises nothing: an immediate
    /// primitive or a string. `None` for any other value.
    pub(super) fn primitive_to_number(&self, value: Value) -> Option<f64> {
        value.immediate_number().or_else(|| {
            self.heap
                .string(value)
                .map(|text| super::number::parse_number_string(&text.host_string()))
        })
    }

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
            Some(Cell::Symbol(_)) => {
                return Err(
                    self.type_error(program, "Cannot convert a Symbol value to a number".into())
                );
            }
            Some(Cell::BigInt(_)) => {
                return Err(self.type_error(program, BIGINT_TO_NUMBER.into()));
            }
            _ => {}
        }
        if let Some(atom) = self.lookup_atom("valueOf") {
            let function = self.get_property(program, value, atom)?;
            if !function.is_undefined() {
                let result = self.call_value(program, function, value, &[])?;
                return self.to_number(program, result);
            }
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
            return Ok(text);
        }
        if self.object_data(value).is_some() {
            let primitive = self.to_primitive(program, value, "string")?;
            return self.to_string(program, primitive);
        }
        match self.heap.get(value) {
            Some(Cell::Function(_)) => return Ok("function () { [native code] }".into()),
            _ if let Some(value) = self.heap.string(value) => return Ok(value.to_host_owned()),
            Some(Cell::Error(value)) => return Ok(value.clone()),
            Some(Cell::BigInt(value)) => return Ok(value.to_string()),
            Some(Cell::Symbol(_)) => {
                return Err(
                    self.type_error(program, "Cannot convert a Symbol value to a string".into())
                );
            }
            Some(Cell::Date { milliseconds, .. }) => return Ok(milliseconds.to_string()),
            _ => {}
        }
        if let Some(atom) = self.lookup_atom("toString") {
            let function = self.get_property(program, value, atom)?;
            if self.is_function(function) {
                let result = self.call_value(program, function, value, &[])?;
                return self.to_string(program, result);
            }
        }
        if let Some(atom) = self.lookup_atom("valueOf") {
            let function = self.get_property(program, value, atom)?;
            if self.is_function(function) {
                let result = self.call_value(program, function, value, &[])?;
                if !result.is_heap() || self.heap.is_string(result) {
                    return self.to_string(program, result);
                }
            }
        }
        Ok("[object Object]".into())
    }

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
        if a == b {
            return true;
        }
        if let (Some(a), Some(b)) = (self.heap.string(a), self.heap.string(b)) {
            return a == b;
        }
        match (self.heap.get(a), self.heap.get(b)) {
            (Some(Cell::BigInt(a)), Some(Cell::BigInt(b))) => a == b,
            _ => false,
        }
    }

    #[inline(always)]
    pub(super) fn truthy(&self, v: Value) -> bool {
        v.truthiness().unwrap_or_else(|| self.heap_cell_truthy(v))
    }

    /// ToBoolean of a heap value `Value::truthiness` leaves to its cell: one cell read
    /// decides every falsy heap value, "", 0n and the IsHTMLDDA object.
    #[inline(never)]
    pub(super) fn heap_cell_truthy(&self, v: Value) -> bool {
        match self.heap.get(v) {
            _ if let Some(text) = self.heap.string(v) => !text.units().is_empty(),
            Some(Cell::BigInt(value)) => value.sign() != num_bigint::Sign::NoSign,
            _ => !self.is_html_dda(v),
        }
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

/// The kinds whose `typeof` names differ: `null` reads as an object.
const TYPEOF_KINDS: [crate::ValueType; 8] = {
    use crate::ValueType as T;
    [
        T::Undefined,
        T::Object,
        T::Boolean,
        T::Number,
        T::String,
        T::BigInt,
        T::Symbol,
        T::Function,
    ]
};

/// The index in `bytecode::TYPEOF_NAMES` of the name `typeof` yields for a value of `kind`;
/// `TYPEOF_KINDS` lists the kinds in that order.
fn typeof_name_index(kind: crate::ValueType) -> usize {
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
fn typeof_text(kind: crate::ValueType) -> &'static str {
    crate::bytecode::TYPEOF_NAMES[typeof_name_index(kind)]
}

/// ToString where the value alone decides it: `undefined` (the deleted marker reads as it),
/// `null`, a Boolean or a Number. `None` for a heap value, whose conversion reads its cell and
/// may run guest code.
pub(super) fn immediate_to_string(value: Value) -> Option<String> {
    if value.is_undefined() || value.is_deleted() {
        return Some("undefined".into());
    }
    if value.is_null() {
        return Some("null".into());
    }
    if let Some(value) = value.as_bool() {
        return Some(value.to_string());
    }
    value.as_number().map(crate::number_to_string::format)
}
