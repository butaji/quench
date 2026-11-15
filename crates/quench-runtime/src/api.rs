use crate::vm::Vm;
use crate::{Diagnostic, Engine, Host, JsError, ResidualProgram, RootId, Value};

#[path = "api_embedding.rs"]
mod embedding;
pub use embedding::RootedError;
#[path = "api_native.rs"]
mod native;
pub(crate) use native::CompiledScriptState;
pub use native::{
    CollectionContext, BufferStorage, BuiltinBrand, DescriptorFields, ValueType, BuiltinData, CollectionPreview, PromiseSnapshot, HostCallback, HostFunction, HostFunctionDescriptor, HostFunctionId, HostWeakRoot, IntactBuiltin, PreparedScript, NativeArithmetic,
    CompiledScript, NativeContext, RootScopeMark, RootedIterator, ErrorSourcePosition, SourceStackFrame, SynchronousModule,
    ModuleAcceptArguments, ModuleAcceptCallback,
    DeserializerDelegate, SerializerDelegate,
};

/// The syntax context used when compiling source. Script and Module use
/// their OXC parse goals; contextual Eval requires an active guest activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Script,
    Module,
    Eval,
}

/// A named source unit submitted to the specializer.
#[derive(Clone, Copy, Debug)]
pub struct ExecutionRequest<'a> {
    pub source: &'a str,
    pub name: &'a str,
    pub kind: SourceKind,
}

/// A host-visible Promise rejection notification produced at an execution
/// checkpoint. Runtime event methods return owned roots: release each after
/// dispatch. NativeContext event methods instead scope these roots to the
/// callback frame; retain explicitly to carry one beyond that frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromiseRejectionEvent {
    Unhandled {
        id: u64,
        promise: RootId,
        reason: RootId,
    },
    Handled {
        id: u64,
        promise: RootId,
    },
}

impl<'a> ExecutionRequest<'a> {
    pub fn script(source: &'a str, name: &'a str) -> Self {
        Self {
            source,
            name,
            kind: SourceKind::Script,
        }
    }
}

impl Engine {
    /// Compile a source unit through the single OXC-to-residual boundary.
    pub fn compile(request: ExecutionRequest<'_>) -> Result<ResidualProgram, Vec<Diagnostic>> {
        match request.kind {
            SourceKind::Script => Self::specialize(request.source, request.name),
            SourceKind::Module => Self::specialize_module(request.source, request.name),
            SourceKind::Eval => Err(vec![Diagnostic::unsupported(
                request.name,
                "eval compilation requires an activation context",
            )]),
        }
    }
}

/// Explicit compile/execute owner.  Keeping the host and VM together prevents
/// callers from bypassing residual validation or creating an uninitialized VM.
pub struct Runtime<H> {
    vm: Vm<H>,
}

impl<H: Host> Runtime<H> {
    pub fn new(host: H) -> Self { Self { vm: Vm::new(host) } }
    /// What this runtime's tiers did, counted whatever diagnostics are on.
    pub fn tier_events(&self) -> &crate::TierEvents { self.vm.tier_events() }

    /// Takes `images` (compiled ahead from this runtime's program) as its functions' first bodies.
    pub fn load_ahead_images(&mut self, images: crate::AheadImages) { self.vm.load_ahead_images(images); }
    /// Choose how JavaScript and Wasm functions execute; see [`ExecutionMode`].
    pub fn set_execution_mode(&mut self, mode: crate::ExecutionMode) {
        self.vm.set_execution_mode(mode); if mode == crate::ExecutionMode::Compiled { self.vm.enable_lane_methods(); }
    }

    /// Compile each Wasm module whole at its first call under [`crate::ExecutionMode::Compiled`],
    /// instead of starting it in the interpreter and compiling it once it runs long: a
    /// conformance run uses it to exercise every module's compiled code.
    pub fn compile_wasm_eagerly(&mut self) {
        self.vm.compile_wasm_eagerly();
    }

    /// Choose how this runtime trades memory for speed; see [`crate::MemoryPolicy`]. Set it
    /// at startup, before the program runs; a runtime that sets none uses the default.
    pub fn set_memory_policy(&mut self, policy: crate::MemoryPolicy) {
        self.vm.set_memory_policy(policy);
    }

    pub fn memory_policy(&self) -> crate::MemoryPolicy {
        self.vm.memory_policy()
    }

    /// Construct the Vm on this thread with an owned cancellation authority.
    pub fn new_with_execution_interrupt(host: H, interrupt: crate::ExecutionInterrupt) -> Self {
        Self {
            vm: Vm::new_with_execution_interrupt(host, interrupt),
        }
    }

    pub fn execution_interrupt_handle(&self) -> crate::ExecutionInterrupt {
        self.vm.execution_interrupt_handle()
    }

    /// Native-only requests rejected during this runtime's lifetime, including
    /// guest-caught source-compilation errors. A conformance host must not
    /// mistake those capability rejections for expected JavaScript errors.
    pub fn native_execution_rejections(&self) -> u64 {
        self.vm.native_execution_rejections()
    }

    pub fn execution_mode(&self) -> crate::ExecutionMode {
        self.vm.execution_mode()
    }

    /// Ends this runtime for a process that exits next. The host is dropped, so its effects
    /// (flushed output, terminated workers, closed handles) happen as on any drop; the heap
    /// and compiled programs, which hold only this process's memory, are left to the
    /// operating system instead of being freed one allocation at a time.
    pub fn end_for_process_exit(self) {
        self.vm.end_for_process_exit();
    }

    pub fn host_mut(&mut self) -> &mut H {
        &mut self.vm.host
    }

    pub fn into_host(self) -> H {
        self.vm.host
    }

    fn execute_value(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        program.validate_unless_proven().map_err(JsError::validation)?;
        self.vm.execute(program)
    }

    /// Execute without exposing a guest handle to the host.
    pub fn execute(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.execute_value(program).map(drop)
    }

    /// Stage thread-local native artifacts without moving code ownership into
    /// the host. Missing keys defer to Host::kept_native_program; errors do not.
    #[cfg(feature = "native-execution")]
    pub fn set_native_program_resolver(&mut self, resolver: impl Fn(&str, &[&str]) -> Result<Option<crate::NativeJsProgram<H>>, crate::NativeSourceError> + 'static) {
        self.vm.set_native_program_resolver(resolver);
    }

    /// Adopt an already type-erased resolver without allocating another wrapper.
    #[cfg(feature = "native-execution")]
    pub fn set_native_program_resolver_boxed(&mut self, resolver: Box<crate::NativeProgramResolver<H>>) {
        self.vm.set_native_program_resolver_boxed(resolver);
    }

    /// Execute an entry whose known functions are already native, retaining its
    /// code owner for closures and jobs that outlive this call.
    #[cfg(feature = "native-execution")]
    pub fn execute_native(&mut self, program: &crate::NativeJsProgram<H>) -> Result<(), JsError> {
        self.vm.execute_native_program(program, true).map(drop)
    }

    /// Run a native entry while leaving jobs queued for the host event loop's
    /// checkpoints, just as `execute_deferred_jobs` does for a residual entry.
    #[cfg(feature = "native-execution")]
    pub fn execute_native_deferred_jobs(&mut self, program: &crate::NativeJsProgram<H>) -> Result<(), JsError> {
        self.vm.execute_native_program(program, false).map(drop)
    }

    /// Resume canonical bootstrap state for this native program's exact atom namespace,
    /// then run through the shared installed-entry and host-resume path, leaving jobs queued.
    #[cfg(feature = "native-execution")]
    pub fn execute_native_from_snapshot(
        &mut self,
        snapshot: crate::KeptBytes,
        program: &crate::NativeJsProgram<H>,
        read_host: impl FnOnce(&mut H, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String>,
    ) -> Result<(), JsError> {
        self.vm.restore_native_snapshot(snapshot, program, read_host)?;
        self.vm.execute_restored(program.residual()).map(drop)
    }

    /// Run precompiled global Scripts before a native entry in the same realm.
    #[cfg(feature = "native-execution")]
    pub fn execute_native_after_scripts(&mut self, scripts: &[crate::NativeJsProgram<H>], program: &crate::NativeJsProgram<H>) -> Result<(), JsError> {
        self.vm.execute_native_after_scripts(scripts, program).map(drop)
    }

    /// Admit a precompiled dependency graph before global harness Scripts and
    /// the entry run. Native code stays owned by this runtime's module records.
    #[cfg(feature = "native-execution")]
    pub fn execute_native_module_graph(
        &mut self, scripts: &[crate::NativeJsProgram<H>], modules: &[crate::NativeJsProgram<H>], program: &crate::NativeJsProgram<H>,
    ) -> Result<(), JsError> {
        self.vm.execute_native_module_graph(scripts, modules, program).map(drop)
    }

    /// Evaluate each `(source, name)` as a global Script of the entry's realm,
    /// in order, then execute the entry. A module entry and the modules it
    /// loads observe the scripts' global bindings, as with host-run harnesses.
    pub fn execute_after_scripts(
        &mut self,
        scripts: &[(&str, &str)],
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        program.validate_unless_proven().map_err(JsError::validation)?;
        self.vm.execute_after_scripts(scripts, program).map(drop)
    }

    /// Execute the entry while leaving VM jobs queued for the embedder's
    /// event-loop checkpoint. Call `run_host_jobs` after host nextTick work,
    /// then `finish_deferred_execution` once the host loop is complete.
    /// Compiles an application's entry program, as `Engine::compile` does, through the host's
    /// compile cache (`Host::kept_compiled_program`), keyed by the entry's name, parse goal and
    /// text: a program this build kept for them decodes instead, valid as the compiler output
    /// it is. A program compiled here is validated before it is kept.
    pub fn compile_entry(
        &mut self,
        request: ExecutionRequest<'_>,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        if !crate::DYNAMIC_EXECUTION_ENABLED {
            return Err(vec![Diagnostic::unsupported(request.name,
                "native guest code required: entry source compilation is unavailable")]);
        }
        let goal = match request.kind {
            SourceKind::Script => "script",
            SourceKind::Module => "module",
            SourceKind::Eval => return Engine::compile(request),
        };
        let parts = [goal, request.source];
        if let Some(bytes) = self.vm.host.kept_compiled_program(request.name, &parts)
            && let Ok(program) = ResidualProgram::decode_trusted(bytes)
            && program.source_name == request.name
        {
            return Ok(program);
        }
        let program = Engine::compile(request)?;
        if program.validate().is_ok() {
            let bytes = program.encode_trusted();
            self.vm.host.keep_compiled_program(&program.source_name, &parts, &bytes);
            program.validated.establish();
        }
        Ok(program)
    }

    /// Installs the realm and the host for `program` without running it, then writes the VM
    /// as a startup snapshot, with the host's state that `write_host` adds. The host ends its
    /// `initialize` at the point its `resume` continues from.
    pub fn build_snapshot(
        &mut self,
        program: &ResidualProgram,
        write_host: impl FnOnce(&H, &mut crate::SnapshotWriter<'_>) -> Result<(), String>,
    ) -> Result<Vec<u8>, String> {
        program.validate_unless_proven()?;
        self.vm.initialize_for_snapshot(program).map_err(|error| error.to_string())?;
        // A restored heap starts as a full collection leaves it, every cell old: garbage the
        // snapshot kept would stay until the next full collection.
        self.vm.collect_now(program);
        self.vm.write_snapshot(write_host).map_err(|error| error.0)
    }

    /// Makes this new runtime the one a `build_snapshot` wrote into `snapshot`, with `entry`
    /// as its main program and the host state `read_host` reads back. Returns the entry as the
    /// restored runtime runs it, for `execute_restored` and every later call naming it.
    pub fn restore_snapshot(
        &mut self,
        snapshot: crate::KeptBytes,
        entry: &ResidualProgram,
        read_host: impl FnOnce(&mut H, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String>,
    ) -> Result<ResidualProgram, String> {
        entry.validate_unless_proven()?;
        let mode = self.execution_mode();
        let restored = self.vm.restore_snapshot(snapshot, entry, read_host).map_err(|error| error.0)?;
        // A snapshot carries the realm, not the process's tier enrollment: the restored
        // program store starts from the store defaults, which enable no lane body. The mode
        // this runtime was put in is the process's, so it is adopted again here.
        self.set_execution_mode(mode);
        Ok(restored)
    }

    /// `execute_deferred_jobs` for a restored runtime (`restore_snapshot`).
    pub fn execute_restored(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.vm.execute_restored(program).map(drop)
    }

    pub fn execute_deferred_jobs(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        program.validate_unless_proven().map_err(JsError::validation)?;
        self.vm.execute_deferred_jobs(program).map(drop)
    }

    /// Counts collections: a host weak root can die only when this changes.
    pub fn collection_epoch(&self) -> u64 {
        self.vm.collection_epoch()
    }

    /// Release WeakRef targets after the host completes a job and its microtasks.
    pub fn clear_kept_objects(&mut self) {
        self.vm.clear_kept_objects();
    }

    /// Drain VM jobs at a host-selected checkpoint, including module jobs.
    pub fn run_host_jobs(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        program.validate_unless_proven().map_err(JsError::validation)?;
        self.vm.drain_host_jobs(program).map(drop)
    }

    /// Take already-reported Promises that became handled after the previous
    /// checkpoint. Dispatch these before snapshotting newly unhandled Promises.
    pub fn take_promise_rejection_handled_events(&mut self) -> Vec<PromiseRejectionEvent> {
        self.vm.take_promise_rejection_handled_events()
    }

    /// Snapshot still-unhandled Promise rejections after handled notifications
    /// have been dispatched. Returned roots remain valid until released.
    pub fn take_promise_rejection_unhandled_events(&mut self) -> Vec<PromiseRejectionEvent> {
        self.vm.take_promise_rejection_unhandled_events()
    }

    /// Mark one snapshotted rejection reported immediately before host dispatch.
    pub fn mark_promise_rejection_reported(&mut self, promise: RootId) -> Result<(), RootedError> {
        let promise = match self.vm.embedding_value(promise) {
            Ok(promise) => promise,
            Err(error) => return Err(self.retain_error(error)),
        };
        match self.vm.mark_promise_rejection_reported(promise) {
            Ok(()) => Ok(()),
            Err(error) => Err(self.retain_error(error)),
        }
    }

    /// Finish profiling and execution reporting after deferred host work.
    pub fn finish_deferred_execution(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        program.validate_unless_proven().map_err(JsError::validation)?;
        self.vm.finish_deferred_execution(program);
        Ok(())
    }

    /// Query the main module's existing evaluation state after execution/jobs.
    /// The host decides what an unsettled evaluation means when its event loop ends.
    pub fn module_evaluation_pending(&self, program: &ResidualProgram) -> Result<bool, JsError> {
        self.vm.module_evaluation_pending(program)
    }

    /// Execute and retain the result under a generation-checked host root.
    pub fn execute_rooted(&mut self, program: &ResidualProgram) -> Result<RootId, JsError> {
        let value = self.execute_value(program)?;
        Ok(self.root(value))
    }

    /// Invoke a typed Wasm entry in the existing VM, preserving host roots.
    /// The first invocation initializes the VM; `execute` still starts a fresh execution.
    pub fn execute_wasm(
        &mut self,
        function: &crate::WasmFunction,
        args: &[crate::WasmValue],
    ) -> Result<Option<crate::WasmValue>, JsError> {
        self.vm.execute_wasm(function, args)
    }

    /// Invoke a typed Wasm function with its complete result vector.
    pub fn execute_wasm_values(
        &mut self,
        function: &crate::WasmFunction,
        args: &[crate::WasmValue],
    ) -> Result<Vec<crate::WasmValue>, JsError> {
        self.vm.execute_wasm_values(function, args)
    }

    /// Check a live value against an ownerless embedding type without coercion.
    /// Concrete declaration types require their module owner and return false here.
    /// Reference handles follow the usual Runtime rooting contract.
    pub fn wasm_value_matches_type(&self, value: crate::WasmValue, ty: crate::WasmType) -> bool {
        self.vm.wasm_value_matches_type(value, ty)
    }

    /// Allocate a rooted opaque host identity without starting a new guest execution.
    pub fn create_wasm_host_reference(&mut self) -> RootId {
        self.vm.create_wasm_host_reference()
    }

    /// Convert a live host value to the internal anyref hierarchy.
    pub fn internalize_wasm_reference(
        &mut self,
        value: Value,
    ) -> Result<crate::WasmValue, JsError> {
        self.vm
            .decode_wasm_value(value, crate::WasmType::EXTERNREF)?;
        Ok(crate::WasmValue::GcRef(self.vm.wasm_external_conversion(
            crate::wasm::reference::ExternalConversion::Internalize,
            value,
        )))
    }

    /// Recover the original external payload without allocation or identity changes.
    pub fn externalize_wasm_reference(&self, value: crate::WasmValue) -> Result<Value, JsError> {
        let crate::WasmValue::GcRef(value) = value else {
            return Err(JsError::validation(
                "expected internal Wasm reference".into(),
            ));
        };
        self.vm.decode_wasm_value(
            value,
            crate::wasm::reference::ExternalConversion::Externalize.input_type(),
        )?;
        Ok(self.vm.wasm_external_value(value))
    }

    pub fn execute_wasm_i32(
        &mut self,
        function: &crate::WasmI32Function,
        args: &[i32],
    ) -> Result<Option<i32>, JsError> {
        self.vm.execute_wasm_i32(function, args)
    }

    /// Instantiate independent scalar bindings in this runtime's shared heap.
    pub fn instantiate_wasm(
        &mut self,
        function: &crate::WasmFunction,
    ) -> Result<crate::WasmInstance, JsError> {
        self.vm.instantiate_wasm(function)
    }

    /// Instantiate a validated module, including modules with only globals.
    pub fn instantiate_wasm_module(
        &mut self,
        module: &crate::WasmModule,
    ) -> Result<crate::WasmInstance, JsError> {
        self.vm.instantiate_wasm_module(module)
    }

    /// Imports are rooted handles in `module.imports()` declaration order.
    /// The instance retains each original resource; release import roots afterward.
    pub fn instantiate_wasm_module_with_imports(
        &mut self,
        module: &crate::WasmModule,
        imports: &[RootId],
    ) -> Result<crate::WasmInstance, JsError> {
        self.vm
            .instantiate_wasm_module_with_imports(module, imports)
    }

    /// Project a memory's ordinary heap identity. Root it before subsequent VM work.
    pub fn wasm_memory(
        &self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<Value, JsError> {
        self.vm.wasm_memory(instance, index)
    }

    /// Project the original tag identity. Root it before subsequent VM work.
    pub fn wasm_tag(&self, instance: &crate::WasmInstance, index: u32) -> Result<Value, JsError> {
        self.vm.wasm_tag(instance, index)
    }

    /// Project a table's ordinary heap identity. Root it before subsequent VM work.
    pub fn wasm_table(&self, instance: &crate::WasmInstance, index: u32) -> Result<Value, JsError> {
        self.vm.wasm_table(instance, index)
    }

    /// Project the original function identity. Root it before subsequent VM work.
    /// Create a typed native callable owned by this runtime and embedding.
    pub fn wasm_host_function(
        &mut self,
        name: &str,
        id: crate::WasmHostFunctionId,
        signature: crate::WasmSignature,
    ) -> Result<RootId, JsError> {
        let function = self.vm.create_wasm_host_function(
            name,
            crate::heap::WasmHostTarget::Host(id),
            signature,
        )?;
        Ok(self.root(function))
    }

    pub fn invoke_wasm_host_function(
        &mut self,
        root: RootId,
        args: &[crate::WasmValue],
    ) -> Result<Vec<crate::WasmValue>, JsError> {
        self.vm.invoke_wasm_host_function(root, args)
    }

    pub fn wasm_function(
        &mut self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<Value, JsError> {
        self.vm.wasm_function(instance, index)
    }

    pub fn invoke_wasm(
        &mut self,
        instance: &crate::WasmInstance,
        index: u32,
        args: &[crate::WasmValue],
    ) -> Result<Option<crate::WasmValue>, JsError> {
        self.vm.invoke_wasm(instance, index, args)
    }

    pub fn invoke_wasm_values(
        &mut self,
        instance: &crate::WasmInstance,
        index: u32,
        args: &[crate::WasmValue],
    ) -> Result<Vec<crate::WasmValue>, JsError> {
        self.vm.invoke_wasm_values(instance, index, args)
    }

    pub fn wasm_global(
        &self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<crate::WasmValue, JsError> {
        self.vm.wasm_global(instance, index)
    }

    /// Project the global identity for rooted imports and re-exports.
    pub fn wasm_global_binding(
        &self,
        instance: &crate::WasmInstance,
        index: u32,
    ) -> Result<Value, JsError> {
        self.vm.wasm_global_binding(instance, index)
    }

    pub fn release_wasm(&mut self, instance: crate::WasmInstance) -> bool {
        self.release_root(instance.environment)
    }

    pub fn root(&mut self, value: Value) -> RootId {
        self.vm.root(value)
    }

    /// A private weak reference is live only while its target remains reachable.
    pub fn host_weak_root_is_live(&self, reference: HostWeakRoot) -> bool {
        self.vm.embedding_host_weak_value(reference.0).is_some()
    }

    pub fn resolve_host_weak_root(&mut self, reference: HostWeakRoot) -> Option<RootId> {
        self.vm
            .embedding_host_weak_value(reference.0)
            .map(|value| self.vm.root(value))
    }

    pub fn release_host_weak_root(&mut self, reference: HostWeakRoot) -> bool {
        self.vm.release_root(reference.0)
    }

    pub fn update_root(&mut self, root: RootId, value: Value) -> bool {
        self.vm.update_root(root, value)
    }

    pub fn release_root(&mut self, root: RootId) -> bool {
        self.vm.release_root(root)
    }

    /// Read a generation-checked persistent handle belonging to this runtime.
    pub fn rooted_value(&self, root: RootId) -> Option<Value> {
        self.vm.root_value(root)
    }

    /// Apply JavaScript truthiness to a live host root without coercion.
    pub fn truthy_rooted(&self, root: RootId) -> Result<bool, JsError> {
        self.vm.embedding_truthy(root)
    }

    /// Check whether a persistent handle still belongs to this runtime.
    pub fn root_is_live(&self, root: RootId) -> bool {
        self.rooted_value(root).is_some()
    }

    /// Queue a callback using only generation-checked persistent roots. The
    /// queue owns the values until `run_jobs` drains them through the VM.
    pub fn enqueue_rooted_job(&mut self, callback: RootId, args: &[RootId]) -> bool {
        let Some(callback) = self.vm.root_value(callback) else {
            return false;
        };
        let Some(args) = args
            .iter()
            .copied()
            .map(|root| self.vm.root_value(root))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        self.vm.enqueue_job(callback, args);
        true
    }

    fn run_jobs_value(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        program.validate_unless_proven().map_err(JsError::validation)?;
        self.vm.drain_jobs(program)
    }

    /// Drain queued jobs without exposing a guest handle to the host.
    pub fn run_jobs(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.run_jobs_value(program).map(drop)
    }

    /// Drain queued jobs and retain the completion under a host root.
    pub fn run_jobs_rooted(&mut self, program: &ResidualProgram) -> Result<RootId, JsError> {
        let value = self.run_jobs_value(program)?;
        Ok(self.root(value))
    }

    pub fn format_error(&mut self, program: &ResidualProgram, error: &JsError) -> String {
        self.vm.format_error(program, error)
    }

    /// Force the VM's named GC safepoint. The residual program supplies the
    /// frame root maps used when a host asks for collection between calls.
    pub fn collect(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        program.validate_unless_proven().map_err(JsError::validation)?;
        self.vm.collect_now(program);
        Ok(())
    }

    /// The VM's GC safepoint between calls: a collection runs when the VM's own pacing
    /// makes one due, as at a back edge. A host's event-loop turn, which may run no guest
    /// loop, names it so a due collection does not wait for one.
    pub fn collect_if_due(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        program.validate_unless_proven().map_err(JsError::validation)?;
        self.vm.maybe_collect(program);
        Ok(())
    }

    pub fn compile_and_execute(
        &mut self,
        request: ExecutionRequest<'_>,
    ) -> Result<(), RuntimeError> {
        let program = Engine::compile(request).map_err(RuntimeError::Diagnostics)?;
        self.execute(&program).map_err(RuntimeError::Execution)
    }

    /// Compile, execute, and retain the result under a host-owned root.
    pub fn compile_and_execute_rooted(
        &mut self,
        request: ExecutionRequest<'_>,
    ) -> Result<RootId, RuntimeError> {
        let program = Engine::compile(request).map_err(RuntimeError::Diagnostics)?;
        self.execute_rooted(&program)
            .map_err(RuntimeError::Execution)
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    Diagnostics(Vec<Diagnostic>),
    Execution(JsError),
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "api_promise_tests.rs"]
mod promise_tests;

/// The origin a host wrapper's `prefix` gives the source after it: the prefix's line
/// count and its last line's UTF-16 length, which compiler positions and diagnostic
/// excerpts discount. `None` past the compiler's position range.
pub fn wrapper_origin(prefix: &str) -> Option<(u32, u32)> {
    let lines = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count()).ok()?;
    let columns =
        u32::try_from(prefix.rsplit('\n').next().unwrap_or("").encode_utf16().count()).ok()?;
    Some((lines, columns))
}
