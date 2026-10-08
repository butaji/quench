use super::*;
use crate::{HostFunctionId, NativeContext};

// Native environments have no bytecode-function binding metadata.
const HOST_ENVIRONMENT_FUNCTION: u32 = u32::MAX;

#[repr(usize)]
enum HostEnvironmentSlot {
    Operation,
    Data,
}

impl HostEnvironmentSlot {
    const COUNT: usize = Self::Data as usize + 1;
}

impl<H: Host> Vm<H> {
    pub(crate) fn create_host_function(&mut self, id: HostFunctionId) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, None)
    }

    pub(crate) fn create_host_function_with_data(
        &mut self,
        id: HostFunctionId,
        data: Value,
    ) -> Result<Value, JsError> {
        self.create_host_function_with_capture(id, Some(data))
    }

    fn create_host_function_with_capture(
        &mut self,
        id: HostFunctionId,
        data: Option<Value>,
    ) -> Result<Value, JsError> {
        self.embedding_program()?;
        let (name, length) = self
            .host
            .functions()
            .get(id.0 as usize)
            .map(|binding| (binding.name, binding.length))
            .ok_or_else(|| JsError("unknown host function index".into()))?;
        // Plain callbacks need only the exact numeric operation index. A captured
        // callback adds ordinary traced environment slots, not a side registry.
        let environment = match data {
            None => Value::number(f64::from(id.0)),
            Some(data) => {
                let mut slots = vec![Value::UNDEFINED; HostEnvironmentSlot::COUNT];
                slots[HostEnvironmentSlot::Operation as usize] = Value::number(f64::from(id.0));
                slots[HostEnvironmentSlot::Data as usize] = data;
                self.heap.alloc(Cell::Environment {
                    parent: Value::NULL,
                    program: None,
                    root_eval_scope: false,
                    binding_site_pc: None,
                    function: HOST_ENVIRONMENT_FUNCTION,
                    slots: slots.into_boxed_slice().into(),
                    dynamic_bindings: Vec::new().into(),
                    with_objects: Vec::new(),
                })
            }
        };
        let function = self.native_with_env(Native::HostFunction, environment);
        self.set_builtin_function_name(function, name)?;
        let atom = self.intern_atom("length");
        self.set_property(function, atom, Value::number(f64::from(length)))?;
        self.set_property_attributes(
            function,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(function)
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

    pub(super) fn initialize_host(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        for index in 0..self.host.functions().len() {
            let Some(name) = self.host.functions()[index].global else {
                continue;
            };
            let id = u32::try_from(index)
                .map_err(|_| JsError("host function table exceeds index space".into()))?;
            let function = self.create_host_function(HostFunctionId(id))?;
            self.global(program, name, function)?;
        }
        let mut context = NativeContext::new(self);
        H::initialize(&mut context).map_err(|error| context.finish_error(error))
    }

    pub(super) fn call_host_function(
        &mut self,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let index = self
            .active_native_env()
            .and_then(|environment| {
                environment.as_number().or_else(|| {
                    self.heap
                        .environment_slot(environment, HostEnvironmentSlot::Operation as usize)
                        .and_then(Value::as_number)
                })
            })
            .filter(|value| {
                value.is_finite()
                    && value.fract() == 0.0
                    && *value >= 0.0
                    && *value <= f64::from(u32::MAX)
            })
            .ok_or_else(|| JsError("invalid host function environment".into()))?
            as u32;
        let callback = self
            .host
            .functions()
            .get(index as usize)
            .map(|binding| binding.call)
            .ok_or_else(|| JsError("unknown host function index".into()))?;
        let mut context = NativeContext::new(self);
        let receiver = context.scoped_value(receiver);
        let arguments = args
            .iter()
            .map(|value| context.scoped_value(*value))
            .collect::<Vec<_>>();
        let result = callback(&mut context, receiver, &arguments);
        context.finish(result)
    }
}
