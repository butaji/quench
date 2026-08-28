//! WebAssembly execution for Quench.
//!
//! `quench-wasm` keeps the Wasm machine separate from the JavaScript runtime,
//! while providing one explicit bridge for host code that needs to evaluate
//! JavaScript through `quench-runtime`.  The Wasm implementation is backed by
//! Wasmi; callers only need this crate's small, owned value and instance API.

use std::fmt;

pub use wasmi::{Config, Engine as RawEngine, Extern, Func, ValType, F32, F64};

/// A WebAssembly value accepted by the dynamic invocation API.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl Value {
    fn into_raw(self) -> wasmi::Val {
        match self {
            Self::I32(value) => wasmi::Val::I32(value),
            Self::I64(value) => wasmi::Val::I64(value),
            Self::F32(value) => wasmi::Val::F32(wasmi::F32::from(value)),
            Self::F64(value) => wasmi::Val::F64(wasmi::F64::from(value)),
        }
    }

    fn from_raw(value: &wasmi::Val) -> Result<Self, Error> {
        match value {
            wasmi::Val::I32(value) => Ok(Self::I32(*value)),
            wasmi::Val::I64(value) => Ok(Self::I64(*value)),
            wasmi::Val::F32(value) => Ok(Self::F32(f32::from(*value))),
            wasmi::Val::F64(value) => Ok(Self::F64(f64::from(*value))),
            other => Err(Error::UnsupportedValue(format!("{other:?}"))),
        }
    }
}

/// Errors raised at the Wasm or JavaScript execution boundary.
#[derive(Debug)]
pub enum Error {
    Compile(String),
    Instantiate(String),
    ExportNotFound(String),
    Invocation(String),
    UnsupportedValue(String),
    JavaScript(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, message) = match self {
            Self::Compile(message) => ("compile", message),
            Self::Instantiate(message) => ("instantiate", message),
            Self::ExportNotFound(message) => ("export", message),
            Self::Invocation(message) => ("invoke", message),
            Self::UnsupportedValue(message) => ("value", message),
            Self::JavaScript(message) => ("javascript", message),
        };
        write!(f, "{kind} error: {message}")
    }
}

impl std::error::Error for Error {}

/// A compiled WebAssembly engine. Cloning is cheap and shares Wasmi's engine
/// internals, so one engine can compile and instantiate many modules.
#[derive(Clone, Debug)]
pub struct Engine {
    inner: wasmi::Engine,
    config: Config,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl Engine {
    pub fn new(config: Config) -> Self {
        Self {
            inner: wasmi::Engine::new(&config),
            config,
        }
    }

    pub fn compile(&self, bytes: &[u8]) -> Result<Module, Error> {
        wasmi::Module::new(&self.inner, bytes)
            .map(|inner| Module {
                engine: self.inner.clone(),
                inner,
            })
            .map_err(|error| Error::Compile(error.to_string()))
    }

    pub fn compile_wat(&self, source: &str) -> Result<Module, Error> {
        let bytes = wat::parse_str(source).map_err(|error| Error::Compile(error.to_string()))?;
        self.compile(&bytes)
    }

    /// Evaluate JavaScript using the same residual VM used by Quench's other
    /// hosts. This is deliberately an edge operation: Wasm execution remains
    /// independent, while host adapters can opt into JavaScript semantics.
    pub fn javascript(&self) -> JavaScriptRuntime {
        JavaScriptRuntime
    }

    /// Run one WAST file with Wasmi's conformance runner.
    pub fn run_wast(&self, filename: &str, source: &str) -> Result<(), Error> {
        let mut runner = wasmi_wast::WastRunner::new(&self.config);
        runner
            .register_spectest()
            .map_err(|error| Error::Instantiate(error.to_string()))?;
        runner
            .register_wasmitest()
            .map_err(|error| Error::Instantiate(error.to_string()))?;
        runner
            .process_directives(filename, source)
            .map_err(|error| Error::Invocation(error.to_string()))
    }
}

/// A compiled module tied to its originating [`Engine`].
#[derive(Clone, Debug)]
pub struct Module {
    engine: wasmi::Engine,
    inner: wasmi::Module,
}

impl Module {
    pub fn instantiate(&self) -> Result<Instance, Error> {
        let mut store = wasmi::Store::new(&self.engine, ());
        let linker = wasmi::Linker::new(&self.engine);
        let instance = linker
            .instantiate_and_start(&mut store, &self.inner)
            .map_err(|error| Error::Instantiate(error.to_string()))?;
        Ok(Instance { store, instance })
    }
}

/// An instantiated module with an owned store and dynamic export invocation.
pub struct Instance {
    store: wasmi::Store<()>,
    instance: wasmi::Instance,
}

impl Instance {
    pub fn call(&mut self, export: &str, arguments: &[Value]) -> Result<Vec<Value>, Error> {
        let function = self
            .instance
            .get_func(&self.store, export)
            .ok_or_else(|| Error::ExportNotFound(export.to_string()))?;
        let inputs: Vec<_> = arguments.iter().cloned().map(Value::into_raw).collect();
        let mut outputs = function
            .ty(&self.store)
            .results()
            .iter()
            .copied()
            .map(wasmi::Val::default_for_ty)
            .collect::<Vec<_>>();
        function
            .call(&mut self.store, &inputs, &mut outputs)
            .map_err(|error| Error::Invocation(error.to_string()))?;
        outputs.iter().map(Value::from_raw).collect()
    }
}

/// JavaScript execution bridge used by Wasm host adapters.
#[derive(Clone, Copy, Debug, Default)]
pub struct JavaScriptRuntime;

impl JavaScriptRuntime {
    /// Execute JavaScript source through Quench's residual runtime.
    pub fn execute(&self, source: &str) -> Result<(), Error> {
        self.evaluate(source).map(|_| ())
    }

    /// Evaluate JavaScript source and return the runtime completion value.
    pub fn evaluate(&self, source: &str) -> Result<quench_runtime::value::Value, Error> {
        quench_runtime::vm::reset_global_object();
        quench_runtime::vm::reset_host_agent_state();
        quench_runtime::builtins::reset_intrinsic_prototype_state();
        quench_runtime::execute::reset_replacements();
        let program = quench_runtime::reduce::reduce_source(source)
            .map_err(|errors| Error::JavaScript(errors.join("; ")))?;
        let context = quench_runtime::vm::VmContext::isolated();
        let result = quench_runtime::execute::execute_code_with_context(program.code(), &context)
            .map_err(|error| Error::JavaScript(error.render()));
        quench_runtime::vm::reset_host_agent_state();
        quench_runtime::module_bindings::drain_jobs();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{Engine, Value};

    #[test]
    fn executes_an_exported_function() {
        let module = Engine::default()
            .compile_wat("(module (func (export \"add\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add))")
            .expect("compile");
        let mut instance = module.instantiate().expect("instantiate");
        assert_eq!(
            instance
                .call("add", &[Value::I32(2), Value::I32(40)])
                .unwrap(),
            [Value::I32(42)]
        );
    }

    #[test]
    fn evaluates_javascript_through_runtime() {
        let result = Engine::default()
            .javascript()
            .evaluate("1 + 2")
            .expect("evaluate");
        assert_eq!(result, quench_runtime::value::Value::Undefined);
    }
}
