//! The one host type the VM is compiled for. `Vm<H: Host>` is generic, but an embedding runs
//! one host, so every VM function was compiled as an instance of the embedder's host type in the
//! embedder's crate, and any runtime edit recompiled that whole crate. `ErasedHost` is a concrete
//! host defined here: the embedder implements `ErasableHost` (the `Host` interface with
//! `ErasedHost` in place of `Self`) and wraps its host in it, so the VM's instances are
//! compiled in this crate, once. A native callback reaches the embedder's own host with
//! `ErasedHost::get`.

use std::any::{Any, TypeId};
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::host::{HostExecutionContext, HostGlobal, ModuleLoadError, ModuleSource, WasmHostFunctionId, WasmHostValue};
#[cfg(feature = "native-execution")]
use crate::host::NativeSourceError;

/// The `Host` interface of an embedder's host, wrapped in an `ErasedHost`. Each method is the
/// `Host` method of the same name; the associated functions without a receiver are read once,
/// when the host is wrapped (`ErasedHost::new`).
pub trait ErasableHost: Any {

    /// Whether host function `index` (into `functions`) observes the frame stack only by
    /// calling guest code, capturing an error's stack or making a collection due, so a lane
    /// may call it while its frameless activations stay frameless: each of those first makes
    /// them frames (`Vm::materialize_deferred_lane`). A host function that reads frames any
    /// other way (source positions, callers) must answer `false`, as the default does.
    /// Read this proof from operation metadata in constant time; the VM keeps no separate
    /// per-instance copy of the host's declared policy.
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

    /// A young collection is about to run (a full one's start is `at_collection_boundary`).
    fn before_collection(&mut self) {}

    /// Weak targets have been cleared and VM cleanup has finished. Release native resources
    /// owned by collected values before execution resumes, without running guest code.
    fn after_collection(_context: &mut crate::CollectionContext<'_, ErasedHost>) where Self: Sized {}

    /// A compiled host program kept across runs: the bytes `keep_compiled_program` was given
    /// for compiling `parts` (the program's text, in order) under `name` in this build. The
    /// runtime decodes them under its own atoms and compiles when they do not decode. A
    /// decoded program keeps the bytes and reads each function's record on its first call,
    /// so a host serving a mapped file keeps the records never called out of memory.
    fn kept_compiled_program(&mut self, _name: &str, _parts: &[&str]) -> Option<crate::KeptBytes> {
        None
    }

    /// Native code for the same exact source-shaping key as the compile cache.
    /// Returning an image commits to its atom IDs and host ABI; an incompatible
    /// image fails admission rather than recompiling behind the host's back.
    #[cfg(feature = "native-execution")]
    fn kept_native_program(
        &mut self, _name: &str, _parts: &[&str],
    ) -> Result<Option<crate::NativeJsProgram<ErasedHost>>, NativeSourceError>
    {
        Ok(None)
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

    /// V8's `--allow-natives-syntax`: eval code may carry its optimization directives
    /// (`%PrepareFunctionForOptimization(f)`), which then evaluate only their argument.
    fn allows_natives_syntax(&self) -> bool {
        false
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
    fn functions(&self) -> &[crate::HostFunction<ErasedHost>]
    {
        &[]
    }

    /// Install host-owned objects after shared intrinsics and native globals,
    /// before guest execution. Context roots expire when installation returns.
    fn initialize(_context: &mut crate::NativeContext<'_, ErasedHost>) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        Ok(())
    }

    /// Continue installation in a VM restored from a startup snapshot this host's state was
    /// written into (`Runtime::write_snapshot`): what `initialize` does after the point the
    /// snapshot was taken at.
    fn resume(_context: &mut crate::NativeContext<'_, ErasedHost>) -> Result<(), crate::RootedError>
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
        _context: &mut crate::NativeContext<'_, ErasedHost>,
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
        _context: &mut crate::NativeContext<'_, ErasedHost>,
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
        _context: &mut crate::NativeContext<'_, ErasedHost>,
        _resource: HostExecutionContext,
        _before: bool,
    ) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        Ok(())
    }

    fn promise_settled(
        _context: &mut crate::NativeContext<'_, ErasedHost>,
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

    /// Enter the context of a job `capture_job_context` captured as `None`, unless the job
    /// queue has `entered` it for an earlier job. A host that observes the job now answers a
    /// fresh context, which the job enters, observes and releases as a captured one;
    /// otherwise the job runs, then `end_unobserved_job`. A host whose
    /// `observes_promise_lifecycle` is false should answer `None` once the queue has `entered`
    /// the context: the VM runs such a queue's consecutive unobserved await jobs in one
    /// resumption (`Vm::run_job_chain`), where a context answered runs the job as an observed
    /// one and ends the chain.
    fn enter_unobserved_job(&mut self, _entered: bool) -> Option<HostExecutionContext> {
        None
    }

    /// End an unobserved job in the context it shares with the queue's next ones: what the
    /// job left in it is released. Returned roots are released by the runtime.
    fn end_unobserved_job(&mut self) -> Vec<crate::RootId> {
        Vec::new()
    }

    /// Leave `enter_unobserved_job`'s context. Returned roots are released by the runtime.
    fn leave_unobserved_job(&mut self) -> Vec<crate::RootId> {
        Vec::new()
    }

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

    /// Validate a binary without producing executable code. The default preserves
    /// existing compile-backed hosts; frontends can provide validation independently.
    fn validate_wasm(&mut self, bytes: &[u8], mode: crate::ExecutionMode) -> bool {
        self.compile_wasm(bytes, mode).is_ok()
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

    /// Absolute filesystem name for a file-backed module. Non-file modules
    /// leave this unset, matching the optional Node `import.meta.filename`.
    fn import_meta_filename(&mut self, _source_name: &str) -> Option<String> {
        None
    }

    /// Whether `initialize_import_meta` adds fields to the `import.meta` objects it creates.
    fn initializes_import_meta(&self) -> bool {
        false
    }

    /// Add host-owned fields to `import_meta` after its standard fields are defined.
    fn initialize_import_meta(
        _context: &mut crate::NativeContext<'_, ErasedHost>,
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
    fn function_count(&self) -> usize {
        self.functions().len()
    }

    fn function_descriptor(&self, index: usize) -> Option<crate::HostFunctionDescriptor> {
        self.functions().get(index).map(|binding| crate::HostFunctionDescriptor {
            name: binding.name, length: binding.length, global: binding.global,
        })
    }

    fn function_callback(&self, index: usize) -> Option<crate::HostCallback<ErasedHost>> {
        self.functions().get(index).map(|binding| binding.call)
    }
}

type ContextCall<R> = for<'a, 'b> fn(&'b mut crate::NativeContext<'a, ErasedHost>) -> R;

/// The receiver-less functions of the wrapped host's `ErasableHost` implementation.
struct Statics {
    after_collection: for<'a, 'b> fn(&'b mut crate::CollectionContext<'a, ErasedHost>),
    initialize: ContextCall<Result<(), crate::RootedError>>,
    resume: ContextCall<Result<(), crate::RootedError>>,
    initialize_shadow_realm: for<'a, 'b> fn(&'b mut crate::NativeContext<'a, ErasedHost>, crate::RootId) -> Result<Option<crate::RootId>, crate::RootedError>,
    initialize_promise: for<'a, 'b> fn(&'b mut crate::NativeContext<'a, ErasedHost>, crate::RootId, HostExecutionContext) -> Result<(), crate::RootedError>,
    promise_job_boundary: for<'a, 'b> fn(&'b mut crate::NativeContext<'a, ErasedHost>, HostExecutionContext, bool) -> Result<(), crate::RootedError>,
    promise_settled: for<'a, 'b> fn(&'b mut crate::NativeContext<'a, ErasedHost>, HostExecutionContext) -> Result<(), crate::RootedError>,
    initialize_import_meta: for<'a, 'b, 'c> fn(&'b mut crate::NativeContext<'a, ErasedHost>, &'c str, crate::RootId) -> Result<(), crate::RootedError>,
}

/// The wrapped host's `kept_bytes_read` (it has no receiver, and `Host::kept_bytes_read` no
/// context): the last host wrapped sets it.
static KEPT_BYTES_READ: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

fn kept_bytes_reader() -> fn(&[u8]) {
    let read = KEPT_BYTES_READ.load(Ordering::Relaxed);
    if read.is_null() {
        |_| {}
    } else {
        // SAFETY: only `ErasedHost::new` stores here, and it stores a `fn(&[u8])`.
        unsafe { std::mem::transmute::<*mut (), fn(&[u8])>(read) }
    }
}

/// The concrete host every embedding runs the VM with: its own host behind `ErasableHost`.
pub struct ErasedHost {
    inner: Box<dyn ErasableHost>,
    statics: &'static Statics,
    type_id: TypeId,
}

impl ErasedHost {
    pub fn new<T: ErasableHost>(host: T) -> Self {
        let statics: &'static Statics = const {
            &Statics {
                after_collection: T::after_collection,
                initialize: T::initialize,
                resume: T::resume,
                initialize_shadow_realm: T::initialize_shadow_realm,
                initialize_promise: T::initialize_promise,
                promise_job_boundary: T::promise_job_boundary,
                promise_settled: T::promise_settled,
                initialize_import_meta: T::initialize_import_meta,
            }
        };
        KEPT_BYTES_READ.store(T::kept_bytes_read as fn(&[u8]) as *mut (), Ordering::Relaxed);
        Self { inner: Box::new(host), statics, type_id: TypeId::of::<T>() }
    }

    /// The wrapped host, which must be a `T`.
    #[inline(always)]
    pub fn get<T: ErasableHost>(&mut self) -> &mut T {
        debug_assert!(self.type_id == TypeId::of::<T>(), "ErasedHost::get of a type it does not wrap");
        // SAFETY: the box holds a `T` (checked in debug builds); a `dyn` pointer's data part is
        // the value's address.
        unsafe { &mut *(&mut *self.inner as *mut dyn ErasableHost).cast::<T>() }
    }

    /// The wrapped host, which must be a `T`.
    #[inline(always)]
    pub fn get_ref<T: ErasableHost>(&self) -> &T {
        debug_assert!(self.type_id == TypeId::of::<T>(), "ErasedHost::get_ref of a type it does not wrap");
        // SAFETY: as in `get`.
        unsafe { &*(&*self.inner as *const dyn ErasableHost).cast::<T>() }
    }

    /// The wrapped host, if it is a `T`.
    pub fn downcast_mut<T: ErasableHost>(&mut self) -> Option<&mut T> {
        (self.type_id == TypeId::of::<T>()).then(|| self.get::<T>())
    }

    /// The wrapped host back, if it is a `T`.
    pub fn into_inner<T: ErasableHost>(self) -> Result<T, Self> {
        if self.type_id != TypeId::of::<T>() {
            return Err(self);
        }
        let raw = Box::into_raw(self.inner).cast::<T>();
        // SAFETY: the box held a `T`, allocated as one.
        Ok(*unsafe { Box::from_raw(raw) })
    }
}

impl crate::Host for ErasedHost {
    #[inline]
    fn function_count(&self) -> usize {
        self.inner.function_count()
    }

    #[inline]
    fn function_descriptor(&self, index: usize) -> Option<crate::HostFunctionDescriptor> {
        self.inner.function_descriptor(index)
    }

    #[inline]
    fn function_callback(&self, index: usize) -> Option<crate::HostCallback<Self>> {
        self.inner.function_callback(index)
    }
    #[inline]
    fn host_function_runs_beside_activations(&self, index: usize) -> bool {
        self.inner.host_function_runs_beside_activations(index)
    }

    #[inline]
    fn observes_promise_lifecycle(&self) -> bool {
        self.inner.observes_promise_lifecycle()
    }

    #[inline]
    fn observes_promise_job_boundaries(&self) -> bool {
        self.inner.observes_promise_job_boundaries()
    }

    #[inline]
    fn at_collection_boundary(&mut self, policy: crate::MemoryPolicy) {
        self.inner.at_collection_boundary(policy)
    }

    #[inline]
    fn before_collection(&mut self) {
        self.inner.before_collection()
    }

    #[inline]
    fn after_collection(context: &mut crate::CollectionContext<'_, Self>)
    where
        Self: Sized,
    {
        let call = context.host_mut().statics.after_collection;
        call(context)
    }

    #[inline]
    fn kept_compiled_program(&mut self, name: &str, parts: &[&str]) -> Option<crate::KeptBytes> {
        self.inner.kept_compiled_program(name, parts)
    }

    #[cfg(feature = "native-execution")]
    #[inline]
    fn kept_native_program(
        &mut self, name: &str, parts: &[&str],
    ) -> Result<Option<crate::NativeJsProgram<Self>>, NativeSourceError> {
        self.inner.kept_native_program(name, parts)
    }

    #[inline]
    fn cached_program_decoded(&mut self) {
        self.inner.cached_program_decoded()
    }

    #[inline]
    fn keep_compiled_program(&mut self, name: &str, parts: &[&str], bytes: &[u8]) {
        self.inner.keep_compiled_program(name, parts, bytes)
    }

    #[inline]
    fn keeps_compiled_programs(&self) -> bool {
        self.inner.keeps_compiled_programs()
    }

    #[inline]
    fn before_native_compilation(&mut self) {
        self.inner.before_native_compilation()
    }

    #[inline]
    fn after_native_compilation(&mut self) {
        self.inner.after_native_compilation()
    }

    #[inline]
    fn kept_bytes_read(bytes: &[u8])
    where
        Self: Sized,
    {
        (kept_bytes_reader())(bytes)
    }

    #[inline]
    fn write_line(&mut self, text: &str) {
        self.inner.write_line(text)
    }

    #[inline]
    fn clock_millis(&mut self) -> f64 {
        self.inner.clock_millis()
    }

    #[inline]
    fn proper_tail_calls(&self) -> bool {
        self.inner.proper_tail_calls()
    }

    #[inline]
    fn allows_natives_syntax(&self) -> bool {
        self.inner.allows_natives_syntax()
    }

    #[inline]
    fn nonextensible_applies_to_private(&self) -> bool {
        self.inner.nonextensible_applies_to_private()
    }

    #[inline]
    fn error_stack_formatter(&self) -> Option<crate::RootId> {
        self.inner.error_stack_formatter()
    }

    #[inline]
    fn functions(&self) -> &[crate::HostFunction<Self>] {
        self.inner.functions()
    }

    #[inline]
    fn initialize(context: &mut crate::NativeContext<'_, Self>) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        let call = context.host_mut().statics.initialize;
        call(context)
    }

    #[inline]
    fn resume(context: &mut crate::NativeContext<'_, Self>) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        let call = context.host_mut().statics.resume;
        call(context)
    }

    #[inline]
    fn initialize_shadow_realm(
        context: &mut crate::NativeContext<'_, Self>,
        global: crate::RootId,
    ) -> Result<Option<crate::RootId>, crate::RootedError>
    where
        Self: Sized,
    {
        let call = context.host_mut().statics.initialize_shadow_realm;
        call(context, global)
    }

    #[inline]
    fn create_promise_context(
        &mut self,
        parent: Option<HostExecutionContext>,
    ) -> Option<HostExecutionContext> {
        self.inner.create_promise_context(parent)
    }

    #[inline]
    fn initialize_promise(
        context: &mut crate::NativeContext<'_, Self>,
        promise: crate::RootId,
        resource: HostExecutionContext,
    ) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        let call = context.host_mut().statics.initialize_promise;
        call(context, promise, resource)
    }

    #[inline]
    fn promise_job_boundary(
        context: &mut crate::NativeContext<'_, Self>,
        resource: HostExecutionContext,
        before: bool,
    ) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        let call = context.host_mut().statics.promise_job_boundary;
        call(context, resource, before)
    }

    #[inline]
    fn promise_settled(
        context: &mut crate::NativeContext<'_, Self>,
        resource: HostExecutionContext,
    ) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        let call = context.host_mut().statics.promise_settled;
        call(context, resource)
    }

    #[inline]
    fn capture_job_context(
        &mut self,
        resource: Option<HostExecutionContext>,
    ) -> Option<HostExecutionContext> {
        self.inner.capture_job_context(resource)
    }

    #[inline]
    fn enter_job_context(
        &mut self,
        context: HostExecutionContext,
    ) -> Option<HostExecutionContext> {
        self.inner.enter_job_context(context)
    }

    #[inline]
    fn restore_job_context(&mut self, previous: Option<HostExecutionContext>) {
        self.inner.restore_job_context(previous)
    }

    #[inline]
    fn enter_unobserved_job(&mut self, entered: bool) -> Option<HostExecutionContext> {
        self.inner.enter_unobserved_job(entered)
    }

    #[inline]
    fn end_unobserved_job(&mut self) -> Vec<crate::RootId> {
        self.inner.end_unobserved_job()
    }

    #[inline]
    fn leave_unobserved_job(&mut self) -> Vec<crate::RootId> {
        self.inner.leave_unobserved_job()
    }

    #[inline]
    fn release_job_context(&mut self, context: HostExecutionContext) -> Vec<crate::RootId> {
        self.inner.release_job_context(context)
    }

    #[inline]
    fn release_promise_contexts(&mut self, contexts: &[HostExecutionContext]) {
        self.inner.release_promise_contexts(contexts)
    }

    #[inline]
    fn call_wasm(
        &mut self,
        function: WasmHostFunctionId,
        args: &[WasmHostValue],
    ) -> Result<Vec<WasmHostValue>, String> {
        self.inner.call_wasm(function, args)
    }

    #[inline]
    fn validate_wasm(&mut self, bytes: &[u8], mode: crate::ExecutionMode) -> bool {
        self.inner.validate_wasm(bytes, mode)
    }

    #[inline]
    fn compile_wasm(
        &mut self,
        bytes: &[u8],
        mode: crate::ExecutionMode,
    ) -> Result<crate::WasmModule, String> {
        self.inner.compile_wasm(bytes, mode)
    }

    #[inline]
    fn globals(&self) -> &'static [HostGlobal] {
        self.inner.globals()
    }

    #[inline]
    fn resolve_dynamic_import(
        &mut self,
        referrer: &str,
        specifier: &str,
    ) -> Result<Option<ModuleSource>, ModuleLoadError> {
        self.inner.resolve_dynamic_import(referrer, specifier)
    }

    #[inline]
    fn validate_module_type(&mut self, module: &ModuleSource, module_type: &str) -> Result<(), ModuleLoadError> {
        self.inner.validate_module_type(module, module_type)
    }

    #[inline]
    fn import_meta_url(&mut self, source_name: &str) -> String {
        self.inner.import_meta_url(source_name)
    }

    #[inline]
    fn import_meta_filename(&mut self, source_name: &str) -> Option<String> {
        self.inner.import_meta_filename(source_name)
    }

    #[inline]
    fn initializes_import_meta(&self) -> bool {
        self.inner.initializes_import_meta()
    }

    #[inline]
    fn initialize_import_meta(
        context: &mut crate::NativeContext<'_, Self>,
        source_name: &str,
        import_meta: crate::RootId,
    ) -> Result<(), crate::RootedError>
    where
        Self: Sized,
    {
        let call = context.host_mut().statics.initialize_import_meta;
        call(context, source_name, import_meta)
    }

    #[inline]
    fn stack_source_name(&mut self, source_name: &str, is_module: bool) -> String {
        self.inner.stack_source_name(source_name, is_module)
    }

    #[inline]
    fn has_module_source(&self, module: &ModuleSource) -> bool {
        self.inner.has_module_source(module)
    }

    #[inline]
    fn done(&mut self, text: Option<&str>) {
        self.inner.done(text)
    }

    #[inline]
    fn can_block(&self) -> bool {
        self.inner.can_block()
    }

}

/// The VM compiled for `ErasedHost` in this crate: each public entry an embedder calls, by
/// address, so its instance (and the VM under it) is generated here and shared with the
/// embedder (`-Z share-generics`) instead of being generated again in the embedder's crate.
#[doc(hidden)]
pub fn erased_host_instances<'a>() -> Vec<usize> {
    #[allow(unused_mut)]
    let mut instances = vec![
        crate::NativeContext::<'a, ErasedHost>::execution_mode as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::execution_interrupt_handle as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::register_termination_waker as fn(_, _) as usize,
        crate::NativeContext::<'a, ErasedHost>::check_execution_interrupt as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::heap_memory_composition as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::scope_root as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::value_type_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::new_target_rooted as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::root_scope_mark as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::release_roots_since as fn(_, _, _) as usize,
        crate::NativeContext::<'a, ErasedHost>::buffer_storage_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::string_text as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::string_code_units as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::string_utf8 as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::string_utf8_length as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::builtins_intact as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::string_units_within as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::prevent_extensions_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::has_property_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::is_symbol_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::is_promise_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::pending_promise_rooted as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::resolve_promise_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::reject_promise_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::promise_snapshot_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::mark_promise_handled_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::has_startup_global_name as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::proxy_target_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::proxy_details_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::original_function_name_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::constructed_name_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::constructor_name_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::collection_preview_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::external_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::external_address_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::has_builtin_brand_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::builtin_data_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::floating_view_values_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::strict_equal_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::source_stack_frames as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::caller_source_expression as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::error_source_position_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::call_stack_source_names as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::instance_of_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::is_array_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::prototype_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::weak_map_get_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::array_like_values_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::own_keys_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::define_property_descriptor_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::own_property_descriptor_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::own_property_fields_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::define_property_fields_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::own_non_index_keys_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::enumerable_own_keys_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::to_string as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::to_string_code_units as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::to_number as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::arithmetic_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::numeric_update_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::subtract_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::greater_than_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::greater_equal_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::bitwise_or_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::iterator_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::iterator_step_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::iterator_close_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::truthy_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::same_value_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::plain_data_equal_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::equal_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::native_execution_unavailable as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_script_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::define_global_lexical_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::create_realm_rooted as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_script_in_realm_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_specialized_script_in_realm_with_origin_rooted as fn(_, _, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::prepare_specialized_script_in_realm_with_origin as fn(_, _, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::compile_script_in_realm_with_origin as fn(_, _, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::compile_script_in_realm_with_origin_owned as fn(_, _, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::compiled_script_is_current as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_compiled_script_in_realm_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::prepared_script_is_current as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_prepared_script_in_realm_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_specialized_script_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_typescript_script_with_origin_rooted as fn(_, _, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_owned_script_with_origin_rooted as fn(_, _, _, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_entry_script_with_origin_rooted as fn(_, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_specialized_script_with_origin_rooted as fn(_, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_specialized_static_body_rooted as fn(_, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_specialized_library_body_rooted as fn(_, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::parse_json_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::stringify_json_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::mark_uncloneable_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::mark_untransferable_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::is_marked_untransferable_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::clone_description_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::serializer_write_value as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::serializer_transfer_array_buffer as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::serializer_release as fn(_, _) as usize,
        crate::NativeContext::<'a, ErasedHost>::deserializer_read_header as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::deserializer_read_value as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::deserializer_transfer_array_buffer as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::deserializer_release as fn(_, _) as usize,
        crate::NativeContext::<'a, ErasedHost>::capture_owned_clone_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::import_owned_clone_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::clone_metadata_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::clone_bigint_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::clone_allocate_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::clone_collection_insert_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::structured_clone_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::require_transfer_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::serialize_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::deserialize_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::structured_clone_with_transfers_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::array_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::boolean as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::symbol_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::private_symbol_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::null as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::error_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::type_error_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::syntax_error_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::range_error_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::is_module as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_function_with_data as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_function_data as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_mut as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::seal_startup_intrinsics as fn(_) as usize,
        crate::NativeContext::<'a, ErasedHost>::startup_data_property_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::materialized_startup_data_property_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::rooted_value as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::array_buffer_aligned_offset_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::uint8_array_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::uint8_array_from_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_weak_root as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::detail_string_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::has_own_property_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::is_error_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_weak_root_is_live as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::resolve_host_weak_root as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::release_host_weak_root as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::collection_epoch as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::view_bytes_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::bytes_prefix_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::is_callable_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::is_object_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::number as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::bigint_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::undefined as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::string_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::decoded_string_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::string_units_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::object_rooted as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::record_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::async_iterator_prototype_rooted as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::null_object_rooted as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::global_root as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_function as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_function_named as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_function_named_with_data as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_constructor as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_constructor_named as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_constructor_named_with_data as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::host_constructor_with_data as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::evaluate_module_sync_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::replace_module_sync_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::module_namespace_with_export_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::get_property_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::plain_enumerable_entries_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::capture_object_template_rooted as fn(_, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::instantiate_object_template_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::get_named_property_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::assign_named_property_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::assign_property_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::delete_property_strict_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::delete_property_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::set_property_rooted as fn(_, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::define_named_data_property_rooted as fn(_, _, _, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::set_named_property_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::define_data_property_rooted as fn(_, _, _, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::pending_lazy_data_property_rooted as fn(_, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::materialize_lazy_data_property_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::is_pending_lazy_data_property_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::define_lazy_data_properties_rooted as fn(_, _, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::define_lazy_data_members_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::define_lazy_holder_keys_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::define_lazy_members_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::set_prototype_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::call_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::take_promise_rejection_handled_events_rooted as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::take_promise_rejection_unhandled_events_rooted as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::mark_promise_rejection_reported as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::clear_kept_objects as fn(_) as usize,
        crate::NativeContext::<'a, ErasedHost>::is_microtask_checkpoint_active as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::run_microtask_checkpoint as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::queue_microtask_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::retain as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::construct_rooted as fn(_, _, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::release_root as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::collect as fn(_) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::objects_with_prototype_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::throw as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::host_function as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::host_constructor as fn(_, _) -> _ as usize,
        crate::CollectionContext::<'a, ErasedHost>::host_mut as fn(_) -> _ as usize,
        crate::CollectionContext::<'a, ErasedHost>::host_weak_root_is_live as fn(_, _) -> _ as usize,
        crate::CollectionContext::<'a, ErasedHost>::release_host_weak_root as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::heap_memory_composition as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::format_rooted_error as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::global_root as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::string_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::string_text_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::detail_string_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::has_own_property_rooted as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::is_error_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::error_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::object_rooted as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::array_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::uint8_array_from_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::null_object_rooted as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::get_property_rooted as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::get_named_property_rooted as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::set_property_rooted as fn(_, _, _, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::set_named_property_rooted as fn(_, _, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::subtract_rooted as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::call_rooted as fn(_, _, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::construct_rooted as fn(_, _, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::new as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::tier_events as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::load_ahead_images as fn(_, _) as usize,
        crate::Runtime::<ErasedHost>::set_execution_mode as fn(_, _) as usize,
        crate::Runtime::<ErasedHost>::compile_wasm_eagerly as fn(_) as usize,
        crate::Runtime::<ErasedHost>::set_memory_policy as fn(_, _) as usize,
        crate::Runtime::<ErasedHost>::memory_policy as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::new_with_execution_interrupt as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::execution_interrupt_handle as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::native_execution_rejections as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::execution_mode as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::end_for_process_exit as fn(_) as usize,
        crate::Runtime::<ErasedHost>::host_mut as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::into_host as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::execute as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::execute_after_scripts as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::compile_entry as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::execute_restored as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::execute_deferred_jobs as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::collection_epoch as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::clear_kept_objects as fn(_) as usize,
        crate::Runtime::<ErasedHost>::run_host_jobs as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::take_promise_rejection_handled_events as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::take_promise_rejection_unhandled_events as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::mark_promise_rejection_reported as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::finish_deferred_execution as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::module_evaluation_pending as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::execute_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::execute_wasm as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::execute_wasm_values as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::wasm_value_matches_type as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::create_wasm_host_reference as fn(_) -> _ as usize,
        crate::Runtime::<ErasedHost>::internalize_wasm_reference as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::externalize_wasm_reference as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::execute_wasm_i32 as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::instantiate_wasm as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::instantiate_wasm_module as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::instantiate_wasm_module_with_imports as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::wasm_memory as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::wasm_tag as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::wasm_table as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::wasm_host_function as fn(_, _, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::invoke_wasm_host_function as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::wasm_function as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::invoke_wasm as fn(_, _, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::invoke_wasm_values as fn(_, _, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::wasm_global as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::wasm_global_binding as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::release_wasm as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::root as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::host_weak_root_is_live as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::resolve_host_weak_root as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::release_host_weak_root as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::update_root as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::release_root as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::rooted_value as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::truthy_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::root_is_live as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::enqueue_rooted_job as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::run_jobs as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::run_jobs_rooted as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::format_error as fn(_, _, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::collect as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::collect_if_due as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::compile_and_execute as fn(_, _) -> _ as usize,
        crate::Runtime::<ErasedHost>::compile_and_execute_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::bigint_words_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::bigint_from_words_rooted as fn(_, _, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::date_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::date_value_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::detach_array_buffer_rooted as fn(_, _) -> _ as usize,
        crate::NativeContext::<'a, ErasedHost>::array_buffer_detached_rooted as fn(_, _) -> _ as usize,
    ];
    #[cfg(feature = "profile-memory")]
    instances.push(crate::NativeContext::<'a, ErasedHost>::profile_memory_checkpoint as fn(_, _) as usize);
    #[cfg(feature = "native-execution")]
    instances.push(crate::Runtime::<ErasedHost>::set_native_program_resolver_boxed as fn(_, _) as usize);
    #[cfg(feature = "native-execution")]
    instances.push(crate::Runtime::<ErasedHost>::execute_native as fn(_, _) -> _ as usize);
    #[cfg(feature = "native-execution")]
    instances.push(crate::Runtime::<ErasedHost>::execute_native_deferred_jobs as fn(_, _) -> _ as usize);
    #[cfg(feature = "native-execution")]
    instances.push(crate::Runtime::<ErasedHost>::execute_native_after_scripts as fn(_, _, _) -> _ as usize);
    #[cfg(feature = "native-execution")]
    instances.push(crate::Runtime::<ErasedHost>::execute_native_module_graph as fn(_, _, _, _) -> _ as usize);
    instances
}
