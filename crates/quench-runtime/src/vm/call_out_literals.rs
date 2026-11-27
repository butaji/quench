//! The host's side of the literal, template, closure and object-construction call-out rows (`staged::protocol_literals`): each step runs
//! the interpreter's own helper for its opcode.
use super::object_descriptors::PropertyDescriptorRecord;
use super::*;
use crate::bytecode::{PropertyDefinitionMode, Register};
use crate::staged::protocol_literals::LiteralStep;

impl Vm {
    /// The literal, template, closure and object-construction `step` of its operand values (`first` is the first operand register's
    /// value, `second` the second's): the value it defines, if any.
    pub(super) fn literal_step(&mut self, p: &ResidualProgram, site: super::call_out::CallSite, step: LiteralStep, first: Option<Value>, second: Option<Value>)
        -> Result<Option<Value>, JsError> {
        match step {
            LiteralStep::DefineArrayElement { .. } | LiteralStep::CopyDataProperties { .. } | LiteralStep::DefinePropertyRecord { .. } =>
                self.literal_define(p, step, first, second).map(|()| None),
            LiteralStep::Object | LiteralStep::Array { .. } | LiteralStep::ConstArray { .. } | LiteralStep::ObjectPair { .. }
            | LiteralStep::ObjectLiteral { .. } => self.literal_object(p, step, first, second).map(Some),
            LiteralStep::LiteralTemplate { .. } | LiteralStep::RegExp { .. } | LiteralStep::CacheTemplate { .. }
            | LiteralStep::LoadTemplate { .. } => self.literal_site(p, step, first).map(Some),
            LiteralStep::Closure { function } => self.owner_closure(p, site.owner, function, site.resume).map(Some),
        }
    }

    /// The closure of function `function` that `MakeClosure` makes in the activation `owner` (a frame, or a lane activation's
    /// record) at the binding site resuming at `resume`: over the binding environment the owner captures there.
    pub fn owner_closure(&mut self, p: &ResidualProgram, owner: VarOwner, function: u32, resume: usize) -> Result<Value, JsError> {
        let env = self.at_binding_site(owner, resume, |vm| vm.capture_binding_environment(owner))?;
        self.closure_over_environment(p, owner, function, env)
    }

    /// The closure of function `function` over the binding environment `env` the activation `owner` captured (`MakeClosure`):
    /// a module root's closure is the cached one of that environment, when one is live.
    pub(super) fn closure_over_environment(&mut self, p: &ResidualProgram, owner: VarOwner, function: u32, env: Value)
        -> Result<Value, JsError> {
        let (program, running) = (self.owner_view(owner).program(), self.owner_view(owner).function());
        let module_root = p.is_module() && running == ROOT_FUNCTION_ID && self.programs.module_environment(program) == Some(env);
        // A module's hoisted declaration is the closure its instance made at instantiation
        // (`ModuleFunctions`), which a cyclic importer may already hold; the body's own
        // declaration of it is that closure, not a second one.
        let cached = if module_root {
            self.module_function(program, function)
                .filter(|closure| matches!(self.heap.function(*closure), Some(crate::heap::FunctionRecord { env: closure_env, .. }) if *closure_env == env))
                .or_else(|| self.cached_functions_in_environment(program, function, env).next())
        } else {
            None
        };
        match cached {
            Some(closure) => Ok(closure),
            None => self.closure(p, function, env),
        }
    }

    /// The object and array constructors of `step`: the value each makes.
    fn literal_object(&mut self, p: &ResidualProgram, step: LiteralStep, first: Option<Value>, second: Option<Value>)
        -> Result<Value, JsError> {
        match step {
            LiteralStep::Object => Ok(self.object()),
            LiteralStep::Array { length } => Ok(self.hole_array(length)),
            LiteralStep::ConstArray { constant, count } => {
                let frame = self.own_frame();
                let program = self.frames[frame].program;
                self.const_array_value(program, constant, usize::from(count))
                    .ok_or_else(|| JsError::validation("constant array is outside program".into()))
            }
            LiteralStep::ObjectPair { site } => {
                let first = first.expect("MakeObject2 names its first field");
                let second = second.expect("MakeObject2 names its second field");
                Ok(self.object_pair(p, site, first, second))
            }
            LiteralStep::ObjectLiteral { site, base, count } => Ok(self.object_literal(p, site, base, count)),
            _ => unreachable!("{step:?} is no object constructor step"),
        }
    }

    /// The object of the register window `base..base + count` of the current frame, built at `site`.
    fn object_literal(&mut self, p: &ResidualProgram, site: usize, base: Register, count: u16) -> Value {
        let frame = self.own_frame();
        let shape = self.object_site_shape(p, site);
        let start = usize::from(base);
        let properties = &self.frames[frame].registers[start..start + usize::from(count)];
        let prototype = self.object_proto;
        self.heap.alloc_object_with_properties(prototype, shape, properties)
    }

    /// The literal-site steps of `step`: a nested literal of constants, a RegExp literal and the template object cache.
    fn literal_site(&mut self, p: &ResidualProgram, step: LiteralStep, first: Option<Value>) -> Result<Value, JsError> {
        let frame = self.own_frame();
        match step {
            LiteralStep::LiteralTemplate { template } => self.literal_template(p, frame, template),
            LiteralStep::RegExp { site } => self.regexp_literal(p, frame, site),
            LiteralStep::CacheTemplate { site } => {
                let value = first.expect("CacheTemplateObject names its template object");
                Ok(self.cache_template_object(frame, site, value))
            }
            LiteralStep::LoadTemplate { site } => Ok(self.load_template_object(frame, site)),
            _ => unreachable!("{step:?} is no literal site step"),
        }
    }

    /// The template object `site` of the function running in `frame`: `value` if none is cached yet, else
    /// the cached one (`CacheTemplateObject`). No guest code runs.
    pub fn cache_template_object(&mut self, frame: usize, site: u32, value: Value) -> Value {
        let key = self.template_key(frame, site);
        *self.realm.template_objects.entry(key).or_insert(value)
    }

    /// The template object `site` cached for the function running in `frame`, or `undefined` before its
    /// first make (`LoadCachedTemplateObject`). No guest code runs.
    pub fn load_template_object(&self, frame: usize, site: u32) -> Value {
        let key = self.template_key(frame, site);
        self.realm.template_objects.get(&key).copied().unwrap_or(Value::UNDEFINED)
    }

    /// The key of the template object `site` of the function running in `frame`.
    fn template_key(&self, frame: usize, site: u32) -> (ProgramId, u32, u32) {
        (self.frames[frame].program, self.frames[frame].function, site)
    }

    /// The definitions of `step` into the object of its first operand: an array element, a copy of
    /// data properties and a property record.
    fn literal_define(&mut self, p: &ResidualProgram, step: LiteralStep, first: Option<Value>, second: Option<Value>)
        -> Result<(), JsError> {
        let target = first.expect("a definition names its target");
        let second = second.expect("a definition names its second operand");
        match step {
            LiteralStep::DefineArrayElement { index } => self.define_array_literal_element(p, target, index as usize, second),
            LiteralStep::CopyDataProperties { exclusions } => {
                let frame = self.own_frame();
                let exclusions = self.read(frame, exclusions);
                self.copy_data_properties(p, target, second, exclusions)
            }
            LiteralStep::DefinePropertyRecord { mode, value } => {
                let frame = self.own_frame();
                let value = self.read(frame, value);
                let mode = PropertyDefinitionMode::from_word(mode).expect("a decoded definition mode is a catalog mode");
                let descriptor = PropertyDescriptorRecord::for_definition(mode, value);
                self.define_property_or_throw(p, target, second, descriptor)
            }
            _ => unreachable!("{step:?} is no definition step"),
        }
    }
}
