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
    /// Opt in to Promise lifecycle observations. Hosts without observers keep
    /// Promise allocation and optimized await free of embedding callbacks.
    const OBSERVE_PROMISE_LIFECYCLE: bool = false;

    fn write_line(&mut self, text: &str);
    fn clock_millis(&mut self) -> f64;

    /// ECMAScript proper tail calls discard strict caller activations. Hosts
    /// matching engines that retain those observable frames can disable them.
    fn proper_tail_calls(&self) -> bool {
        true
    }

    /// Optional retained callback owning the host's Error stack policy,
    /// including dispatch of user prepareStackTrace hooks. Called as
    /// (global, error, callSites). The host owns the callback root's lifetime.
    fn error_stack_formatter(&self) -> Option<crate::RootId> {
        None
    }

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

    /// Initialize host policy in a fresh ShadowRealm before guest execution.
    /// An optional callback owns this realm's Error formatting as
    /// (global, error, callSites). The VM traces it with the realm intrinsics;
    /// temporary context roots can be returned without retaining them.
    fn initialize_shadow_realm(
        _context: &mut crate::NativeContext<'_, Self>,
        _global: crate::RootId,
    ) -> Result<Option<crate::RootId>, crate::RootedError>
    where
        Self: Sized,
    {
        Ok(None)
    }

    /// Allocate host metadata before Promise creation becomes observable. This
    /// phase cannot call guest code; reentrant init hooks see the stored identity.
    fn create_promise_context(
        &mut self,
        _parent: Option<HostExecutionContext>,
    ) -> Option<HostExecutionContext> {
        None
    }

    /// Observe creation of an actual Promise with its installed host identity.
    fn initialize_promise(
        _context: &mut crate::NativeContext<'_, Self>,
        _promise: crate::RootId,
        _resource: HostExecutionContext,
    ) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        Ok(())
    }

    /// Observe execution boundaries and settlement of genuine Promise resources.
    fn promise_job_boundary(
        _context: &mut crate::NativeContext<'_, Self>,
        _resource: HostExecutionContext,
        _before: bool,
    ) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        Ok(())
    }

    fn promise_settled(
        _context: &mut crate::NativeContext<'_, Self>,
        _resource: HostExecutionContext,
    ) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        Ok(())
    }

    /// Capture the host context when the VM creates a Promise or thenable job.
    /// The returned token remains owned by that job until it runs or the VM
    /// collects it.
    fn capture_job_context(
        &mut self,
        _resource: Option<HostExecutionContext>,
    ) -> Option<HostExecutionContext> {
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

    /// Decode and validate a WebAssembly binary for the `WebAssembly` namespace, lowering
    /// it for `mode`. The error text becomes a `WebAssembly.CompileError` message. A host
    /// without a Wasm frontend cannot compile.
    fn compile_wasm(
        &mut self,
        _bytes: &[u8],
        _mode: crate::ExecutionMode,
    ) -> Result<crate::WasmModule, String> {
        Err("WebAssembly compilation is not available in this host".into())
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

    /// Resolve one dynamic or static module request while the VM context is
    /// available. Hosts that need guest-owned module projections can override
    /// this hook; the default preserves the context-free resolver contract.
    fn resolve_dynamic_import_with_context(
        context: &mut crate::NativeContext<'_, Self>,
        referrer: &str,
        specifier: &str,
    ) -> Result<Option<ModuleSource>, crate::RootedError>
    where
        Self: Sized,
    {
        context
            .host_mut()
            .resolve_dynamic_import(referrer, specifier)
            .map_err(crate::RootedError::host)
    }

    /// Host-defined value for `import.meta.url`. Embeddings that use file paths
    /// for source names can expose the corresponding URL without changing the
    /// source identity used by module resolution.
    fn import_meta_url(&mut self, source_name: &str) -> String {
        source_name.to_owned()
    }

    /// Absolute filesystem name for a file-backed module. Non-file modules
    /// leave this unset, matching the optional Node `import.meta.filename`.
    fn import_meta_filename(&mut self, _source_name: &str) -> Option<String> {
        None
    }

    /// Initialize host-specific properties on a freshly-created `import.meta`
    /// object. The object is cached by the VM for its source module; hosts that
    /// do not expose extra metadata keep the default empty behavior.
    fn initialize_import_meta(
        _context: &mut crate::NativeContext<'_, Self>,
        _source_name: &str,
        _import_meta: crate::RootId,
    ) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        Ok(())
    }

    /// Display identity for source locations in captured stacks. This must not
    /// change the identity used to resolve imports or look up compiled programs.
    fn stack_source_name(&mut self, source_name: &str, _is_module: bool) -> String {
        source_name.to_owned()
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
