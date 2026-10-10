use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

/// Opaque operation index owned by the embedding's host-function table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasmHostFunctionId(pub u32);

/// Bit-exact scalars and generation-checked references at the host boundary.
/// Reference handles borrowed for a call expire when that call returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmHostValue {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    V128(u128),
    FuncRef(Option<crate::RootId>),
    ExternRef(Option<crate::RootId>),
    GcRef(Option<crate::RootId>),
}

/// Stable capability identifiers used at the VM/host boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum CapabilityId {
    WriteLine = 1,
    ClockMillis = 2,
    Done = 3,
    CreateRealm = 4,
    IsHTMLDDA = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostGlobal {
    pub name: &'static str,
    pub capability: CapabilityId,
}

/// A host-resolved source unit. The VM owns parsing, module semantics, and
/// execution; the host supplies only source identity and bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleSource {
    pub name: String,
    pub source: String,
    pub bytes: Vec<u8>,
}

/// An opaque host execution context captured by a Promise reaction or
/// thenable-assimilation job. The VM carries it through the job lifecycle but
/// never interprets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HostExecutionContext(pub u64);

pub trait Host {
    fn write_line(&mut self, text: &str);
    fn clock_millis(&mut self) -> f64;

    /// Stable native-operation table for this host's lifetime. Mutable host
    /// state belongs in `Self`; operation indices must never be reassigned.
    fn functions(&self) -> &[crate::HostFunction<Self>]
    where
        Self: Sized,
    {
        &[]
    }

    /// Install host-owned objects after shared intrinsics and native globals,
    /// before guest execution. Context roots expire when installation returns.
    fn initialize(_context: &mut crate::NativeContext<'_, Self>) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        Ok(())
    }

    /// Capture the host context when the VM creates a Promise or thenable job.
    /// The returned token remains owned by that job until it runs or the VM
    /// collects it.
    fn capture_job_context(&mut self) -> Option<HostExecutionContext> {
        None
    }

    /// Enter a captured host context around one queued Promise or thenable job
    /// and return the context to restore after the job completes.
    fn enter_job_context(
        &mut self,
        _context: HostExecutionContext,
    ) -> Option<HostExecutionContext> {
        None
    }

    /// Restore the host context returned by `enter_job_context`.
    fn restore_job_context(&mut self, _previous: Option<HostExecutionContext>) {}

    /// Release a completed or collected context snapshot. Returned roots are
    /// released by the runtime that owns them.
    fn release_job_context(&mut self, _context: HostExecutionContext) -> Vec<crate::RootId> {
        Vec::new()
    }

    fn call_wasm(
        &mut self,
        _function: WasmHostFunctionId,
        _args: &[WasmHostValue],
    ) -> Result<Vec<WasmHostValue>, String> {
        Err("host does not provide Wasm functions".into())
    }

    /// Optional host-owned globals. The evaluator installs these only when
    /// the host explicitly advertises them; ordinary production hosts remain
    /// free of conformance or embedding-specific names.
    fn globals(&self) -> &'static [HostGlobal] {
        &[]
    }

    /// Resolve one dynamic-import request relative to its active source unit.
    /// `Ok(None)` means this host does not provide module loading.
    fn resolve_dynamic_import(
        &mut self,
        _referrer: &str,
        _specifier: &str,
    ) -> Result<Option<ModuleSource>, String> {
        Ok(None)
    }

    /// Whether this host-resolved unit has a host-defined Module Source Object.
    /// JavaScript text and ordinary synthetic imports have no source representation.
    fn has_module_source(&self, _module: &ModuleSource) -> bool {
        false
    }

    fn done(&mut self, _text: Option<&str>) {}

    /// Whether the active embedding permits a synchronous Atomics.wait.
    fn can_block(&self) -> bool {
        false
    }
}

/// Borrowed capability context. It exposes typed host effects without
/// handing heap cells or guest `Value` handles to the host.
pub struct HostContext<'a, H: Host> {
    host: &'a mut H,
}

impl<'a, H: Host> HostContext<'a, H> {
    pub(crate) fn new(host: &'a mut H) -> Self {
        Self { host }
    }

    pub(crate) fn invoke(&mut self, capability: CapabilityId, text: Option<&str>) -> f64 {
        match capability {
            CapabilityId::WriteLine => {
                self.host.write_line(text.unwrap_or_default());
                0.0
            }
            CapabilityId::ClockMillis => self.host.clock_millis(),
            CapabilityId::Done => {
                self.host.done(text);
                0.0
            }
            CapabilityId::CreateRealm => 0.0,
            CapabilityId::IsHTMLDDA => 0.0,
        }
    }
}

#[derive(Default)]
pub struct SystemHost;

impl Host for SystemHost {
    fn write_line(&mut self, text: &str) {
        let _ = writeln!(io::stdout().lock(), "{text}");
    }

    fn clock_millis(&mut self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
            * 1000.0
    }
}
