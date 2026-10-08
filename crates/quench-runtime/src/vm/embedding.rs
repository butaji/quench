use super::*;

impl<H: Host> Vm<H> {
    pub(crate) fn embedding_is_callable(&self, root: RootId) -> Result<bool, JsError> {
        Ok(self.is_function(self.embedding_value(root)?))
    }

    pub(crate) fn embedding_is_regexp(&mut self, root: RootId) -> Result<bool, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        self.regexp_is_regexp(&program, value)
    }

    pub(crate) fn embedding_string_text(&self, root: RootId) -> Result<Option<String>, JsError> {
        let value = self.embedding_value(root)?;
        Ok(match self.heap.get(value) {
            Some(Cell::String(text)) => Some(text.host_string().to_owned()),
            _ => None,
        })
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

    pub(crate) fn create_embedding_object(&mut self) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        self.call_object_native(&program, Native::ObjectCreate, &[self.object_proto])
    }

    pub(crate) fn create_embedding_null_object(&mut self) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        self.call_object_native(&program, Native::ObjectCreate, &[Value::NULL])
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

    pub(crate) fn embedding_arguments(&self, args: &[RootId]) -> Result<Vec<Value>, JsError> {
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
}
