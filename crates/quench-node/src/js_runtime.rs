use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

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

struct QuenchNodeHost {
    hashes: RefCell<HashMap<u16, Vec<u8>>>,
    next_hash: Cell<u16>,
}

impl Default for QuenchNodeHost {
    fn default() -> Self {
        Self {
            hashes: RefCell::new(HashMap::new()),
            next_hash: Cell::new(100),
        }
    }
}

impl Host for QuenchNodeHost {
    fn call(&self, capability: HostCapabilityRef, arguments: &[Value]) -> Result<Value, VmError> {
        match capability.kind {
            HostCapabilityKind::Custom(1) => require_module(arguments),
            HostCapabilityKind::Custom(2) => basename(arguments),
            HostCapabilityKind::Custom(4) => console_log(arguments),
            HostCapabilityKind::Custom(6) => current_directory(arguments),
            HostCapabilityKind::Custom(7) => read_file_sync(arguments),
            HostCapabilityKind::Custom(8) => self.create_hash(arguments),
            HostCapabilityKind::Custom(9) => buffer_byte_length(arguments),
            HostCapabilityKind::Custom(id) if id >= 100 => self.hash_call(id, arguments),
            _ => Err(VmError::NotCallable),
        }
    }
}

impl QuenchNodeHost {
    fn create_hash(&self, arguments: &[Value]) -> Result<Value, VmError> {
        if !matches!(arguments.first(), Some(Value::String(name)) if name == "sha256") {
            return Err(VmError::EvalError("only sha256 is supported".into()));
        }
        let id = self.next_hash.get();
        self.next_hash.set(id.saturating_add(2));
        self.hashes.borrow_mut().insert(id, Vec::new());
        Ok(Value::object(vec![
            (
                "update".into(),
                capability_function(HostCapabilityKind::Custom(id)),
            ),
            (
                "digest".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            ),
        ]))
    }

    fn hash_call(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let base = id - (id % 2);
        if id % 2 == 0 {
            let value = string_or_bytes(arguments.first())?;
            self.hashes
                .borrow_mut()
                .entry(base)
                .or_default()
                .extend(value);
            return Ok(Value::object(vec![(
                "digest".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            )]));
        }
        let data = self.hashes.borrow().get(&base).cloned().unwrap_or_default();
        let digest = Sha256::digest(data);
        if matches!(arguments.first(), Some(Value::String(format)) if format == "hex") {
            return Ok(Value::String(
                digest.iter().map(|byte| format!("{byte:02x}")).collect(),
            ));
        }
        Ok(Value::String(String::from_utf8_lossy(&digest).into_owned()))
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

fn read_file_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(path)) = arguments.first() else {
        return Err(VmError::EvalError("readFileSync expects a path".into()));
    };
    let bytes = std::fs::read(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    if matches!(arguments.get(1), Some(Value::String(encoding)) if encoding == "utf8") {
        return String::from_utf8(bytes)
            .map(Value::String)
            .map_err(|error| VmError::EvalError(error.to_string()));
    }
    let buffer = std::rc::Rc::new(quench_runtime::value::ArrayBufferData::new(bytes.len()));
    buffer.bytes.borrow_mut().copy_from_slice(&bytes);
    Ok(Value::Uint8Array(std::rc::Rc::new(
        quench_runtime::value::Uint8ArrayData::new(buffer, 0, bytes.len()),
    )))
}

fn string_or_bytes(value: Option<&Value>) -> Result<Vec<u8>, VmError> {
    match value {
        Some(Value::String(value)) => Ok(value.as_bytes().to_vec()),
        Some(Value::Uint8Array(view)) => Ok(view.buffer.bytes.borrow()
            [view.byte_offset..view.byte_offset + view.length]
            .to_vec()),
        _ => Err(VmError::EvalError("expected string or bytes".into())),
    }
}

fn buffer_byte_length(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(
        string_or_bytes(arguments.first())?.len() as f64
    ))
}

fn require_module(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Err(VmError::EvalError("require expects a module name".into()));
    };
    if name != "node:path" && name != "path" {
        if name == "node:fs" || name == "fs" {
            return Ok(Value::object(vec![(
                "readFileSync".into(),
                capability_function(HostCapabilityKind::Custom(7)),
            )]));
        }
        if name == "node:crypto" || name == "crypto" {
            return Ok(Value::object(vec![(
                "createHash".into(),
                capability_function(HostCapabilityKind::Custom(8)),
            )]));
        }
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
                HostCapabilityKind::Custom(7),
                HostCapabilityKind::Custom(8),
                HostCapabilityKind::Custom(9),
            ],
        )
        .with_host(Rc::new(QuenchNodeHost::default()))
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
        )
        .with_host_value(
            "Buffer",
            Value::object(vec![(
                "byteLength".into(),
                capability_function(HostCapabilityKind::Custom(9)),
            )]),
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
