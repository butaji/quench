use super::*;
use crate::{HostExecutionContext, HostFunctionId, NativeContext};

// Native environments have no bytecode-function binding metadata.
const NATIVE_ENVIRONMENT_FUNCTION: u32 = u32::MAX;

#[repr(usize)]
enum HostEnvironmentSlot {
    Operation,
    Data,
    Constructible,
}

impl HostEnvironmentSlot {
    const COUNT: usize = Self::Data as usize + 1;
    const CONSTRUCTOR_COUNT: usize = Self::Constructible as usize + 1;
}

/// Arguments a host call roots without allocating their list: all but 12 of the 906 host
/// operations `quench-node` declares take at most this many.
const HOST_CALL_INLINE_ARGUMENTS: usize = 6;

impl<H: Host> Vm<H> {
    /// A native function's private environment: traced slots outside any scope chain.
    pub(super) fn native_environment(&mut self, slots: Vec<Value>) -> Value {
        self.heap.alloc(Cell::Environment {
            parent: Value::NULL,
            function: NATIVE_ENVIRONMENT_FUNCTION,
            slots: slots.into(),
            scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: None,
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: Vec::new().into(),
                with_objects: Box::default(),
            }),
        })
    }

    pub(crate) fn create_host_function(&mut self, id: HostFunctionId) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, None, false, None)
    }

    pub(crate) fn create_host_constructor(&mut self, id: HostFunctionId) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, None, true, None)
    }

    pub(crate) fn create_host_constructor_with_data(
        &mut self,
        id: HostFunctionId,
        data: Value,
    ) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, Some(data), true, None)
    }

    pub(crate) fn create_host_function_with_data(
        &mut self,
        id: HostFunctionId,
        data: Value,
    ) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, Some(data), false, None)
    }

    pub(crate) fn create_named_host_constructor(
        &mut self,
        id: HostFunctionId,
        name: &str,
    ) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, None, true, Some(name))
    }

    pub(crate) fn create_named_host_constructor_with_data(
        &mut self,
        id: HostFunctionId,
        name: &str,
        data: Value,
    ) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, Some(data), true, Some(name))
    }

    pub(crate) fn create_named_host_function(
        &mut self,
        id: HostFunctionId,
        name: &str,
    ) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, None, false, Some(name))
    }

    pub(crate) fn create_named_host_function_with_data(
        &mut self,
        id: HostFunctionId,
        name: &str,
        data: Value,
    ) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, Some(data), false, Some(name))
    }

    fn create_host_function_with_capture(
        &mut self,
        id: HostFunctionId,
        data: Option<Value>,
        constructible: bool,
        name_override: Option<&str>,
    ) -> Result<Value, JsError> {
        self.embedding_program()?;
        let (name, length) = self
            .host
            .functions()
            .get(id.0 as usize)
            .map(|binding| (binding.name, binding.length))
            .ok_or_else(|| JsError("unknown host function index".into()))?;
        let name = name_override.unwrap_or(name);
        // Plain callbacks need only the exact numeric operation index. A captured
        // callback adds ordinary traced environment slots, not a side registry.
        let environment = match (data, constructible) {
            (None, false) => Value::number(f64::from(id.0)),
            (data, constructible) => {
                let count = if constructible {
                    HostEnvironmentSlot::CONSTRUCTOR_COUNT
                } else {
                    HostEnvironmentSlot::COUNT
                };
                let mut slots = vec![Value::UNDEFINED; count];
                slots[HostEnvironmentSlot::Operation as usize] = Value::number(f64::from(id.0));
                slots[HostEnvironmentSlot::Data as usize] = data.unwrap_or(Value::UNDEFINED);
                if constructible {
                    slots[HostEnvironmentSlot::Constructible as usize] = Value::TRUE;
                }
                self.native_environment(slots)
            }
        };
        let function = self.native_with_env(Native::HostFunction, environment);
        // A fresh function holds neither key and no cache can name it yet, so its `length`
        // and `name` are plain shape additions.
        let length_key = PropertyKey::string(self.length_atom);
        self.create_shape_data_property(
            function,
            length_key,
            Value::number(f64::from(length)),
            FUNCTION_INFO_ATTRIBUTES,
        )?;
        let name_key = PropertyKey::string(self.intern_atom("name"));
        let name_atom = self.intern_atom(name);
        let name_value = self.heap.alloc_string(self.atom_value(name_atom));
        self.create_shape_data_property(function, name_key, name_value, FUNCTION_INFO_ATTRIBUTES)?;
        self.capture_function_name_for_atom(function, name_atom);
        if constructible {
            let prototype = self
                .heap
                .alloc(Cell::Object(Self::empty_object(self.object_proto)));
            let constructor_atom = self.runtime_atoms.constructor;
            self.set_property_with_attributes(
                prototype,
                constructor_atom,
                function,
                PropertyAttributes {
                    enumerable: false,
                    ..DEFAULT_PROPERTY_ATTRIBUTES
                },
            )?;
            let prototype_atom = self.prototype_atom();
            self.set_property_with_attributes(
                function,
                prototype_atom,
                prototype,
                PropertyAttributes {
                    enumerable: false,
                    configurable: false,
                    ..DEFAULT_PROPERTY_ATTRIBUTES
                },
            )?;
        }
        Ok(function)
    }

    pub(super) fn host_function_constructible(&self, environment: Value) -> bool {
        self.heap
            .environment_slot(environment, HostEnvironmentSlot::Constructible as usize)
            .and_then(Value::as_bool)
            == Some(true)
    }

    pub(crate) fn host_function_data(&self) -> Result<Value, JsError> {
        let environment = self
            .active_native_env()
            .ok_or_else(|| JsError("host data requires an active native callback".into()))?;
        if environment.as_number().is_some() {
            return Ok(Value::UNDEFINED);
        }
        self.heap
            .environment_slot(environment, HostEnvironmentSlot::Data as usize)
            .ok_or_else(|| JsError("invalid host function data environment".into()))
    }

    pub(super) fn promise_job_boundary(
        &mut self,
        resource: Option<HostExecutionContext>,
        before: bool,
    ) -> Result<(), JsError> {
        let Some(resource) = resource else {
            return Ok(());
        };
        if !self.host.observes_promise_job_boundaries() {
            return Ok(());
        }
        let mut context = NativeContext::new(self);
        H::promise_job_boundary(&mut context, resource, before)
            .map_err(|error| context.finish_error(error))
    }

    pub(super) fn initialize_host(&mut self) -> Result<(), JsError> {
        for index in 0..self.host.functions().len() {
            let Some(name) = self.host.functions()[index].global else {
                continue;
            };
            let id = u32::try_from(index)
                .map_err(|_| JsError("host function table exceeds index space".into()))?;
            let function = self.create_host_function(HostFunctionId(id))?;
            // A host's globals are its platform's operations (timers, `fetch`,
            // `structuredClone`), enumerable as WebIDL makes a global's operations and as
            // Node defines them, unlike the language's own non-enumerable builtins.
            let atom = self.intern_atom(name);
            self.set_property_with_attributes(
                self.realm.globals,
                atom,
                function,
                PropertyAttributes {
                    writable: true,
                    enumerable: true,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            )?;
        }
        let mut context = NativeContext::new(self);
        H::initialize(&mut context).map_err(|error| context.finish_error(error))
    }

    /// The rest of the host's installation in a restored VM (`Host::resume`).
    pub(super) fn resume_host(&mut self) -> Result<(), JsError> {
        let mut context = NativeContext::new(self);
        H::resume(&mut context).map_err(|error| context.finish_error(error))
    }

    pub(super) fn initialize_host_shadow_realm(
        &mut self,
        global: crate::RootId,
    ) -> Result<Option<Value>, JsError> {
        let mut context = NativeContext::new(self);
        let callback = H::initialize_shadow_realm(&mut context, global)
            .map_err(|error| context.finish_error(error))?;
        callback
            .map(|root| {
                context
                    .rooted_value(root)
                    .ok_or_else(|| JsError("Host realm formatter root expired".into()))
            })
            .transpose()
    }

    pub(super) fn call_host_function(
        &mut self,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let result = self.run_host_function(receiver, args);
        // Host operations read strings as text, which they may make on demand.
        self.heap.charge_materialized_text();
        result
    }

    /// The host operation index a host function's `environment` names (`create_host_function`):
    /// the number itself, or its `HostEnvironmentSlot::Operation` slot.
    pub(super) fn host_operation_index(&self, environment: Value) -> Option<u32> {
        environment
            .as_number()
            .or_else(|| {
                self.heap
                    .environment_slot(environment, HostEnvironmentSlot::Operation as usize)
                    .and_then(Value::as_number)
            })
            .filter(|value| {
                value.is_finite()
                    && value.fract() == 0.0
                    && *value >= 0.0
                    && *value <= f64::from(u32::MAX)
            })
            .map(|value| value as u32)
    }

    /// The declared name of host function `function`, for diagnostics.
    #[cfg(feature = "profile-lane-exits")]
    pub(super) fn host_function_name(&self, function: Value) -> Option<&'static str> {
        let index = self.host_operation_index(self.heap.function(function)?.env)?;
        self.host
            .functions()
            .get(index as usize)
            .map(|binding| binding.name)
    }

    fn run_host_function(&mut self, receiver: Value, args: &[Value]) -> Result<Value, JsError> {
        let index = self
            .active_native_env()
            .and_then(|environment| self.host_operation_index(environment))
            .ok_or_else(|| JsError("invalid host function environment".into()))?;
        let callback = self
            .host
            .functions()
            .get(index as usize)
            .map(|binding| binding.call)
            .ok_or_else(|| JsError("unknown host function index".into()))?;
        let callee = self.active_native_callable();
        let new_target = self
            .construct_target
            .filter(|target| Some(target.callee) == callee)
            .map(|target| target.new_target);
        let mut context = NativeContext::with_new_target(self, new_target);
        let receiver = context.scoped_value(receiver);
        let arguments = args
            .iter()
            .map(|value| context.scoped_value(*value))
            .collect::<smallvec::SmallVec<[_; HOST_CALL_INLINE_ARGUMENTS]>>();
        let result = callback(&mut context, receiver, &arguments);
        context.finish(result)
    }
}
