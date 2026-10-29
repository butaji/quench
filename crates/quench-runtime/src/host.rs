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
    /// The text the VM compiles, shared: a resolved module is kept and handed out once per
    /// import that names it, and copying its text each time had cost a large bundle's
    /// whole text per import (vite npm scenario: 250 copies of a 1.36 MB module).
    pub source: std::sync::Arc<str>,
    /// The module's bytes when they are not `source`'s UTF-8: a file that is not
    /// UTF-8, or a host interop wrapper that keeps the bytes it wraps. Absent, the
    /// bytes are `source` itself, so a module's text is never held twice.
    pub original_bytes: Option<std::sync::Arc<[u8]>>,
}

impl ModuleSource {
    /// A module whose bytes are its source text.
    pub fn text(name: impl Into<String>, source: impl Into<std::sync::Arc<str>>) -> Self {
        Self { name: name.into(), source: source.into(), original_bytes: None }
    }

    /// A module read as bytes: its source is their UTF-8 decoding, with replacement
    /// characters where they are not UTF-8, and the bytes are kept only then.
    pub fn from_bytes(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        match String::from_utf8(bytes) {
            Ok(source) => Self::text(name, source),
            Err(error) => {
                let bytes = error.into_bytes();
                let source = String::from_utf8_lossy(&bytes).into();
                Self { name: name.into(), source, original_bytes: Some(bytes.into()) }
            }
        }
    }

    /// The module's bytes, as an import with `type: "bytes"` reads them.
    pub fn bytes(&self) -> &[u8] {
        self.original_bytes.as_deref().unwrap_or(self.source.as_bytes())
    }
}

/// An opaque host execution context captured by a Promise reaction or
/// thenable-assimilation job. The VM carries it through the job lifecycle but
/// never interprets it. Hosts number contexts below `u64::MAX`, which a promise
/// record keeps for "no context" (`heap::StoredHostContext`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HostExecutionContext(pub u64);

pub trait Host {
    /// Whether host function `index` (into `functions`) observes the frame stack only by
    /// calling guest code, capturing an error's stack or making a collection due, so a lane
    /// may call it while its frameless activations stay frameless: each of those first makes
    /// them frames (`Vm::materialize_deferred_lane`). A host function that reads frames any
    /// other way (source positions, callers) must answer `false`, as the default does.
    fn host_function_runs_beside_activations(&self, _index: usize) -> bool {
        false
    }

    /// Whether Promise lifecycle observers are active now. While none are,
    /// Promise allocation and optimized await stay free of embedding callbacks
    /// and of per-Promise host resources.
    fn observes_promise_lifecycle(&self) -> bool {
        false
    }

    /// Whether `promise_job_boundary` may do anything now; while it is false the VM skips
    /// the call, and its context, around each Promise job.
    fn observes_promise_job_boundaries(&self) -> bool {
        true
    }

    /// A collection has finished, or a full one is starting: a host may return memory the
    /// process no longer needs resident (its read-only pages), which nothing observable
    /// depends on. At these boundaries the code about to run differs from the code that ran.
    /// The runtime's memory policy says whether it should
    /// (`MemoryPolicy::returns_pages_at_collections`).
    fn at_collection_boundary(&mut self, _policy: crate::MemoryPolicy) {}

    /// A compiled host program kept across runs: the bytes `keep_compiled_program` was given
    /// for compiling `parts` (the program's text, in order) under `name` in this build. The
    /// runtime decodes them under its own atoms and compiles when they do not decode. A
    /// decoded program keeps the bytes and reads each function's record on its first call,
    /// so a host serving a mapped file keeps the records never called out of memory.
    fn kept_compiled_program(&mut self, _name: &str, _parts: &[&str]) -> Option<crate::KeptBytes> {
        None
    }

    /// A program kept by `kept_compiled_program` has decoded, in place of its compilation,
    /// and is about to run. It parsed nothing, so it took none of a compilation's phase
    /// boundaries (`set_compile_phase_boundary`): a host may return the pages loading it
    /// touched here instead.
    fn cached_program_decoded(&mut self) {}

    /// Offers the host a compiled host program's compact form (`kept_compiled_program`).
    fn keep_compiled_program(&mut self, _name: &str, _parts: &[&str], _bytes: &[u8]) {}

    /// Whether `keep_compiled_program` keeps what it is offered: a host without a compile
    /// cache answers no, and the runtime then encodes nothing to offer it. Only a host that
    /// keeps programs overrides both.
    fn keeps_compiled_programs(&self) -> bool {
        false
    }

    /// A native compilation (a WebAssembly module's, or a JavaScript function's or loop
    /// region's) is about to start: like `at_collection_boundary`, the code about to run (the
    /// code generator) differs from the code that ran before it.
    fn before_native_compilation(&mut self) {}

    /// A JavaScript native compilation has finished: the code generator's code and its
    /// scratch memory, freed by now, are no longer needed while the program runs on.
    fn after_native_compilation(&mut self) {}

    /// The runtime has read `bytes`, part of bytes the host gave it (a snapshot's,
    /// `Runtime::restore_snapshot`, or a compiled program's, `kept_compiled_program`), and
    /// reads them again only to decode a function's record on its first call: a host that
    /// mapped them may return their pages, so they are not resident beside what was decoded
    /// from them.
    fn kept_bytes_read(_bytes: &[u8])
    where
        Self: Sized,
    {
    }

    fn write_line(&mut self, text: &str);
    fn clock_millis(&mut self) -> f64;

    /// ECMAScript proper tail calls discard strict caller activations. Hosts
    /// matching engines that retain those observable frames can disable them.
    fn proper_tail_calls(&self) -> bool {
        true
    }

    /// The stage 3 proposal "non-extensible applies to private": adding a private field,
    /// method or accessor to a non-extensible object throws (Test262
    /// `nonextensible-applies-to-private`). Hosts matching engines that do not ship it (V8,
    /// so Node) add them as before.
    fn nonextensible_applies_to_private(&self) -> bool {
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

    /// Continue installation in a VM restored from a startup snapshot this host's state was
    /// written into (`Runtime::write_snapshot`): what `initialize` does after the point the
    /// snapshot was taken at.
    fn resume(_context: &mut crate::NativeContext<'_, Self>) -> Result<(), crate::RootedError>
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
    fn create_promise_context(&mut self, _parent: Option<HostExecutionContext>) -> Option<HostExecutionContext> {
        None
    }

    /// Observe creation of an actual Promise with its installed host identity.
    fn initialize_promise(
        _context: &mut crate::NativeContext<'_, Self>,
        _promise: crate::RootId,
        _resource: HostExecutionContext,
    ) -> Result<(), crate::RootedError>
    where Self: Sized,
    {
        Ok(())
    }

    /// Observe execution boundaries and settlement of genuine Promise resources.
    fn promise_job_boundary(
        _context: &mut crate::NativeContext<'_, Self>,
        _resource: HostExecutionContext,
        _before: bool,
    ) -> Result<(), crate::RootedError> where Self: Sized { Ok(()) }

    fn promise_settled(
        _context: &mut crate::NativeContext<'_, Self>,
        _resource: HostExecutionContext,
    ) -> Result<(), crate::RootedError> where Self: Sized { Ok(()) }

    /// Capture the host context when the VM creates a Promise or thenable job.
    /// The returned token remains owned by that job until it runs or the VM
    /// collects it.
    fn capture_job_context(&mut self, _resource: Option<HostExecutionContext>) -> Option<HostExecutionContext> {
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

    /// Release the contexts `create_promise_context` gave promises that a collection freed.
    /// No promise, reaction or job names them again.
    fn release_promise_contexts(&mut self, _contexts: &[HostExecutionContext]) {}

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
    ) -> Result<Option<ModuleSource>, ModuleLoadError> {
        Ok(None)
    }

    /// Check a resolved module against the type its import requested (`with { type }`,
    /// `"javascript"` when absent): the host owns each module's format.
    fn validate_module_type(&mut self, _module: &ModuleSource, _module_type: &str) -> Result<(), ModuleLoadError> {
        Ok(())
    }

    /// Host-defined value for `import.meta.url`. Embeddings that use file paths
    /// for source names can expose the corresponding URL without changing the
    /// source identity used by module resolution.
    fn import_meta_url(&mut self, source_name: &str) -> String {
        source_name.to_owned()
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

/// The error constructor a host's module-load failure is reported with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleLoadErrorKind {
    Error,
    TypeError,
}

/// A host's failure to resolve or load a module, thrown as `kind` with `message` and,
/// when present, a `code` property (as embedders such as Node identify their errors)
/// followed by any further string `fields`, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleLoadError {
    pub kind: ModuleLoadErrorKind,
    pub code: Option<&'static str>,
    pub message: String,
    pub fields: Vec<(&'static str, String)>,
}

impl ModuleLoadError {
    pub fn new(kind: ModuleLoadErrorKind, code: &'static str, message: impl Into<String>) -> Self {
        Self { kind, code: Some(code), message: message.into(), fields: Vec::new() }
    }

    /// Add an own string property after `code`.
    #[must_use]
    pub fn with_field(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.fields.push((name, value.into()));
        self
    }
}

/// An uncoded `TypeError`, the report of a host without its own error identities.
impl From<String> for ModuleLoadError {
    fn from(message: String) -> Self {
        Self { kind: ModuleLoadErrorKind::TypeError, code: None, message, fields: Vec::new() }
    }
}

impl From<&str> for ModuleLoadError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
