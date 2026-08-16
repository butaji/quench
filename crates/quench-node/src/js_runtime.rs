use std::path::{Path, PathBuf};
use std::rc::Rc;

use quench_runtime::{
    ops::{HostCapabilityKind, HostCapabilityRef, RealmId},
    value::{HostCapabilityValue, Value},
    vm::{Host, VmContext, VmError},
};

pub(crate) struct FilesystemNodeHost;

impl NodeHost for FilesystemNodeHost {
    fn resolve_module(
        &self,
        request: &str,
        parent: Option<&Path>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let base = parent
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        Ok(if request.starts_with('.') {
            base.join(request)
        } else {
            PathBuf::from(request)
        })
    }

    fn load_module(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
        Ok(std::fs::read_to_string(path)?)
    }
}

pub(crate) trait NodeHost {
    fn resolve_module(
        &self,
        request: &str,
        parent: Option<&Path>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>>;

    fn load_module(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>>;
}

pub(crate) trait JsRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>>;

    fn has_pending_jobs(&self) -> bool;
}

pub(crate) struct QuenchRuntime;

struct QuenchNodeHost;

impl Host for QuenchNodeHost {
    fn call(&self, capability: HostCapabilityRef, arguments: &[Value]) -> Result<Value, VmError> {
        match capability.kind {
            HostCapabilityKind::Custom(1) => require_module(arguments),
            HostCapabilityKind::Custom(2) => basename(arguments),
            HostCapabilityKind::Custom(4) => console_log(arguments),
            HostCapabilityKind::Custom(6) => current_directory(arguments),
            _ => Err(VmError::NotCallable),
        }
    }
}

fn console_log(arguments: &[Value]) -> Result<Value, VmError> {
    let line = arguments
        .iter()
        .map(|value| match value {
            Value::String(value) => value.clone(),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join(" ");
    println!("{line}");
    Ok(Value::Undefined)
}

fn current_directory(arguments: &[Value]) -> Result<Value, VmError> {
    if !arguments.is_empty() {
        return Err(VmError::EvalError(
            "process.cwd expects no arguments".into(),
        ));
    }
    Ok(Value::String(
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .into_owned(),
    ))
}

fn require_module(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Err(VmError::EvalError("require expects a module name".into()));
    };
    if name != "node:path" && name != "path" {
        return Err(VmError::EvalError(format!("Cannot find module '{name}'")));
    }
    let basename = capability_function(HostCapabilityKind::Custom(2));
    Ok(Value::object(vec![("basename".into(), basename)]))
}

fn capability_function(kind: HostCapabilityKind) -> Value {
    let token = Value::HostCapability(Rc::new(HostCapabilityValue::new(HostCapabilityRef {
        realm: RealmId::ROOT,
        kind,
    })));
    Value::BoundFunction(Rc::new(quench_runtime::value::BoundFunctionValue::new(
        RealmId::ROOT,
        Value::Builtin(quench_runtime::ops::Builtin::HostCapability(kind)),
        token,
    )))
}

fn basename(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(path)) = arguments.first() else {
        return Err(VmError::EvalError("path.basename expects a string".into()));
    };
    Ok(Value::String(
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .into(),
    ))
}

impl JsRuntime for QuenchRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        _host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let program =
            match path.is_some_and(|path| path.extension().is_some_and(|ext| ext == "mjs")) {
                true => quench_runtime::reduce::reduce_module_source(source),
                false => quench_runtime::reduce::reduce_source(source),
            }
            .map_err(|errors| errors.join("\n"))?;
        let capability = HostCapabilityRef {
            realm: RealmId::ROOT,
            kind: HostCapabilityKind::Custom(1),
        };
        let context = VmContext::for_realm(
            RealmId::ROOT,
            vec![
                HostCapabilityKind::Custom(1),
                HostCapabilityKind::Custom(2),
                HostCapabilityKind::Custom(3),
                HostCapabilityKind::Custom(4),
                HostCapabilityKind::Custom(5),
                HostCapabilityKind::Custom(6),
            ],
        )
        .with_host(Rc::new(QuenchNodeHost))
        .with_host_capability("require", capability)
        .with_host_capability(
            "console",
            HostCapabilityRef {
                realm: RealmId::ROOT,
                kind: HostCapabilityKind::Custom(3),
            },
        )
        .with_host_value(
            "process",
            Value::object(vec![
                (
                    "argv".into(),
                    Value::array(std::env::args().map(Value::String).collect()),
                ),
                (
                    "cwd".into(),
                    capability_function(HostCapabilityKind::Custom(6)),
                ),
            ]),
        );
        quench_runtime::execute::execute_with_context(program.ops(), &context)
            .map(|_| ())
            .map_err(|error| error.render().into())
    }

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }

    fn has_pending_jobs(&self) -> bool {
        false
    }
}

pub(crate) struct QuickJsRuntime {
    runtime: rquickjs::Runtime,
}

impl QuickJsRuntime {
    pub(crate) fn new() -> Result<Self, rquickjs::Error> {
        Ok(Self {
            runtime: rquickjs::Runtime::new()?,
        })
    }
}

impl JsRuntime for QuickJsRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        _host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::quickjs_backend::execute_source(source, &self.runtime, path)?;
        while self.has_pending_jobs() {
            self.poll_jobs()?;
        }
        Ok(())
    }

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>> {
        self.runtime
            .execute_pending_job()
            .map_err(|error| format!("QuickJS job failed: {error:?}").into())
    }

    fn has_pending_jobs(&self) -> bool {
        self.runtime.is_job_pending()
    }
}
