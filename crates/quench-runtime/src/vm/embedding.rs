use super::*;

impl<H: Host> Vm<H> {
    pub(crate) fn embedding_is_callable(&self, root: RootId) -> Result<bool, JsError> {
        Ok(self.is_function(self.embedding_value(root)?))
    }

    pub(crate) fn embedding_is_object(&self, root: RootId) -> Result<bool, JsError> {
        let value = self.embedding_value(root)?;
        Ok(self.is_object_like(value) && !self.is_function(value))
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

    fn detail_string(&self, value: Value) -> String {
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
        let value = self.embedding_value(root)?;
        Ok(self.realm.promise.records.contains_key(&value))
    }

    pub(crate) fn embedding_to_string(&mut self, root: RootId) -> Result<String, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        self.to_string(&program, value)
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
        Ok(self.heap.alloc(Cell::array(
            self.array_proto,
            Rc::new(elements),
        )))
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
