//! The host's side of the class, private-name and function-naming call-out rows (`staged::protocol_classes`): each step runs
//! the interpreter's own helper for its opcode.
use super::*;
use crate::staged::protocol_classes::ClassStep;

impl Vm {
    /// The class, private-name and function-naming `step` of its operand values (`first` is the first operand register's
    /// value, `second` the second's): the value it defines, if any.
    pub(super) fn class_step(&mut self, p: &ResidualProgram, _site: super::call_out::CallSite, step: ClassStep, first: Option<Value>, second: Option<Value>)
        -> Result<Option<Value>, JsError> {
        match step {
            ClassStep::DefineField { atom, site } => {
                let object = second.expect("a field definition names its object");
                let value = first.expect("a field definition names its value");
                let frame = self.own_frame();
                let function = self.frames[frame].function;
                self.define_field_cached(p, object, atom, value, site, function)?;
                Ok(None)
            }
            ClassStep::DefineComputedField { key } => {
                let object = second.expect("a computed field definition names its object");
                let value = first.expect("a computed field definition names its value");
                let frame = self.own_frame();
                let key = self.owner_register(VarOwner::Frame(frame), key);
                let key = self.to_property_key(p, key)?;
                let key = match self.heap.get(key).cloned() {
                    _ if let Some(atom) = self.string_key_atom(key) => PropertyKey::string(atom),
                    _ if self.heap.is_symbol(key) => PropertyKey::symbol(key),
                    _ => return Err(JsError::validation("invalid class field key".into())),
                };
                self.define_class_field(p, object, key, value)?;
                Ok(None)
            }
            ClassStep::DefinePrivateField { atom, home } => {
                let target = second.expect("a private field definition names its target");
                let value = first.expect("a private field definition names its value");
                let frame = self.own_frame();
                let home = self.owner_register(VarOwner::Frame(frame), home);
                self.initialize_private_field(p, target, home, atom, value)?;
                Ok(None)
            }
            ClassStep::MarkPrivateName { atom } => {
                let object = first.expect("a private name mark names its object");
                let home = second.expect("a private name mark names its home");
                self.mark_private_name(p, object, home, atom)?;
                Ok(None)
            }
            ClassStep::CheckPrivate { atom } => {
                let object = first.expect("a private check names its object");
                // A private field found under its home's brand proves the brand check.
                if self.private_field_value(p, object, atom).is_some() {
                    return Ok(None);
                }
                self.check_private_brand(p, object, atom)?;
                if self.private_field_value(p, object, atom).is_none()
                    && self.own_property(object, atom).is_none()
                    && self.property_accessor(object, atom).is_none()
                {
                    return Err(self.type_error(p, "private member is not present on this object".into()));
                }
                Ok(None)
            }
            ClassStep::PrivateIn { atom } => {
                let object = second.expect("a private `in` names its object");
                if !self.is_object_like(object) {
                    return Err(self.type_error(p, "right-hand side of 'in' is not an object".into()));
                }
                Ok(Some(Value::boolean(self.has_private_brand(p, object, atom))))
            }
            ClassStep::SetFunctionName { atom, site } => {
                let function = first.expect("a function naming names its function");
                self.set_function_name(p, function, (atom, site))?;
                Ok(None)
            }
            ClassStep::SetFunctionNameKey { prefix } => {
                let function = first.expect("a function naming names its function");
                let key = second.expect("a function naming by key names its key");
                self.set_function_name_key(function, key, prefix);
                Ok(None)
            }
            ClassStep::SetFunctionDebugName { atom } => {
                let function = first.expect("a debug naming names its function");
                self.set_function_debug_name(function, atom);
                Ok(None)
            }
            ClassStep::ValidateClassHeritage => {
                let heritage = first.expect("a class heritage check names its heritage");
                if !heritage.is_null() && !self.is_constructable(heritage) {
                    return Err(self.type_error(p, "Class extends value is not a constructor or null".into()));
                }
                Ok(None)
            }
            ClassStep::SuperCallCheck => {
                self.check_super_call(p)?;
                Ok(None)
            }
            ClassStep::InitializeThis => {
                let value = first.expect("a this initialization names its value");
                let frame = self.own_frame();
                self.initialize_this_binding(p, frame, value)?;
                Ok(None)
            }
            ClassStep::LoadCallee => {
                let frame = self.own_frame();
                Ok(Some(self.frames[frame].context.callee().unwrap_or(Value::UNDEFINED)))
            }
            ClassStep::LoadImportMeta => {
                let frame = self.own_frame();
                let program = self.frames[frame].program;
                self.load_import_meta(program).map(Some)
            }
        }
    }

    /// The import-meta object of `program` (`LoadImportMeta`): made on its first load, with the `url` of its source, and
    /// the `filename` and `dirname` the host gives it, then initialized by the host when it asks.
    pub(super) fn load_import_meta(&mut self, program: ProgramId) -> Result<Value, JsError> {
        if let Some(value) = self.programs.import_meta(program) {
            return Ok(value);
        }
        let value = self.heap.alloc_object(Self::empty_object(Value::NULL));
        if let Some(source_name) = self.programs.source_name(program).map(str::to_owned) {
            let url = self.host.import_meta_url(&source_name);
            let url = self.heap.alloc_string(url.into());
            let url_atom = self.intern_atom("url");
            self.set_property(value, url_atom, url)?;
            if let Some(filename) = self.host.import_meta_filename(&source_name) {
                let filename_value = self.heap.alloc_string(filename.as_str().into());
                let filename_atom = self.intern_atom("filename");
                self.set_property(value, filename_atom, filename_value)?;
                let dirname = std::path::Path::new(&filename)
                    .parent()
                    .and_then(|path| path.to_str());
                if let Some(dirname) = dirname {
                    let dirname_value = self.heap.alloc_string(dirname.into());
                    let dirname_atom = self.intern_atom("dirname");
                    self.set_property(value, dirname_atom, dirname_value)?;
                }
            }
            if self.host.initializes_import_meta() {
                let mut context = crate::NativeContext::new(self);
                let import_meta = context.scoped_value(value);
                (context.host_statics().initialize_import_meta)(&mut context, &source_name, import_meta)
                    .map_err(|error| context.finish_error(error))?;
            }
        }
        self.programs.set_import_meta(program, value);
        Ok(value)
    }
}
