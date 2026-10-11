use super::*;

const BOXED_MARKERS: &[(crate::BuiltinBrand, &str)] = &[
    (crate::BuiltinBrand::BooleanObject, "\0quench:boolean-value"),
    (crate::BuiltinBrand::NumberObject, "\0quench:number-value"),
    (crate::BuiltinBrand::StringObject, "\0quench:string-value"),
    (crate::BuiltinBrand::SymbolObject, "\0quench:symbol-value"),
    (crate::BuiltinBrand::BigIntObject, "\0quench:bigint-value"),
];
const TYPED_BRANDS: &[(crate::BuiltinBrand, TypedArrayKind)] = &[
    (crate::BuiltinBrand::Uint8Array, TypedArrayKind::Uint8),
    (
        crate::BuiltinBrand::Uint8ClampedArray,
        TypedArrayKind::Uint8Clamped,
    ),
    (crate::BuiltinBrand::Uint16Array, TypedArrayKind::Uint16),
    (crate::BuiltinBrand::Uint32Array, TypedArrayKind::Uint32),
    (crate::BuiltinBrand::Int8Array, TypedArrayKind::Int8),
    (crate::BuiltinBrand::Int16Array, TypedArrayKind::Int16),
    (crate::BuiltinBrand::Int32Array, TypedArrayKind::Int32),
    (crate::BuiltinBrand::BigInt64Array, TypedArrayKind::BigInt64),
    (
        crate::BuiltinBrand::BigUint64Array,
        TypedArrayKind::BigUint64,
    ),
    (crate::BuiltinBrand::Float16Array, TypedArrayKind::Float16),
    (crate::BuiltinBrand::Float32Array, TypedArrayKind::Float32),
    (crate::BuiltinBrand::Float64Array, TypedArrayKind::Float64),
];
fn typed_array_brand(kind: TypedArrayKind) -> crate::BuiltinBrand {
    TYPED_BRANDS
        .iter()
        .find(|(_, candidate)| *candidate == kind)
        .expect("typed kind")
        .0
}

impl<H: Host> Vm<H> {
    pub(crate) fn embedding_push_native_stack_frame(&mut self, name: &str) {
        self.native_stack_frames
            .push((self.frames.len(), name.to_owned()));
    }

    pub(crate) fn embedding_pop_native_stack_frame(&mut self) {
        self.native_stack_frames
            .pop()
            .expect("balanced native stack scope");
    }

    pub(crate) fn embedding_is_array(&mut self, root: RootId) -> Result<bool, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        self.is_array(&program, value)
    }

    pub(crate) fn embedding_prototype(&mut self, root: RootId) -> Result<Value, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        self.object_get_prototype_of(&program, value)
    }

    pub(crate) fn embedding_weak_map_get(
        &self,
        map: RootId,
        key: RootId,
    ) -> Result<Value, JsError> {
        let map = self.embedding_value(map)?;
        let key = self.embedding_value(key)?;
        let Some(Cell::WeakMap { entries, .. }) = self.heap.get(map) else {
            return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
        };
        Ok(entries.get(key).unwrap_or(Value::UNDEFINED))
    }

    pub(crate) fn embedding_own_keys(&mut self, root: RootId) -> Result<Vec<RootId>, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        let keys = self.object_own_key_values(&program, value)?;
        Ok(keys.into_iter().map(|key| self.root(key)).collect())
    }

    pub(crate) fn embedding_define_descriptor(
        &mut self,
        object: RootId,
        key: RootId,
        descriptor: RootId,
    ) -> Result<Value, JsError> {
        let args = [
            self.embedding_value(object)?,
            self.embedding_value(key)?,
            self.embedding_value(descriptor)?,
        ];
        let program = self.embedding_program()?;
        self.object_define_property(&program, &args)
    }

    pub(crate) fn embedding_own_descriptor(
        &mut self,
        root: RootId,
        key: RootId,
    ) -> Result<Value, JsError> {
        let value = self.embedding_value(root)?;
        let key = self.embedding_value(key)?;
        let program = self.embedding_program()?;
        self.object_get_own_property_descriptor(&program, &[value, key])
    }

    pub(crate) fn embedding_enumerable_own_keys(
        &mut self,
        root: RootId,
        include_symbols: bool,
        include_indices: bool,
    ) -> Result<Vec<RootId>, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        let keys = if !include_indices && !matches!(self.heap.get(value), Some(Cell::Proxy { .. }))
        {
            self.ordinary_own_key_values(value, false)
        } else {
            self.object_own_key_values(&program, value)?
        }
        .into_iter()
        .map(|key| self.root(key))
        .collect::<Vec<_>>();
        let mut retained = vec![false; keys.len()];
        let outcome = (|| {
            let mut enumerable = Vec::with_capacity(keys.len());
            for (index, key) in keys.iter().enumerate() {
                let property = self.root_value(*key).expect("own key root is live");
                if !include_indices {
                    if let Some(Cell::String(text)) = self.heap.get(property) {
                        if super::object_static::array_index(text.host_string()).is_some() {
                            continue;
                        }
                    }
                }
                if !include_symbols && matches!(self.heap.get(property), Some(Cell::Symbol(_))) {
                    continue;
                }
                let descriptor =
                    self.object_get_own_property_descriptor(&program, &[value, property])?;
                if !descriptor.is_undefined() && self.descriptor_flag(descriptor, "enumerable") {
                    enumerable.push(*key);
                    retained[index] = true;
                }
            }
            Ok(enumerable)
        })();
        for (key, retained) in keys.into_iter().zip(retained) {
            if outcome.is_err() || !retained {
                self.release_root(key);
            }
        }
        outcome
    }

    pub(crate) fn embedding_has_builtin_brand(
        &self,
        root: RootId,
        brand: crate::BuiltinBrand,
    ) -> Result<bool, JsError> {
        use crate::BuiltinBrand as B;
        let value = self.embedding_value(root)?;
        let cell = self.heap.get(value);
        Ok(match brand {
            B::Date => matches!(cell, Some(Cell::Date { .. })),
            B::RegExp => matches!(cell, Some(Cell::RegExp { .. })),
            B::NativeError => self.error_is_error(value) || matches!(cell, Some(Cell::Error(_))),
            B::Promise => self.realm.promise.records.contains_key(&value),
            B::Map => matches!(cell, Some(Cell::Map { .. })),
            B::Set => matches!(cell, Some(Cell::Set { .. })),
            B::WeakMap => matches!(cell, Some(Cell::WeakMap { .. })),
            B::WeakSet => matches!(cell, Some(Cell::WeakSet { .. })),
            B::Proxy => matches!(cell, Some(Cell::Proxy { .. })),
            B::ArrayBuffer => matches!(cell, Some(Cell::ArrayBuffer { shared: false, .. })),
            B::SharedArrayBuffer => matches!(cell, Some(Cell::ArrayBuffer { shared: true, .. })),
            B::AnyArrayBuffer => matches!(cell, Some(Cell::ArrayBuffer { .. })),
            B::DataView => matches!(cell, Some(Cell::DataView { .. })),
            B::TypedArray => matches!(cell, Some(Cell::TypedArray { .. })),
            B::ArrayBufferView => {
                matches!(cell, Some(Cell::TypedArray { .. } | Cell::DataView { .. }))
            }
            B::ArgumentsObject => self
                .object_data(value)
                .is_some_and(Object::is_arguments_object),
            B::AsyncFunction | B::GeneratorFunction => match cell {
                Some(Cell::Function {
                    kind:
                        FunctionKind::User(program, function)
                        | FunctionKind::NumericUser(program, function),
                    ..
                }) => self
                    .programs
                    .get(*program)
                    .and_then(|program| {
                        program.functions.get(*function as usize).map(|function| {
                            if brand == B::AsyncFunction {
                                function.is_async
                            } else {
                                function.is_generator
                            }
                        })
                    })
                    .unwrap_or(false),
                _ => false,
            },
            B::GeneratorObject => matches!(
                cell,
                Some(Cell::Iterator {
                    kind: IteratorKind::Generator | IteratorKind::AsyncGenerator,
                    ..
                })
            ),
            B::MapIterator => matches!(
                cell,
                Some(Cell::Iterator {
                    kind: IteratorKind::MapKeys
                        | IteratorKind::MapValues
                        | IteratorKind::MapEntries,
                    ..
                })
            ),
            B::SetIterator => matches!(
                cell,
                Some(Cell::Iterator {
                    kind: IteratorKind::SetValues | IteratorKind::SetEntries,
                    ..
                })
            ),
            B::ModuleNamespaceObject => {
                matches!(cell, Some(Cell::Object(object)) if object.is_module_namespace())
            }
            B::BooleanObject
            | B::NumberObject
            | B::StringObject
            | B::SymbolObject
            | B::BigIntObject
            | B::BoxedPrimitive => {
                if !matches!(cell, Some(Cell::Object(_))) {
                    false
                } else {
                    BOXED_MARKERS.iter().copied().any(|(candidate, marker)| {
                        (brand == B::BoxedPrimitive || brand == candidate)
                            && self
                                .lookup_atom(marker)
                                .and_then(|atom| self.own_property(value, atom))
                                .is_some()
                    })
                }
            }
            typed => {
                let kind = TYPED_BRANDS
                    .iter()
                    .find(|(brand, _)| *brand == typed)
                    .expect("typed brand")
                    .1;
                matches!(cell, Some(Cell::TypedArray { kind: actual, .. }) if *actual == kind)
            }
        })
    }

    pub(crate) fn embedding_strict_equal(
        &self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, JsError> {
        Ok(self.strict_equal(self.embedding_value(left)?, self.embedding_value(right)?))
    }

    pub(crate) fn embedding_builtin_data(
        &self,
        root: RootId,
    ) -> Result<Option<crate::BuiltinData<Value>>, JsError> {
        use crate::{BuiltinBrand as B, BuiltinData as D};
        let value = self.embedding_value(root)?;
        if self.realm.promise.records.contains_key(&value) {
            return Ok(Some(D::Opaque));
        }
        let data = match self.heap.get(value) {
            Some(Cell::Date { milliseconds, .. }) => D::Primitive {
                brand: B::Date,
                value: Value::number(*milliseconds),
            },
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                ..
            }) => D::Bytes {
                brand: if *shared {
                    B::SharedArrayBuffer
                } else {
                    B::ArrayBuffer
                },
                bytes: if *detached {
                    Vec::new()
                } else {
                    bytes.to_vec()
                },
            },
            Some(Cell::TypedArray { kind, .. }) => D::Bytes {
                brand: typed_array_brand(*kind),
                bytes: self.embedding_view_bytes(value).unwrap_or_default(),
            },
            Some(Cell::DataView { .. }) => D::Bytes {
                brand: B::DataView,
                bytes: self.embedding_view_bytes(value).unwrap_or_default(),
            },
            Some(Cell::Map { entries, .. }) => D::Map(entries.clone()),
            Some(Cell::Set { entries, .. }) => D::Set(entries.clone()),
            Some(Cell::RegExp { .. }) => D::RegExp,
            Some(Cell::WeakMap { .. } | Cell::WeakSet { .. }) => D::Opaque,
            _ if self.error_is_error(value)
                || matches!(self.heap.get(value), Some(Cell::Error(_))) =>
            {
                D::Error
            }
            Some(Cell::Object(_)) => {
                for &(brand, marker) in BOXED_MARKERS {
                    if let Some(payload) = self
                        .lookup_atom(marker)
                        .and_then(|atom| self.own_property(value, atom))
                    {
                        return Ok(Some(D::Primitive {
                            brand,
                            value: payload,
                        }));
                    }
                }
                return Ok(None);
            }
            _ => return Ok(None),
        };
        Ok(Some(data))
    }

    pub(crate) fn embedding_floating_view_values(
        &mut self,
        root: RootId,
    ) -> Result<Option<Vec<f64>>, JsError> {
        let value = self.embedding_value(root)?;
        if !matches!(
            self.heap.get(value),
            Some(Cell::TypedArray {
                kind: TypedArrayKind::Float16 | TypedArrayKind::Float32 | TypedArrayKind::Float64,
                ..
            })
        ) {
            return Ok(None);
        }
        let length = self.typed_array_length(value).unwrap_or(0);
        Ok(Some(
            (0..length)
                .map(|index| {
                    self.typed_array_get(value, index)
                        .and_then(Value::as_number)
                        .unwrap_or(f64::NAN)
                })
                .collect(),
        ))
    }

    pub(crate) fn embedding_is_callable(&self, root: RootId) -> Result<bool, JsError> {
        Ok(self.is_function(self.embedding_value(root)?))
    }

    pub(crate) fn embedding_is_object(&self, root: RootId) -> Result<bool, JsError> {
        let value = self.embedding_value(root)?;
        Ok(self.is_object_like(value) && !self.is_function(value))
    }

    pub(crate) fn embedding_string_units(&self, root: RootId) -> Result<Option<Vec<u16>>, JsError> {
        let value = self.embedding_value(root)?;
        Ok(match self.heap.get(value) {
            Some(Cell::String(text)) => Some(text.units().to_vec()),
            _ => None,
        })
    }

    pub(crate) fn embedding_string_text(&self, root: RootId) -> Result<Option<String>, JsError> {
        let value = self.embedding_value(root)?;
        Ok(match self.heap.get(value) {
            Some(Cell::String(text)) => Some(text.host_string().to_owned()),
            _ => None,
        })
    }

    pub(crate) fn embedding_detail_string(&self, root: RootId) -> Result<String, JsError> {
        let value = self.embedding_value(root)?;
        Ok(self.detail_string(value))
    }

    pub(crate) fn embedding_has_property(&mut self, object: RootId, key: RootId) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let key = self.embedding_value(key)?;
        let program = self.embedding_program()?;
        self.has_property(&program, object, key)
    }

    pub(crate) fn embedding_has_own_property(
        &self,
        root: RootId,
        name: &str,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(root)?;
        if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
            return Ok(false);
        }
        let Some(atom) = self.lookup_atom(name) else {
            return Ok(false);
        };
        Ok(self
            .property_attributes(object, super::property_key::PropertyKey::string(atom))
            .is_some())
    }

    pub(crate) fn embedding_is_error(&self, root: RootId) -> Result<bool, JsError> {
        Ok(self.error_is_error(self.embedding_value(root)?))
    }

    pub(super) fn detail_string(&self, value: Value) -> String {
        if let Some(number) = value.as_number() {
            return crate::number_to_string::format(number);
        }
        if let Some(boolean) = value.as_bool() {
            return boolean.to_string();
        }
        if value.is_null() {
            return "null".into();
        }
        if value.is_undefined() {
            return "undefined".into();
        }

        let mut value = value;
        while let Some(Cell::Proxy { target, .. }) = self.heap.get(value) {
            if target.is_null() || target.is_undefined() {
                return "[object Proxy]".into();
            }
            value = *target;
        }

        match self.heap.get(value) {
            Some(Cell::String(text)) => text.host_string().to_owned(),
            Some(Cell::BigInt(integer)) => {
                if Self::detail_bigint_is_too_large(integer) {
                    "<a very large BigInt>".into()
                } else {
                    integer.clone()
                }
            }
            Some(Cell::Symbol(description)) => Self::detail_symbol(description.as_deref()),
            Some(Cell::Error(message)) => format!("Error: {message}"),
            Some(Cell::Object(object)) if object.has_error_data() => {
                let name = self
                    .own_data_string(value, "name")
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "Error".into());
                let message = self.own_data_string(value, "message").unwrap_or_default();
                if message.is_empty() {
                    name
                } else {
                    format!("{name}: {message}")
                }
            }
            Some(Cell::Object(_)) => "#<Object>".into(),
            Some(Cell::Map { .. }) => "#<Map>".into(),
            Some(Cell::Set { .. }) => "#<Set>".into(),
            Some(cell) => format!("[object {}]", Self::diagnostic_class_name(cell)),
            None => "[object Unknown]".into(),
        }
    }

    fn detail_bigint_is_too_large(integer: &str) -> bool {
        const MAX_V8_DETAIL_BIGINT_LIMBS: usize = 100;

        let digits = integer
            .strip_prefix('-')
            .unwrap_or(integer)
            .trim_start_matches('0');
        if digits.is_empty() || digits.parse::<usize>().is_ok() {
            return false;
        }

        let mut limbs = vec![0usize];
        for byte in digits.bytes() {
            let Some(digit) = byte.checked_sub(b'0').filter(|digit| *digit <= 9) else {
                return false;
            };
            let mut carry = u128::from(digit);
            for limb in &mut limbs {
                let product = (*limb as u128) * 10 + carry;
                *limb = product as usize;
                carry = product >> usize::BITS;
            }
            if carry != 0 {
                limbs.push(carry as usize);
                if limbs.len() > MAX_V8_DETAIL_BIGINT_LIMBS {
                    return true;
                }
            }
        }
        limbs.len() > MAX_V8_DETAIL_BIGINT_LIMBS
    }

    fn detail_symbol(description: Option<&str>) -> String {
        const MAX_V8_DETAIL_SYMBOL_UNITS: usize = 128;
        const V8_DETAIL_SYMBOL_EDGE_UNITS: usize = 56;

        let Some(description) = description else {
            return "Symbol()".into();
        };
        if description.encode_utf16().count() <= MAX_V8_DETAIL_SYMBOL_UNITS {
            return format!("Symbol({description})");
        }
        let units: Vec<_> = description.encode_utf16().collect();
        let prefix = String::from_utf16_lossy(&units[..V8_DETAIL_SYMBOL_EDGE_UNITS]);
        let suffix = String::from_utf16_lossy(&units[units.len() - V8_DETAIL_SYMBOL_EDGE_UNITS..]);
        format!("Symbol({prefix}...<omitted>...{suffix})")
    }

    fn own_data_string(&self, object: Value, name: &str) -> Option<String> {
        let atom = self.lookup_atom(name)?;
        let data = self.object_data(object)?;
        let index = self.shape_slot(data.shape(), atom)?;
        if self
            .property_attributes(object, super::property_key::PropertyKey::string(atom))
            .is_some_and(|attributes| attributes.accessor)
        {
            return None;
        }
        let value = self.heap.property_get(data, index)?;
        match self.heap.get(value) {
            Some(Cell::String(text)) => Some(text.host_string().to_owned()),
            _ => None,
        }
    }

    fn diagnostic_class_name(cell: &Cell) -> &'static str {
        match cell {
            Cell::Array { .. } => "Array",
            Cell::ArrayBuffer { shared: true, .. } => "SharedArrayBuffer",
            Cell::ArrayBuffer { .. } => "ArrayBuffer",
            Cell::TypedArray { kind, .. } => match kind {
                TypedArrayKind::Uint8 => "Uint8Array",
                TypedArrayKind::Uint8Clamped => "Uint8ClampedArray",
                TypedArrayKind::Uint16 => "Uint16Array",
                TypedArrayKind::Uint32 => "Uint32Array",
                TypedArrayKind::Int8 => "Int8Array",
                TypedArrayKind::Int16 => "Int16Array",
                TypedArrayKind::Int32 => "Int32Array",
                TypedArrayKind::BigInt64 => "BigInt64Array",
                TypedArrayKind::BigUint64 => "BigUint64Array",
                TypedArrayKind::Float16 => "Float16Array",
                TypedArrayKind::Float32 => "Float32Array",
                TypedArrayKind::Float64 => "Float64Array",
            },
            Cell::DataView { .. } => "DataView",
            Cell::Map { .. } => "Map",
            Cell::Set { .. } => "Set",
            Cell::Date { .. } => "Date",
            Cell::RegExp { .. } => "RegExp",
            Cell::Function { .. } => "Function",
            Cell::Error(_) => "Error",
            _ => "Object",
        }
    }

    pub(crate) fn embedding_is_symbol(&self, root: RootId) -> Result<bool, JsError> {
        let value = self.embedding_value(root)?;
        Ok(matches!(self.heap.get(value), Some(Cell::Symbol(_))))
    }

    pub(crate) fn embedding_is_promise(&self, root: RootId) -> Result<bool, JsError> {
        self.embedding_has_builtin_brand(root, crate::BuiltinBrand::Promise)
    }

    pub(crate) fn embedding_to_string(&mut self, root: RootId) -> Result<String, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        self.to_string(&program, value)
    }

    pub(crate) fn embedding_greater_than(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, JsError> {
        let left = self.embedding_value(left)?;
        let right = self.embedding_value(right)?;
        let program = self.embedding_program()?;
        let result = self.binary(
            &program,
            super::operations::RelationalOperator::GreaterThan as u32,
            left,
            right,
        )?;
        Ok(self.truthy(result))
    }

    pub(crate) fn embedding_bitwise_or(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<Value, JsError> {
        let left = self.embedding_value(left)?;
        let right = self.embedding_value(right)?;
        let program = self.embedding_program()?;
        self.binary(
            &program,
            oxc_ast::ast::BinaryOperator::BitwiseOR as u32,
            left,
            right,
        )
    }

    pub(crate) fn embedding_iterator_record(
        &mut self,
        source: RootId,
    ) -> Result<(RootId, RootId), JsError> {
        let source = self.embedding_value(source)?;
        let program = self.embedding_program()?;
        let iterator = self.get_iterator(&program, source)?;
        let iterator = self.root(iterator);
        let value = self.embedding_value(iterator)?;
        let next = self.intern_atom("next");
        match self.get_property(&program, value, next) {
            Ok(next) => Ok((iterator, self.root(next))),
            Err(error) => {
                self.release_root(iterator);
                Err(error)
            }
        }
    }

    pub(crate) fn embedding_iterator_step(
        &mut self,
        iterator: RootId,
        next: RootId,
    ) -> Result<Option<Value>, JsError> {
        self.embedding_value(iterator)?;
        self.embedding_value(next)?;
        let program = self.embedding_program()?;
        self.rooted_iterator_step_value(&program, iterator, next)
    }

    pub(crate) fn embedding_iterator_close(&mut self, iterator: RootId) -> Result<Value, JsError> {
        let value = self.embedding_value(iterator)?;
        let program = self.embedding_program()?;
        self.iterator_close(&program, value)
    }

    pub(crate) fn embedding_truthy(&self, root: RootId) -> Result<bool, JsError> {
        Ok(self.truthy(self.embedding_value(root)?))
    }

    pub(crate) fn embedding_same_value(
        &self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, JsError> {
        Ok(self.same_value(self.embedding_value(left)?, self.embedding_value(right)?))
    }

    pub(crate) fn embedding_equal(&mut self, left: RootId, right: RootId) -> Result<bool, JsError> {
        let left = self.embedding_value(left)?;
        let right = self.embedding_value(right)?;
        let program = self.embedding_program()?;
        self.equal(&program, left, right)
    }

    pub(crate) fn evaluate_embedding_script(
        &mut self,
        source: &str,
        name: &str,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let previous = std::mem::replace(&mut self.direct_eval, false);
        let result = self.eval_global_script_named(&program, source, false, name);
        self.direct_eval = previous;
        result
    }

    pub(crate) fn parse_embedding_json(&mut self, source: &str) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let source = self.heap.alloc(Cell::String(source.into()));
        self.json_parse(&program, &[source])
    }

    pub(crate) fn create_embedding_array(&mut self, values: &[RootId]) -> Result<Value, JsError> {
        self.embedding_program()?;
        let elements = self.embedding_arguments(values)?;
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(elements),
        }))
    }

    pub(crate) fn create_embedding_exception(
        &mut self,
        kind: Native,
        message: &str,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let message = self.heap.alloc(Cell::String(message.into()));
        self.construct_error_native(&program, kind, &[message])
    }

    pub(crate) fn global_root(&mut self) -> Result<RootId, JsError> {
        self.embedding_program()?;
        Ok(self.root(self.realm.globals))
    }

    pub(crate) fn string_rooted(&mut self, text: &str) -> RootId {
        let value = self.heap.alloc(Cell::String(JsString::from_str(text)));
        self.root(value)
    }

    pub(crate) fn string_units_rooted(&mut self, units: &[u16]) -> RootId {
        let value = self.heap.alloc(Cell::String(JsString::from_units(units)));
        self.root(value)
    }

    pub(crate) fn create_embedding_object(&mut self) -> Result<Value, JsError> {
        self.create_embedding_object_with_prototype(self.object_proto)
    }

    pub(crate) fn create_embedding_null_object(&mut self) -> Result<Value, JsError> {
        self.create_embedding_object_with_prototype(Value::NULL)
    }

    pub(crate) fn embedding_symbol(&mut self, description: Option<String>) -> Value {
        self.heap.alloc(Cell::Symbol(description))
    }

    fn create_embedding_object_with_prototype(
        &mut self,
        prototype: Value,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        self.call_object_native(&program, Native::ObjectCreate, &[prototype])
    }

    pub(crate) fn embedding_value(&self, root: RootId) -> Result<Value, JsError> {
        self.root_value(root)
            .ok_or_else(|| JsError("released or foreign embedding root".into()))
    }

    pub(crate) fn embedding_program(&self) -> Result<Rc<ResidualProgram>, JsError> {
        self.programs
            .get(self.active_program)
            .ok_or_else(|| JsError("runtime is not initialized".into()))
    }

    pub(crate) fn get_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
    ) -> Result<Value, JsError> {
        let object = self.embedding_value(object)?;
        let key = self.embedding_value(key)?;
        let program = self.embedding_program()?;
        self.get_index(&program, object, key)
    }

    pub(crate) fn call_rooted(
        &mut self,
        callee: RootId,
        receiver: RootId,
        args: &[RootId],
    ) -> Result<Value, JsError> {
        let callee = self.embedding_value(callee)?;
        let receiver = self.embedding_value(receiver)?;
        let args = self.embedding_arguments(args)?;
        let program = self.embedding_program()?;
        self.call_value(&program, callee, receiver, &args)
    }

    pub(crate) fn embedding_enqueue_job(
        &mut self,
        callback: RootId,
        args: &[RootId],
    ) -> Result<(), JsError> {
        let callback = self.embedding_value(callback)?;
        let args = self.embedding_arguments(args)?;
        self.enqueue_job(callback, args);
        Ok(())
    }

    fn embedding_arguments(&self, args: &[RootId]) -> Result<Vec<Value>, JsError> {
        args.iter()
            .map(|root| self.embedding_value(*root))
            .collect()
    }

    pub(crate) fn construct_rooted(
        &mut self,
        callee: RootId,
        new_target: RootId,
        args: &[RootId],
    ) -> Result<Value, JsError> {
        let callee = self.embedding_value(callee)?;
        let new_target = self.embedding_value(new_target)?;
        let args = self.embedding_arguments(args)?;
        let program = self.embedding_program()?;
        self.construct_value_with_new_target(&program, callee, new_target, &args)
    }

    pub(crate) fn set_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        value: RootId,
        receiver: RootId,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let key = self.embedding_value(key)?;
        let value = self.embedding_value(value)?;
        let receiver = self.embedding_value(receiver)?;
        let program = self.embedding_program()?;
        let accepted = self.call_reflect_native(
            &program,
            Native::ReflectSet,
            &[object, key, value, receiver],
        )?;
        Ok(accepted.as_bool().expect("Reflect.set returns a boolean"))
    }

    pub(crate) fn define_data_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        value: RootId,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let key = self.embedding_value(key)?;
        let value = self.embedding_value(value)?;
        let program = self.embedding_program()?;
        let key = self.to_property_key(&program, key)?;
        self.define_own_property_record(
            &program,
            object,
            key,
            super::object_descriptors::PropertyDescriptorRecord {
                value: Some(value),
                writable: Some(writable),
                enumerable: Some(enumerable),
                configurable: Some(configurable),
                getter: None,
                setter: None,
            },
        )
    }

    pub(crate) fn set_prototype_rooted(
        &mut self,
        object: RootId,
        prototype: RootId,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let prototype = self.embedding_value(prototype)?;
        let program = self.embedding_program()?;
        self.set_prototype_of(&program, object, prototype)
    }
}
