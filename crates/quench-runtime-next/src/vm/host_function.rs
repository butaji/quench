use super::*;
use crate::{HostFunctionId, NativeContext};

impl<H: Host> Vm<H> {
    pub(crate) fn create_host_function(&mut self, id: HostFunctionId) -> Result<Value, JsError> {
        self.embedding_program()?;
        let (name, length) = self
            .host
            .functions()
            .get(id.0 as usize)
            .map(|binding| (binding.name, binding.length))
            .ok_or_else(|| JsError("unknown host function index".into()))?;
        // An operation index is exact in f64. It is private native environment
        // data, not a guest object identity or a second callback registry.
        let function = self.native_with_env(Native::HostFunction, Value::number(f64::from(id.0)));
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
            .and_then(Value::as_number)
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
