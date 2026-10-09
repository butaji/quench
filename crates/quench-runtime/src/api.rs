use crate::vm::Vm;
use crate::{Diagnostic, Engine, Host, JsError, ResidualProgram, RootId, Value};

#[path = "api_embedding.rs"]
mod embedding;
pub use embedding::RootedError;
#[path = "api_native.rs"]
mod native;
pub use native::{HostFunction, HostFunctionId, NativeContext};

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
/// checkpoint. Release every root in the event after dispatching it.
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
    pub fn new(host: H) -> Self {
        Self { vm: Vm::new(host) }
    }

    pub fn host_mut(&mut self) -> &mut H {
        &mut self.vm.host
    }

    pub fn into_host(self) -> H {
        self.vm.host
    }

    fn execute_value(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        program.validate().map_err(JsError::validation)?;
        self.vm.execute(program)
    }

    /// Execute without exposing a guest handle to the host.
    pub fn execute(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.execute_value(program).map(drop)
    }

    /// Execute the entry while leaving VM jobs queued for the embedder's
    /// event-loop checkpoint. Call `run_host_jobs` after host nextTick work,
    /// then `finish_deferred_execution` once the host loop is complete.
    pub fn execute_deferred_jobs(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        program.validate().map_err(JsError::validation)?;
        self.vm.execute_deferred_jobs(program).map(drop)
    }

    /// Drain VM jobs at a host-selected checkpoint, including module jobs.
    pub fn run_host_jobs(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        program.validate().map_err(JsError::validation)?;
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
        program.validate().map_err(JsError::validation)?;
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
        let function = self.vm.create_wasm_host_function(name, id, signature)?;
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
        program.validate().map_err(JsError::validation)?;
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
        program.validate().map_err(JsError::validation)?;
        self.vm.collect_now(program);
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
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Clone, Default)]
    struct Capture(Rc<RefCell<Vec<String>>>);

    impl Host for Capture {
        fn write_line(&mut self, text: &str) {
            self.0.borrow_mut().push(text.into());
        }

        fn globals(&self) -> &'static [crate::HostGlobal] {
            &[crate::HostGlobal {
                name: "$262",
                capability: crate::CapabilityId::CreateRealm,
            }]
        }

        fn clock_millis(&mut self) -> f64 {
            0.0
        }
    }

    fn assert_output_in_execution_modes(source: &str, expected: &[&str]) {
        for (mode, compile) in [
            ("specialized", Engine::specialize as fn(&str, &str) -> _),
            ("unspecialized", Engine::specialize_unspecialized),
        ] {
            let host = Capture::default();
            let view = host.clone();
            let mut runtime = Runtime::new(host);
            let program = compile(source, "regression.js").unwrap();
            if let Err(error) = runtime.execute(&program) {
                panic!(
                    "{mode}: {} (output: {:?})",
                    runtime.format_error(&program, &error),
                    view.0.borrow().as_slice(),
                );
            }
            assert_eq!(view.0.borrow().as_slice(), expected, "{mode}");
        }
    }

    #[test]
    fn regression_dynamic_name_lookup_retains_lexical_fallbacks() {
        assert_output_in_execution_modes(
            r#"
                function factory() {
                    var reads = [];
                    for (let i = 0; i < 2; i++) {
                        reads.push(function reader() {
                            eval('0');
                            print(typeof i); print(i); i += 3; print(i);
                            print(delete i); print(reader === eval('reader'));
                            eval('var i = 9'); print(i); i = 10; print(i);
                            print(delete i); print(i);
                        });
                    }
                    return reads;
                }
                var reads = factory(); $262.gc(); reads[0](); reads[1]();
                function immutableFactory() {
                    const fixed = 7;
                    return function () {
                        eval('0'); print(fixed);
                        try { fixed = 8; } catch (error) { print(error.name); }
                        eval('var fixed = 9'); fixed = 10; print(fixed);
                        print(delete fixed); print(fixed);
                    };
                }
                immutableFactory()();
                var self = function self() {
                    eval('0'); print(typeof self);
                    eval('var self = 4'); print(self); self = 5; print(self);
                    print(delete self); print(typeof self);
                };
                self();
                function parameterFactory(value) {
                    return function () {
                        eval('var value = true');
                        print(typeof value); print(value); print(delete value);
                        print(typeof value); print(value);
                    };
                }
                parameterFactory(1)();
            "#,
            &[
                "number",
                "0",
                "3",
                "false",
                "true",
                "9",
                "10",
                "true",
                "3",
                "number",
                "1",
                "4",
                "false",
                "true",
                "9",
                "10",
                "true",
                "4",
                "7",
                "TypeError",
                "10",
                "true",
                "7",
                "function",
                "4",
                "5",
                "true",
                "function",
                "boolean",
                "true",
                "true",
                "number",
                "1",
            ],
        );
    }

    #[test]
    fn regression_nested_eval_lookup_retains_outer_lexical_bindings() {
        assert_output_in_execution_modes(
            r#"
                var reads = [];
                for (let i = 0; i < 2; i++) {
                    const fixed = i + 7;
                    function middle() {
                        eval('0');
                        return () => {
                            eval('0');
                            print(i); print(fixed);
                            try { fixed = 10; } catch (error) { print(error.name); }
                            i += 2; print(i);
                        };
                    }
                    reads.push(middle());
                }
                $262.gc(); reads[0](); reads[1]();
            "#,
            &["0", "7", "TypeError", "2", "1", "8", "TypeError", "3"],
        );
    }

    #[test]
    fn regression_repeated_class_and_object_evaluations_own_their_home_bindings() {
        assert_output_in_execution_modes(
            r#"
                class Base { constructor() { this.base = 4; } }
                var chain = class extends Base { constructor() { super(); } };
                for (let i = 0; i < 4; i++) {
                    chain = class extends chain { constructor() { super(); } };
                }
                print(new chain().base);
                function factory() {
                    var classes = [], objects = [];
                    let enclosing = 0;
                    for (let i = 0; i < 2; i++) {
                        classes.push(class Named extends Base {
                            #value = i;
                            self() { return Named; }
                            value() { return this.#value + enclosing; }
                        });
                        objects.push({__proto__: {value: i}, read() { return super.value; }});
                        enclosing++;
                    }
                    return [classes, objects];
                }
                var result = factory(), classes = result[0], objects = result[1];
                $262.gc();
                print(new classes[0]().value()); print(new classes[1]().value());
                print(new classes[0]().self() === classes[0]);
                print(new classes[1]().self() === classes[1]);
                print(objects[0].read()); print(objects[1].read());
            "#,
            &["4", "2", "3", "true", "true", "0", "1"],
        );
    }

    #[test]
    fn regression_scope_clones_preserve_catch_bindings_and_residual_plans() {
        let source = r#"
            function captures() {
                var reads = [];
                for (let i = 0; i < 2; i++) {
                    try { throw i; } catch (error) { reads.push(() => [i, error]); }
                }
                return reads;
            }
            var reads = captures();
            $262.gc();
            print(reads[0]().join(',')); print(reads[1]().join(','));
            var call = 0, same;
            for (const binding = {}; call < 2; call++) {
                if (call === 0) same = () => binding;
                else print(same() === binding);
            }
        "#;
        assert_output_in_execution_modes(source, &["0,0", "1,1", "true"]);
        let program = Engine::specialize(source, "scope-clone-binary.js").unwrap();
        assert!(
            program
                .functions
                .iter()
                .any(|function| !function.environment_clones.is_empty())
        );
        let path = std::env::temp_dir().join(format!("quench-scope-clone-{}", std::process::id()));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let host = Capture::default();
        let view = host.clone();
        Runtime::new(host).execute(&decoded).unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["0,0", "1,1", "true"]);
    }

    #[test]
    fn regression_iteration_slots_preserve_enclosing_binding_identity() {
        assert_output_in_execution_modes(
            r#"
                function counter(value) {
                    var readers = [];
                    for (let i = 0; i < 2; i++) {
                        readers.push(() => [i, value, arguments[0]]);
                        value++;
                    }
                    return readers;
                }
                var readers = counter(0);
                $262.gc();
                print(readers[0]().join(',')); print(readers[1]().join(','));
                function nested() {
                    var readers = [], increment;
                    for (let outer = 0; outer < 1; outer++) {
                        increment = () => ++outer;
                        for (let inner = 0; inner < 2; inner++) {
                            readers.push(() => [outer, inner]);
                        }
                        increment();
                        print(readers[0]().join(',')); print(readers[1]().join(','));
                    }
                    return readers;
                }
                readers = nested();
                $262.gc();
                print(readers[0]().join(',')); print(readers[1]().join(','));
                function blocks() {
                    var readers = [], i = 0, increment;
                    let enclosing = 0;
                    while (i < 2) {
                        let local = i++;
                        readers.push(() => [local, enclosing]);
                        enclosing++;
                        increment = () => ++local;
                    }
                    print(increment());
                    return readers;
                }
                readers = blocks();
                $262.gc();
                print(readers[0]().join(',')); print(readers[1]().join(','));
            "#,
            &[
                "0,2,2", "1,2,2", "1,0", "1,1", "1,0", "1,1", "2", "0,2", "2,2",
            ],
        );
    }

    #[test]
    fn regression_iteration_eval_preserves_lexical_precedence() {
        assert_output_in_execution_modes(
            r#"
                function collision() {
                    var readers = [];
                    for (let i = 0; i < 2; i++) readers.push(() => eval('i'));
                    eval('var i = 9');
                    return readers;
                }
                var readers = collision();
                print(readers[0]()); print(readers[1]());
            "#,
            &["0", "1"],
        );
    }

    #[test]
    fn regression_iteration_views_share_function_dynamic_bindings() {
        assert_output_in_execution_modes(
            r#"
                var readers = [], initialize, instance = {}, calls = 0;
                class Base {
                    constructor() { calls++; $262.gc(); return instance; }
                }
                class Derived extends Base {
                    constructor() {
                        for (let i = 0; i < 2; i++) {
                            readers.push(() => [i, this, new.target]);
                        }
                        initialize = () => super();
                    }
                }
                try { new Derived(); } catch (error) { print(error.name); }
                for (var read of readers) {
                    try { read(); } catch (error) { print(error.name); }
                }
                print(initialize() === instance);
                $262.gc();
                for (var read of readers) {
                    var result = read();
                    print(result[0]); print(result[1] === instance); print(result[2] === Derived);
                }
                try { initialize(); } catch (error) { print(error.name); }
                print(calls);
                var erase;
                function factory() {
                    var readers = [];
                    eval('var dynamic = 0');
                    for (let i = 0; i < 2; i++) {
                        readers.push(() => [i, dynamic]);
                        eval('dynamic += 1');
                    }
                    erase = () => eval('delete dynamic');
                    return readers;
                }
                var dynamicReaders = factory();
                $262.gc();
                print(dynamicReaders[0]().join(',')); print(dynamicReaders[1]().join(','));
                print(erase());
                for (var read of dynamicReaders) {
                    try { read(); } catch (error) { print(error.name); }
                }
                "#,
            &[
                "ReferenceError",
                "ReferenceError",
                "ReferenceError",
                "true",
                "0",
                "true",
                "true",
                "1",
                "true",
                "true",
                "ReferenceError",
                "2",
                "0,2",
                "1,2",
                "true",
                "ReferenceError",
                "ReferenceError",
            ],
        );
    }

    #[test]
    fn regression_escaped_arrows_share_constructor_this_initialization() {
        assert_output_in_execution_modes(
            r#"
                var initialize, read, nestedRead, calls = 0, seenTarget;
                var instance = {value: 45};
                class Base {
                    constructor() { calls++; seenTarget = new.target; return instance; }
                }
                class Escaping extends Base {
                    constructor() {
                        initialize = () => super();
                        read = () => this;
                        nestedRead = (() => () => this)();
                    }
                }
                try { new Escaping(); } catch (error) { print(error.name); }
                for (var arrow of [read, nestedRead]) {
                    try { arrow(); } catch (error) { print(error.name); }
                }
                $262.gc();
                print(initialize() === instance);
                print(seenTarget === Escaping);
                print(read() === instance); print(nestedRead() === instance);
                try { initialize(); } catch (error) { print(error.name); }
                print(calls); print(read() === instance);
                class Active extends Base {
                    constructor() {
                        var initialize = () => super(), read = () => this;
                        var enclosing = () => {
                            initialize();
                            print(this === read());
                            print(eval('this') === read());
                        };
                        enclosing();
                        print(this === instance);
                    }
                }
                print(new Active() === instance);
                var ordinaryRead;
                class Independent extends Base {
                    constructor() {
                        function ordinary() { return () => this; }
                        ordinaryRead = ordinary();
                        super();
                    }
                }
                new Independent(); print(ordinaryRead() === undefined);
                $262.gc(); print(read() === instance);
                "#,
            &[
                "ReferenceError",
                "ReferenceError",
                "ReferenceError",
                "true",
                "true",
                "true",
                "true",
                "ReferenceError",
                "2",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_super_constructor_resolution_precedes_argument_evaluation() {
        assert_output_in_execution_modes(
            r#"
                var events = [];
                function Base(value) { events.push('base'); this.value = value; this.target = new.target; }
                class Direct extends Base {
                    constructor() { super((events.push('argument'), Object.setPrototypeOf(Direct, null), 7)); }
                }
                try { print(new Direct().value); } catch (error) { print(error.name); }
                print(events.join(','));
                events = [];
                class Spread extends Base {
                    constructor() {
                        super(...{ [Symbol.iterator]() {
                            events.push('iterator'); Object.setPrototypeOf(Spread, null);
                            var done = false;
                            return { next() {
                                events.push('next');
                                if (done) return {done: true};
                                done = true; return {value: 9, done: false};
                            }};
                        }});
                    }
                }
                try { print(new Spread().value); } catch (error) { print(error.name); }
                print(events.join(','));
                var marker = {}, argumentCalls = 0;
                class Invalid extends Base {
                    constructor() { super((argumentCalls++, (() => { throw marker; })())); }
                }
                Object.setPrototypeOf(Invalid, Math.sin);
                try { new Invalid(); } catch (error) { print(error === marker); }
                print(argumentCalls);
                class InvalidSpread extends Base {
                    constructor() { super(...{[Symbol.iterator]() { throw marker; }}); }
                }
                Object.setPrototypeOf(InvalidSpread, null);
                try { new InvalidSpread(); } catch (error) { print(error === marker); }
                class Checked extends Base {
                    constructor() { super((argumentCalls++, 1)); }
                }
                Object.setPrototypeOf(Checked, Math.sin);
                try { new Checked(); } catch (error) { print(error.name); }
                print(argumentCalls);
                var retained = Base;
                class Replaced extends Base {
                    constructor() { super((Base = function () { throw marker; }, 11)); }
                }
                print(new Replaced().value);
                Base = retained;
                class Implicit extends Base {}
                print(new Implicit(13).value);
                var prototypeReads = 0;
                var target = new Proxy(function Target() {}, {
                    get(target, key, receiver) {
                        if (key === 'prototype') { prototypeReads++; $262.gc(); }
                        return Reflect.get(target, key, receiver);
                    }
                });
                var constructed = Reflect.construct(Implicit, [17], target);
                print(constructed.value); print(prototypeReads); print(constructed.target === target);
                "#,
            &[
                "7",
                "argument,base",
                "9",
                "iterator,next,next,base",
                "true",
                "1",
                "true",
                "TypeError",
                "2",
                "11",
                "13",
                "17",
                "1",
                "true",
            ],
        );
    }

    #[test]
    fn regression_super_constructor_resolution_survives_residual_round_trip() {
        let source = r#"
            function Base(value) { this.value = value; }
            class Direct extends Base {
                constructor() { super((Object.setPrototypeOf(Direct, null), 5)); }
            }
            class Implicit extends Base {}
            print(new Direct().value); print(new Implicit(9).value);
        "#;
        let program = Engine::specialize(source, "super-order-binary.js").unwrap();
        let path = std::env::temp_dir().join(format!("quench-super-order-{}", std::process::id()));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime.execute(&decoded).unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["5", "9"]);
    }

    #[test]
    fn regression_captured_values_and_calls_preserve_runtime_binding_state() {
        assert_output_in_execution_modes(
            r#"
            var captured = 1;
            function read() { return captured; }
            Object.defineProperty(globalThis, 'captured', {
                value: 2
            });
            print(read());
            var target = function () { return 4; };
            function invoke() { return target(); }
            globalThis.target = function () { return 5; };
            print(invoke());
            function lateRead() { return late; }
            try { lateRead(); } catch (error) { print(error.name); }
            const late = 8;
            print(lateRead());
            "#,
            &["2", "5", "ReferenceError", "8"],
        );
    }

    #[test]
    fn regression_instance_initializers_precede_defaults_in_the_class_scope() {
        assert_output_in_execution_modes(
            r#"
            var scope = 'outer', body = 'outer-body', events = [];
            class Base {
                #private = (events.push('private'), 'hello');
                public = (events.push('public'), scope);
                evalRead = (events.push('eval'), eval('scope'));
                bodyRead = body;
                target = new.target;
                evalTarget = eval('new.target');
                read = () => [scope, this, eval('new.target')];
                function = function () {};
                #method() { return 'method'; }
                constructor(scope = (events.push('default'), this.#private),
                            method = (events.push('method-default'), this.#method())) {
                    var body = 'constructor-body';
                    events.push('body');
                    this.value = scope;
                    print(scope); print(method);
                }
            }
            var first = new Base();
            print(events.join(',')); events = [];
            print(first.public); print(first.evalRead); print(first.bodyRead);
            print(first.target === undefined); print(first.evalTarget === undefined);
            print(first.function.name);
            var read = first.read;
            $262.gc(); print(read()[0]); print(read()[1] === first);
            print(read()[2] === undefined);
            class Derived extends Base {
                own = (events.push('derived-field'), this.value);
                constructor(value = (events.push('derived-default'), 'provided')) {
                    events.push('before-super'); super(value);
                    events.push('after-super');
                }
            }
            var second = new Derived();
            print(second.own); print(second.public);
            print(events.join(','));
            var marker = {}, defaultCalls = 0;
            class Abrupt {
                value = (() => { throw marker; })();
                constructor(value = defaultCalls++) {}
            }
            try { new Abrupt(); } catch (error) { print(error === marker); }
            print(defaultCalls);
            class Early extends Base {
                constructor(value = this.public) { super(value); }
            }
            try { new Early(); } catch (error) { print(error.name); }
            "#,
            &[
                "hello",
                "method",
                "private,public,eval,default,method-default,body",
                "outer",
                "outer",
                "outer-body",
                "true",
                "true",
                "function",
                "outer",
                "true",
                "true",
                "provided",
                "method",
                "provided",
                "outer",
                "derived-default,before-super,private,public,eval,method-default,body,derived-field,after-super",
                "true",
                "0",
                "ReferenceError",
            ],
        );
    }

    #[test]
    fn regression_private_updates_share_numeric_and_reference_semantics() {
        assert_output_in_execution_modes(
            r#"
            var receiverReads = 0;
            class Counter {
                #value = '4';
                #big = 3n;
                #method() {}
                static #static = 10;
                read() { return this.#value; }
                post(object = this) { return object.#value++; }
                pre() { return ++this.#value; }
                down() { return this.#value--; }
                big() { print(this.#big++ === 3n); print(--this.#big === 3n); }
                method() { return this.#method++; }
                static down() { return --this.#static; }
                static take(value) {
                    return (() => { receiverReads++; return value; })().#value++;
                }
            }
            var counter = new Counter();
            print(counter.post()); print(counter.read());
            print(counter.pre()); print(counter.down()); print(counter.read());
            counter.big();
            try { counter.method(); } catch (error) { print(error.name); }
            try { counter.post(new Proxy(counter, {})); } catch (error) { print(error.name); }
            Object.freeze(counter);
            print(counter.post()); print(counter.read());
            print(Counter.down());
            print(Counter.take(counter)); print(receiverReads); print(counter.read());
            var events = [], stored = 9n;
            class Accessor {
                get #value() {
                    events.push('get');
                    return {[Symbol.toPrimitive]() { events.push('convert'); $262.gc(); return stored; }};
                }
                set #value(value) { events.push('set'); $262.gc(); stored = value; }
                post() { return this.#value++; }
                pre() { return ++this.#value; }
            }
            var accessor = Object.freeze(new Accessor());
            print(accessor.post() === 9n); print(stored === 10n);
            print(accessor.pre() === 11n); print(stored === 11n);
            print(events.join(','));
            "#,
            &[
                "4",
                "5",
                "6",
                "6",
                "5",
                "true",
                "true",
                "TypeError",
                "TypeError",
                "5",
                "6",
                "9",
                "6",
                "1",
                "7",
                "true",
                "true",
                "true",
                "true",
                "get,convert,set,get,convert,set",
            ],
        );
    }

    #[test]
    fn regression_generator_activation_binds_this_and_new_target() {
        assert_output_in_execution_modes(
            r#"
            function Owner() {
                print((() => eval('new.target'))() === Owner);
                this.sync = function* (read = () => new.target) {
                    print(read() === undefined);
                    yield () => [this, new.target, eval('new.target')];
                };
                this.async = async function* (read = () => new.target) {
                    print(read() === undefined);
                    yield () => [this, new.target, eval('new.target')];
                };
            }
            var owner = new Owner();
            var iterator = owner.sync();
            var read = iterator.next().value;
            $262.gc();
            var values = read();
            print(values[0] === owner);
            print(values[1] === undefined);
            print(values[2] === undefined);
            iterator.next();
            print(read()[0] === owner);
            var strict = function* () { 'use strict'; yield () => this; };
            print(strict.call(7).next().value() === 7);
            var asyncIterator = owner.async();
            asyncIterator.next().then(function (result) {
                $262.gc();
                var read = result.value;
                var values = read();
                print(values[0] === owner);
                print(values[1] === undefined);
                print(values[2] === undefined);
                return asyncIterator.next().then(function () {
                    print(read()[0] === owner);
                });
            });
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true",
            ],
        );
    }

    #[test]
    fn regression_suspended_with_scopes_survive_gc_and_closure_capture() {
        assert_output_in_execution_modes(
            r#"
var x="outer",saved;function* f(){with({x:"inner"}){saved=()=>x;try{yield x;yield eval("x");}finally{$262.gc();print(saved());print(x);}}}var g=f();print(g.next().value);$262.gc();print(g.next().value);$262.gc();print(g.return("done").value);print(saved());print(x);
var x="outer";async function asyncRun(){with({x:"inner"}){await 0;$262.gc();print(x);await 0;print(eval("x"));}}asyncRun();$262.gc();print(x);
            "#,
            &[
                "inner", "inner", "inner", "inner", "done", "inner", "outer", "outer", "inner",
                "inner",
            ],
        );
    }

    #[test]
    fn regression_with_binding_key_survives_collecting_unscopables() {
        assert_output_in_execution_modes(
            r#"
            var keys = [];
            var object = new Proxy({x: 3}, {
                has(target, key) { keys.push(String(key)); return Reflect.has(target, key); },
                get(target, key, receiver) {
                    if (key === Symbol.unscopables) { $262.gc(); return {}; }
                    return Reflect.get(target, key, receiver);
                }
            });
            var result;
            with (object) { result = x; }
            print(result);
            print(keys.join(','));
            "#,
            &["3", "result,x,x"],
        );
    }

    #[test]
    fn regression_dynamic_eval_bindings_survive_yield_and_await() {
        assert_output_in_execution_modes(
            r#"
            function* syncGenerator() {
                eval('var retained = { value: 42 }');
                yield 0;
                eval('var afterResume = 1');
                $262.gc();
                print(retained.value);
            }
            var iterator = syncGenerator();
            iterator.next();
            $262.gc();
            iterator.next();
            async function asyncFunction() {
                eval('var retained = { value: 43 }');
                await 0;
                eval('var afterResume = 1');
                $262.gc();
                print(retained.value);
            }
            asyncFunction();
            async function* asyncGenerator() {
                eval('var retained = { value: 44 }');
                yield 0;
                await 0;
                eval('var afterResume = 1');
                $262.gc();
                print(retained.value);
            }
            var asyncIterator = asyncGenerator();
            asyncIterator.next().then(function () {
                $262.gc();
                return asyncIterator.next();
            });
            "#,
            &["42", "43", "44"],
        );
    }

    #[test]
    fn regression_eval_preserves_strict_context_for_nested_function_expressions() {
        assert_output_in_execution_modes(
            r#"
            var sink = {};
            var outcomes = [];
            for (var binding of ['eval', 'arguments']) {
                var source = 'sink.fn = function ' + binding + '() {};';
                for (var mode = 0; mode < 3; mode++) {
                    try {
                        if (mode === 0) eval("'use strict'; " + source);
                        else if (mode === 1) {
                            (function () { 'use strict'; eval(source); })();
                        } else (0, eval)("'use strict'; " + source);
                        outcomes.push(false);
                    } catch (error) {
                        outcomes.push(error instanceof SyntaxError);
                    }
                    outcomes.push(Object.hasOwn(sink, 'fn'));
                }
            }
            print(outcomes.join(','));
            eval('sink.fn = function eval() {};');
            print(sink.fn.name);
            print(eval("'use strict'; (function () { return this; })()") === undefined);
            (function () {
                'use strict';
                print(eval('(function () { return this; })()') === undefined);
            })();
            print(eval("'use strict'; sink.eval = 1; sink.eval"));
            (function () {
                'use strict';
                print(eval('sink.arguments = 2; sink.arguments'));
            })();
            "#,
            &[
                "true,false,true,false,true,false,true,false,true,false,true,false",
                "eval",
                "true",
                "true",
                "1",
                "2",
            ],
        );
    }

    #[test]
    fn regression_strict_assignment_checks_distinguish_bindings_and_properties() {
        assert_output_in_execution_modes(
            r#"
            for (var source of [
                'eval = 1;', 'eval++;', '++arguments;',
                '\\u0065val = 1;', '({ value: eval } = { value: 1 });'
            ]) {
                try {
                    eval("'use strict'; " + source);
                    print(false);
                } catch (error) {
                    print(error instanceof SyntaxError);
                }
            }
            print(eval("'use strict'; ({ eval: 1, arguments: 2 }).eval"));
            print(eval("'use strict'; /eval=/.source"));
            "#,
            &["true", "true", "true", "true", "true", "1", "eval="],
        );
    }

    #[test]
    fn regression_derived_constructors_preserve_the_actual_superclass() {
        assert_output_in_execution_modes(
            r#"
            class Base { get answer() { return 42; } }
            class Derived extends Base {}
            print(Object.getPrototypeOf(Derived) === Base);
            print(Object.getPrototypeOf(Derived.prototype) === Base.prototype);
            print(new Derived().answer);
            class Explicit extends Base {
                constructor() {
                    super();
                    print(this === (() => this)());
                    print(this === eval('this'));
                    print((() => { eval(''); return () => super.answer; })()());
                }
            }
            new Explicit();
            "#,
            &["true", "true", "42", "true", "true", "42"],
        );
    }

    #[test]
    fn regression_error_results_survive_collecting_message_and_cause() {
        assert_output_in_execution_modes(
            r#"
            for (var Constructor of [Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError]) {
                for (var mode of ['call', 'construct']) {
                    var marker = {rank: 7};
                    var message = {toString() {$262.gc(); return 'message';}};
                    var options = {get cause() {$262.gc(); return marker;}};
                    var error = mode === 'call' ? Constructor(message, options) : new Constructor(message, options);
                    print(error.message);
                    print(error.cause === marker);
                    print(Object.getOwnPropertyDescriptor(error, 'message').enumerable);
                }
            }
            var marker = {rank: 8};
            var suppressed = {rank: 9};
            var error = new SuppressedError(marker, suppressed, {toString() {$262.gc(); return 'suppressed';}});
            print(error.message);
            print(error.error === marker);
            print(error.suppressed === suppressed);
            "#,
            &[
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "suppressed",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_intrinsic_prototype_fallback_ignores_replaced_globals() {
        assert_output_in_execution_modes(
            r#"
            var OriginalObject = Object, OriginalArray = Array, OriginalRegExp = RegExp;
            var objectPrototype = Object.prototype, arrayPrototype = Array.prototype, regexpPrototype = RegExp.prototype;
            var reads = 0;
            for (var name of ['Object', 'Array', 'RegExp']) {
                OriginalObject.defineProperty(globalThis, name, {configurable: true, get() {reads++; $262.gc(); return {prototype: {}};}});
            }
            function Target() {}
            Target.prototype = null;
            print(OriginalObject.getPrototypeOf(new Target()) === objectPrototype);
            print(OriginalObject.getPrototypeOf(Reflect.construct(OriginalArray, [], Target)) === arrayPrototype);
            print(OriginalObject.getPrototypeOf(Reflect.construct(OriginalRegExp, [], Target)) === regexpPrototype);
            print(OriginalObject.getPrototypeOf(new AggregateError([]).errors) === arrayPrototype);
            print(reads);
            "#,
            &["true", "true", "true", "true", "0"],
        );
    }

    #[test]
    fn regression_foreign_intrinsic_prototypes_survive_global_replacement() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm();
            var objectPrototype = foreign.global.Object.prototype;
            var arrayPrototype = foreign.global.Array.prototype;
            var regexpPrototype = foreign.global.RegExp.prototype;
            var functionPrototype = foreign.global.Function.prototype;
            foreign.evalScript('globalThis.Target = function Target() {}; Target.prototype = null;');
            var Target = foreign.global.Target;
            var reads = 0;
            for (var name of ['Object', 'Array', 'RegExp', 'Function']) {
                Object.defineProperty(foreign.global, name, {configurable: true, get() {reads++; $262.gc(); return {prototype: {}};}});
            }
            $262.gc();
            print(Object.getPrototypeOf(new Target()) === objectPrototype);
            print(Object.getPrototypeOf(Reflect.construct(Array, [], Target)) === arrayPrototype);
            print(Object.getPrototypeOf(Reflect.construct(RegExp, [], Target)) === regexpPrototype);
            foreign.evalScript('globalThis.fresh = function fresh() {};');
            print(Object.getPrototypeOf(foreign.global.fresh) === functionPrototype);
            print(Object.getPrototypeOf(foreign.global.fresh.prototype) === objectPrototype);
            print(reads);
            "#,
            &["true", "true", "true", "true", "true", "0"],
        );
    }

    #[test]
    fn regression_dynamic_functions_survive_collecting_prototype_getters() {
        assert_output_in_execution_modes(
            r#"
            var constructors = [Function, (async function() {}).constructor, (function*() {}).constructor, (async function*() {}).constructor];
            for (var Constructor of constructors) {
                var prototype = {rank: 42};
                var reads = 0;
                var target = new Proxy(function Target() {}, {get(object, key, receiver) {
                    if (key === 'prototype') {reads++; $262.gc(); return prototype;}
                    return Reflect.get(object, key, receiver);
                }});
                var result = Reflect.construct(Constructor, ['return 42'], target);
                print(typeof result);
                print(Object.getPrototypeOf(result) === prototype);
                print(Function.prototype.toString.call(result).includes('return 42'));
                print(reads);
            }
            "#,
            &[
                "function", "true", "true", "1", "function", "true", "true", "1", "function",
                "true", "true", "1", "function", "true", "true", "1",
            ],
        );
    }

    #[test]
    fn regression_dynamic_function_fallback_uses_foreign_intrinsics() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm();
            foreign.evalScript('globalThis.kinds = [Function, (async function() {}).constructor, (function*() {}).constructor, (async function*() {}).constructor]; globalThis.Target = function Target() {};');
            var constructors = [Function, (async function() {}).constructor, (function*() {}).constructor, (async function*() {}).constructor];
            var prototypes = foreign.global.kinds.map(function(Constructor) {return Constructor.prototype;});
            var reads = 0;
            for (var name of ['Function', 'AsyncFunction', 'GeneratorFunction', 'AsyncGeneratorFunction']) {
                Object.defineProperty(foreign.global, name, {configurable: true, get() {reads++; $262.gc(); return {prototype: {}};}});
            }
            for (var index = 0; index < constructors.length; index++) {
                var targetReads = 0;
                var target = new Proxy(foreign.global.Target, {get(object, key, receiver) {
                    if (key === 'prototype') {targetReads++; $262.gc(); return null;}
                    return Reflect.get(object, key, receiver);
                }});
                var result = Reflect.construct(constructors[index], ['return 42'], target);
                print(Object.getPrototypeOf(result) === prototypes[index]);
                print(targetReads);
            }
            print(reads);
            for (var Constructor of constructors) {
                var marker = {};
                var target = new Proxy(function Target() {}, {get(object, key, receiver) {
                    if (key === 'prototype') {$262.gc(); throw marker;}
                    return Reflect.get(object, key, receiver);
                }});
                try {Reflect.construct(Constructor, ['return 42'], target); print(false);}
                catch (error) {print(error === marker);}
            }
            "#,
            &[
                "true", "1", "true", "1", "true", "1", "true", "1", "0", "true", "true", "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_eval_created_bindings_shadow_hoisted_captures() {
        assert_output_in_execution_modes(
            r#"
            var reference = 99;
            function owner() {
                function write() { 'use strict'; reference = 13; }
                function strictWrite() { 'use strict'; reference += 1; }
                function sloppyWrite() { with ({}) { reference = 15; } }
                eval('var reference = 1');
                write(); print(reference);
                strictWrite(); print(reference);
                sloppyWrite(); print(reference);
            }
            owner(); print(reference);
            "#,
            &["13", "14", "15", "99"],
        );
    }

    #[test]
    fn regression_eval_created_bindings_share_captured_storage() {
        assert_output_in_execution_modes(
            r#"
            var value = 42;
            function owner() {
                eval('var value = 5');
                var access = eval('(function(action) { if (action === "set") value = 9; return value; })');
                print(access()); print(value);
                value = 8;
                print(access()); print(value);
                access('set');
                print(access()); print(value);
                eval('var another = 1');
                print(access()); print(value);
                function shadow() {var value = 41; return eval('value');}
                print(shadow());
                print(access('delete'));
                return [access, () => value, () => eval('delete value')];
            }
            var escaped = owner();
            $262.gc();
            print(escaped[0]('set')); print(escaped[1]());
            print(escaped[2]()); print(escaped[1]());
            print(value);
            function* generator() {
                eval('var retained = 11');
                var read = () => retained;
                yield read;
                retained = 12;
                yield read;
            }
            var iterator = generator();
            var read = iterator.next().value;
            print(read()); iterator.next(); print(read());
            "#,
            &[
                "5", "5", "8", "8", "9", "9", "9", "9", "41", "9", "9", "9", "true", "42", "42",
                "11", "12",
            ],
        );
    }

    #[test]
    fn regression_eval_var_reuses_existing_activation_binding() {
        assert_output_in_execution_modes(
            r#"
            var globalValue = 17;
            function existing(value) {
                var read = eval('var value = 4; (function() {return value;})');
                print(read()); print(value);
                value = 7;
                print(read()); print(value);
                eval('value = 9');
                print(read()); print(value);
                print(eval('delete value'));
                return read;
            }
            var escaped = existing(2);
            print(escaped()); print(globalValue);
            function initialized() {
                var value;
                eval('var value');
                value = 11;
                print(eval('value'));
            }
            initialized();
            function nested() {
                var value = 'outer';
                print(eval('var value = "inner"; eval("value")'));
                print(value);
            }
            nested();
            var arrow = (p = eval('var arguments = "param"'), q = () => arguments) => {
                var arguments = 'local';
                print(q()); print(arguments);
            };
            arrow();
            "#,
            &[
                "4", "4", "7", "7", "9", "9", "false", "9", "17", "11", "inner", "inner", "param",
                "local",
            ],
        );
    }

    #[test]
    fn regression_replacement_length_errors_preserve_effects_and_utf16() {
        let source = r#"var large='x'.repeat(1<<20), template='$1'.repeat(1<<15);
function throwsRange(action){try{action();return false;}catch(error){return error instanceof RangeError;}}
print(throwsRange(()=>large.replace(/(.+)/g,template)));
print(throwsRange(()=>large.replaceAll(/(.+)/g,template)));
print(throwsRange(()=>large.replace(large,'$&'.repeat(1<<15))));
print(throwsRange(()=>large.replaceAll(large,'$&'.repeat(1<<15))));
var events=[],reason={},groups={get end(){events.push('get');$262.gc();throw reason;}}, rx={flags:'',exec(){return {0:large,1:large,length:2,index:0,groups};}};
try{RegExp.prototype[Symbol.replace].call(rx,large,template+'$<end>');print(false);}catch(error){print(error===reason && events.join()==='get');}
var count=0;
print(throwsRange(()=>'x'.repeat(600).replace(/x/g,()=>{count++;return large;})));
print(count===600);
print('abc'.replace(/(b)/,'$1|$01|$10|$2|$&|$`|$\'|$$')==='ab|b|b0|$2|b|a|c|$c');
print('abc'.replace(/(?<letter>b)/,'$<letter>|$<missing>|$<unterminated')==='ab||$<unterminatedc');
print('ab'.replaceAll('','$&|')==='|a|b|');
print('abc'.replace('b','$`-$&-$\'')==='aa-b-cc');
print('abc'.replaceAll('b',()=>({toString(){$262.gc();return '\ud800';}}))==='a\ud800c');
print('\ud800|\udc00'.replace(/\|/,'$`$$$\'')==='\ud800\ud800$\udc00\udc00');
var calls=0;
print('abc'.replace(/b/g,(value,index,input)=>{calls++;$262.gc();return value+index+input;})==='ab1abcc' && calls===1);
var namedGets=0,ng={get z(){namedGets++;$262.gc();return '\ud800';}}, custom={flags:'',exec(){return {0:'b',length:1,index:1,groups:ng};}};
print(RegExp.prototype[Symbol.replace].call(custom,'abc','$<z>$<z>')==='a\ud800\ud800c' && namedGets===2);
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "string-growth.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-string-growth-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                Runtime::new(host).execute(&program).unwrap();
                assert_eq!(output.0.borrow().as_slice(), &["true"; 15]);
            }
        }
    }

    #[test]
    fn regression_numeric_date_parsing_preserves_iso_and_legacy_boundaries() {
        let source = r#"var cases=[["1997-03-08 1:1:1.01", [1997, 2, 8, 1, 1, 1, 10]], ["1997-03-08 11:19:20", [1997, 2, 8, 11, 19, 20, 0]], ["1997-3-08 11:19:20", [1997, 2, 8, 11, 19, 20, 0]], ["1997-3-8 11:19:20", [1997, 2, 8, 11, 19, 20, 0]], ["+001997-3-8 11:19:20", [1997, 2, 8, 11, 19, 20, 0]], ["+001997-03-8 11:19:20", [1997, 2, 8, 11, 19, 20, 0]], ["1997-03-08 11:19", [1997, 2, 8, 11, 19, 0, 0]], ["1997-03-08 1:19", [1997, 2, 8, 1, 19, 0, 0]], ["1997-03-08 1:1", [1997, 2, 8, 1, 1, 0, 0]], ["1997-03-08 1:1:01", [1997, 2, 8, 1, 1, 1, 0]], ["1997-03-08 1:1:1", [1997, 2, 8, 1, 1, 1, 0]], ["1997-03-08 11", "NaN"], ["1997-03-08 11:19:10-07", 857845150000], ["1997-03-08 11:19:10-0700", 857845150000], ["1997-03-08T11:19:10-07", "NaN"], ["1997-03-08T", "NaN"], ["1997-3-8T11:19:20", "NaN"], ["1997-03-8T11:19:20", "NaN"], ["+001997-3-8T11:19:20", "NaN"], ["1997-03-08T1:19", "NaN"], ["1997-03-08T1:1", "NaN"], ["1997-03-08T1:1:01", "NaN"], ["1997-03-08T1:1:1", "NaN"], ["1997-03-08T11:19:10-0700", 857845150000], ["1997-03-08 11:19:10-7", 857845150000], ["1997-03-08 11:19:10-7:0", 857845150000], ["1997-03-08 11:19:10-0799", 857851090000], ["1997-03-08 11:19:10-007", 857820370000], ["1997-03-08T11:19:10+24:00", "NaN"], ["1997-03-08T24:00:00.001", "NaN"], ["1997-03-08 24:00", [1997, 2, 9, 0, 0, 0, 0]], ["1997-03-08 24:00:00.0001", [1997, 2, 9, 0, 0, 0, 0]], ["1997-03-08T24:00:00.0001", "NaN"], ["1997-03-08 1:1:1.", "NaN"], ["1997-03-08T01:01:01.", "NaN"], ["1997-03-08T01:01:01.0001", [1997, 2, 8, 1, 1, 1, 0]], ["1997-13-08", "NaN"], ["1997-02-30", 857260800000], ["1997-03-08t11:19:20z", 857819960000], ["1997-03-08T11:19:20Z", 857819960000], ["1997-3-8", [1997, 2, 8, 0, 0, 0, 0]], ["1997-03-08", 857779200000], ["1997-00-08T11:00", "NaN"], ["1997-03-32T11:00", "NaN"], ["1997-03-08T11:60", "NaN"], ["1997-03-08T11:19:60", "NaN"], ["1997-03-08T11:19:00+00:60", "NaN"], ["1997-03-08T11:19:00+1\u00e92", "NaN"], ["1997-03-08T11:19:00.1x", "NaN"], ["1997-03-08T11:19:00+04:30", 857803740000], ["1997-03-08T11:19:00-04:30", 857836140000], ["+001997-03-08T11:19:20", [1997, 2, 8, 11, 19, 20, 0]], ["-000000-03-08T11:19:20", "NaN"]];
for(var pair of cases){var expected=Array.isArray(pair[1])?new Date(...pair[1]).getTime():pair[1]==="NaN"?NaN:pair[1];print(Object.is(Date.parse(pair[0]),expected));print(Object.is(new Date(pair[0]).getTime(),expected));}
var calls=0,hint,source={ [Symbol.toPrimitive](h){calls++;hint=h;$262.gc();return "1997-03-08 1:1:1.01";}};print(Date.parse(source)===Date.parse(cases[0][0]) && calls===1 && hint==="string");print(new Date(source).getTime()===Date.parse(cases[0][0]) && calls===2 && hint==="default");
var reason={};try{Date.parse({toString(){throw reason;}});print(false);}catch(e){print(e===reason);}
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "date-parse.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-date-parse-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                Runtime::new(host).execute(&program).unwrap();
                assert_eq!(output.0.borrow().as_slice(), &["true"; 109]);
            }
        }
    }

    #[test]
    fn regression_source_text_modules_reject_source_imports_before_execution() {
        let source = r#"async function checkSources(){
  globalThis.qq_source_runs=0;globalThis.qq_bad_runs=0;globalThis.qq_effect_runs=0;
  var leaf='./task20-module-source-fixtures/leaf.mjs', reasons=[];
  for(var i=0;i<2;i++){var promise=import.source(leaf);print(promise instanceof Promise);try{await promise;print(false);}catch(e){$262.gc();print(e instanceof SyntaxError);reasons.push(e);}}
  print(reasons[0]!==reasons[1]);print(qq_source_runs===0);
  for(var file of ['bad','mixed','parent','reexport']){
    var path='./task20-module-source-fixtures/'+file+'.mjs', first;
    try{await import(path);print(false);}catch(e){$262.gc();print(e instanceof SyntaxError);first=e;}
    try{await import(path);print(false);}catch(e){print(e===first);}
    print(qq_source_runs===0 && qq_bad_runs===0 && qq_effect_runs===0);
  }
  var ns=await import(leaf);print(ns.answer===42 && qq_source_runs===1);ns.bump();print(ns.answer===43);print(await import(leaf)===ns);
  try{await import.source(leaf);print(false);}catch(e){print(e instanceof SyntaxError);}print(qq_source_runs===1);
  var events=[], specifier={toString(){events.push('specifier');$262.gc();return leaf;}};
  try{await import.source(specifier);print(false);}catch(e){print(e instanceof SyntaxError);}print(events.join('|')==='specifier');
  var reason={};try{await import.source({toString(){throw reason;}});print(false);}catch(e){print(e===reason);}
  var proto=$262.AbstractModuleSource.prototype, getter=Object.getOwnPropertyDescriptor(proto,Symbol.toStringTag).get;
  for(var value of [undefined,null,1,{},Object.create(proto)])print(getter.call(value)===undefined);
}
checkSources().then(()=>print('done'),e=>{print('unexpected');print(e);});
"#;
        const MODULES: &[(&str, &str)] = &[
            (
                "bad.mjs",
                "import source x from './leaf.mjs';globalThis.qq_bad_runs++;\n",
            ),
            (
                "effect.mjs",
                "globalThis.qq_effect_runs++;export const value=43;\n",
            ),
            (
                "leaf.mjs",
                "globalThis.qq_source_runs++;export let answer=42;export function bump(){answer++;}\n",
            ),
            (
                "mixed.mjs",
                "import './effect.mjs';import source x from './leaf.mjs';globalThis.qq_bad_runs++;\n",
            ),
            (
                "parent.mjs",
                "import './bad.mjs';globalThis.qq_bad_runs++;\n",
            ),
            (
                "reexport.mjs",
                "import source x from './leaf.mjs';export {x};\n",
            ),
        ];
        struct ModuleCapture(Capture);
        impl Host for ModuleCapture {
            fn write_line(&mut self, text: &str) {
                self.0.write_line(text);
            }
            fn clock_millis(&mut self) -> f64 {
                0.0
            }
            fn globals(&self) -> &'static [crate::HostGlobal] {
                self.0.globals()
            }
            fn resolve_dynamic_import(
                &mut self,
                _: &str,
                specifier: &str,
            ) -> Result<Option<crate::host::ModuleSource>, String> {
                let file = specifier.rsplit('/').next().unwrap();
                Ok(MODULES
                    .iter()
                    .find(|(name, _)| *name == file)
                    .map(|(_, source)| crate::host::ModuleSource {
                        name: format!("task20-module-source-fixtures/{file}"),
                        source: (*source).into(),
                        bytes: source.as_bytes().to_vec(),
                    }))
            }
        }
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "module-source-probe.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-module-source-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(ModuleCapture(host));
                runtime.execute(&program).unwrap();
                let mut expected = vec!["true"; 31];
                expected.push("done");
                assert_eq!(output.0.borrow().as_slice(), expected.as_slice());
            }
        }
        for compile in [
            Engine::specialize_module as fn(&str, &str) -> _,
            Engine::specialize_module_unspecialized,
        ] {
            let program = compile(
                "import source x from './task20-module-source-fixtures/leaf.mjs';print('unexpected root execution');",
                "module-source-root.mjs",
            ).unwrap();
            let path = std::env::temp_dir()
                .join(format!("quench-module-source-root-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(ModuleCapture(host));
                let error = runtime.execute(&program).unwrap_err();
                assert!(
                    runtime
                        .format_error(&program, &error)
                        .starts_with("SyntaxError:")
                );
                assert!(output.0.borrow().is_empty());
            }
        }
    }

    #[test]
    fn regression_let_labels_share_identifier_syntax_and_strictness() {
        let source = r#"for(var label of ['let','l\\u0065t','l\\u{65}t']){
  var block=label+':{marker=43;break '+label+';marker=99;}';
  print(Function('var marker=0;'+block+'return marker;')()===43);
  print((function(){var marker=0;eval(block);return marker;})()===43);
  print((0,eval)('var marker=0;'+block+'marker;')===43);delete globalThis.marker;
  var loop=label+':for(var i=0;i<3;i++){marker++;continue '+label+';marker=99;}';
  print(Function('var marker=0;'+loop+'return marker;')()===3);
  for(var prefix of ['"use strict";','"use strict"\n']){
    var source=prefix+label+':42;';
    try{Function(source);print(false);}catch(e){print(e instanceof SyntaxError);}
    try{eval(source);print(false);}catch(e){print(e instanceof SyntaxError);}
    try{(0,eval)(source);print(false);}catch(e){print(e instanceof SyntaxError);}
  }
  print(Function('if(true) '+label+': {return 47;}')()===47);
  try{Function(label+':'+label+':;');print(false);}catch(e){print(e instanceof SyntaxError);}
}
print(Function('let /* comment */ : {return 53;}')()===53);
print(Function('let\n: {return 59;}')()===59);
print(Function('let x=61; return x;')()===61);
print(Function('var let=2; let+=3; return let;')()===5);
print(Function('let x=2; {let x=3;} return x;')()===2);
print((function(){'use strict';try{eval('let:42');return false;}catch(e){return e instanceof SyntaxError;}})());
try{Function('let: let x=3;');print(false);}catch(e){print(e instanceof SyntaxError);}
print(Function('var let=2; let.foo=3; let( );') instanceof Function);
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "let-label.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-let-label-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(output.0.borrow().as_slice(), &["true"; 44]);
            }
        }
    }

    #[test]
    fn regression_async_disposal_sync_fallback_discards_return_and_rejects_throw() {
        let source = r#"async function checkDisposal(){
  var reads=0, calls=0, receiver, reason={}, poison={get then(){reads++;throw reason;}};
  for(var absent of [undefined,null]){
    var stack=new AsyncDisposableStack(), resource={[Symbol.asyncDispose]:absent,[Symbol.dispose](){calls++;receiver=this;$262.gc();return poison;}};
    print(stack.use(resource)===resource);print(calls===0);resource[Symbol.dispose]=function(){throw reason;};
    var result=stack.disposeAsync();print(result instanceof Promise);print(stack.disposed);print(receiver===resource);await result;print(reads===0);print(calls===1);
    await stack.disposeAsync();print(calls===1);calls=0;
  }
  var stack=new AsyncDisposableStack(), pending=Promise.withResolvers().promise;
  stack.use({[Symbol.dispose](){return pending;}});await stack.disposeAsync();print(true);
  var syntaxCalls=0;async function syntax(){await using resource={[Symbol.dispose](){syntaxCalls++;$262.gc();return poison;}};print(syntaxCalls===0);}
  await syntax();print(syntaxCalls===1 && reads===0);
  async function syntaxPending(){await using resource={[Symbol.dispose](){return pending;}};}
  await syntaxPending();print(true);
  var events=[], stack=new AsyncDisposableStack();
  stack.use({[Symbol.dispose](){events.push('first');}});stack.use({[Symbol.dispose](){events.push('last');throw reason;}});
  var result=stack.disposeAsync();print(events.join('|')==='last');Promise.resolve().then(()=>events.push('tick'));
  try{await result;print(false);}catch(e){print(e===reason);}print(events.join('|')==='last|first|tick');
  var stack=new AsyncDisposableStack(), first={}, last={};
  stack.use({[Symbol.dispose](){throw first;}});stack.use({[Symbol.dispose](){throw last;}});
  try{await stack.disposeAsync();print(false);}catch(e){print(e instanceof SuppressedError && e.error===first && e.suppressed===last);}
  var stack=new AsyncDisposableStack();stack.use({[Symbol.dispose](){ $262.gc();throw {fresh:43};}});
  try{await stack.disposeAsync();print(false);}catch(e){print(e.fresh===43);}
  var stack=new AsyncDisposableStack();stack.use({[Symbol.asyncDispose](){return poison;},[Symbol.dispose](){print(false);}});
  try{await stack.disposeAsync();print(false);}catch(e){print(e===reason && reads===1);}
  var stack=new AsyncDisposableStack(), gate=Promise.withResolvers(), complete=false;
  stack.use({[Symbol.asyncDispose](){return gate.promise;}});var result=stack.disposeAsync().then(()=>{complete=true;});await Promise.resolve();print(!complete);gate.resolve();await result;print(complete);
  for(var operation of ['adopt','defer']){var stack=new AsyncDisposableStack();if(operation==='adopt')stack.adopt(41,()=>poison);else stack.defer(()=>poison);try{await stack.disposeAsync();print(false);}catch(e){print(e===reason);}}
  var stack=new AsyncDisposableStack(), used={get [Symbol.asyncDispose](){events.push('async');return null;},get [Symbol.dispose](){events.push('sync');return function(){print(this===used);return poison;};}};
  events=[];stack.use(used);print(events.join('|')==='async|sync');var moved=stack.move();print(stack.disposed && !moved.disposed);await moved.disposeAsync();print(reads===3);
  var sync=new DisposableStack();sync.use({[Symbol.dispose](){return poison;}});sync.dispose();print(reads===3);
}
checkDisposal().then(()=>print('done'), e=>{print('unexpected');print(e);});
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "disposal-fallback.js").unwrap();
            let path = std::env::temp_dir()
                .join(format!("quench-disposal-fallback-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                let mut expected = vec!["true"; 35];
                expected.push("done");
                assert_eq!(output.0.borrow().as_slice(), expected.as_slice());
            }
        }
    }

    #[test]
    fn regression_eval_string_literals_and_directives_share_cooked_constants() {
        let source = r#"var cases=[["\"\\u{00000000000001234}\"", "\u1234"], ["\"\\u{D800}\"", "\ud800"], ["\"\\u{10ffff}\"", "\udbff\udfff"], ["\"\\x41\\u0042\"", "AB"], ["'\\n\\t\\0'", "\n\t\u0000"], ["\"a\\\rb\"", "ab"], ["\"a\\\nb\"", "ab"], ["\"a\\\r\nb\"", "ab"], ["\"a\\\u2028b\"", "ab"], ["\"a\\\u2029b\"", "ab"], ["'a\"b'", "a\"b"], ["\"a\\\\b\"", "a\\b"]];
for(var pair of cases){print(eval(pair[0])===pair[1]);print((0,eval)(pair[0])===pair[1]);print(Function("return "+pair[0])()===pair[1]);}
print(eval('"one";"\\u1234";')==='\u1234');print(eval('"\\u1234";"use strict"')==='use strict');print(eval('"use strict";"\\u1234"')==='\u1234');
print(eval('"use\\x20strict"; with({}){}')===undefined);
print(eval('"use strict"; ; ;')==='use strict');print(eval('; ;')===undefined);
print(eval('"a"+"b"')==='ab');print((0,eval)('"a"+"b"')==='ab');
var padded='"\\u{'+'0'.repeat(65536)+'1234}"';print(eval(padded)==='\u1234');print((0,eval)(padded)==='\u1234');
for(var source of ['"\\u{}"','"\\u{110000}"','"\\xG0"']){try{eval(source);print(false);}catch(e){print(e instanceof SyntaxError);}}
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "eval-literals.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-eval-literals-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(output.0.borrow().as_slice(), &["true"; 49]);
            }
        }
    }

    #[test]
    fn regression_atanh_preserves_special_values_and_accurate_sign_symmetry() {
        let source = r#"for(var value of [NaN,Infinity,-Infinity,2,-2,1+Number.EPSILON,-1-Number.EPSILON,undefined])print(Number.isNaN(Math.atanh(value)));
print(Object.is(Math.atanh(0),0));print(Object.is(Math.atanh(-0),-0));print(Math.atanh(1)===Infinity);print(Math.atanh(-1)===-Infinity);
for(var value of [Number.MIN_VALUE,-Number.MIN_VALUE,1e-300,-1e-300,1e-30,-1e-30])print(Object.is(Math.atanh(value),value));
var references=[[-0.9999983310699463,-6.998237084679027],[-0.9999978542327881,-6.87257975132917],[-0.3000025749206543,-0.3095224337886503],[0.00001,0.000010000000000333334],[0.3,0.3095196042031117],[0.9928233623504639,2.8132383539094192]];
for(var pair of references){var actual=Math.atanh(pair[0]);print(Math.abs(actual-pair[1])<=Number.EPSILON*Math.abs(pair[1]));print(Object.is(Math.atanh(-pair[0]),-actual));}
var calls=0,hint;var value={[Symbol.toPrimitive](h){calls++;hint=h;$262.gc();return 0.3;}};var result=Math.atanh(value);print(calls===1 && hint==='number' && Math.abs(result-references[4][1])<=Number.EPSILON);
var reason={};try{Math.atanh({valueOf(){throw reason;}});print(false);}catch(e){print(e===reason);}
for(var value of [1n,Symbol('value')]){try{Math.atanh(value);print(false);}catch(e){print(e instanceof TypeError);}}
var d=Object.getOwnPropertyDescriptor(Math,'atanh');print(Math.atanh.length===1 && Math.atanh.name==='atanh' && d.writable && !d.enumerable && d.configurable);
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "atanh.js").unwrap();
            let path = std::env::temp_dir().join(format!("quench-atanh-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(output.0.borrow().as_slice(), &["true"; 35]);
            }
        }
    }

    #[test]
    fn regression_global_object_bindings_use_inherited_presence_and_receiver() {
        let source = r#"var old=Object.getPrototypeOf(globalThis), target=Object.create(old), events=[], receivers=[], gets=0, sets=0, reason={}, throwing=false;
Object.defineProperty(target,'qq_plain',{configurable:true,value:undefined});Object.setPrototypeOf(globalThis,target);
print(qq_plain===undefined);print(typeof qq_plain==='undefined');
var proxy=new Proxy(target,{has(t,k){if(k==='qq_virtual'){events.push('has');$262.gc();return true;}if(k==='qq_absent'){events.push('absent-has');return false;}if(k==='qq_throw' && throwing)throw reason;return Reflect.has(t,k);},get(t,k,r){if(k==='qq_virtual'){events.push('get');receivers.push(r);$262.gc();return undefined;}if(k==='qq_absent'){gets++;return 41;}if(k==='qq_throw')throw reason;return Reflect.get(t,k,r);},set(t,k,v,r){if(k==='qq_virtual'){sets++;receivers.push(r);$262.gc();}return Reflect.set(t,k,v,r);}});
Object.setPrototypeOf(globalThis,proxy);
print(qq_virtual===undefined);print(events.join('|')==='has|get');print(receivers[0]===globalThis);events=[];
print(typeof qq_virtual==='undefined');print(events.join('|')==='has|get');events=[];
for(var i=0;i<2;i++){print(qq_virtual===undefined);}print(events.join('|')==='has|get|has|get');events=[];
try{qq_absent;print(false);}catch(e){print(e instanceof ReferenceError);}print(gets===0);print(typeof qq_absent==='undefined');print(gets===0);
throwing=true;try{qq_throw;print(false);}catch(e){print(e===reason);}try{typeof qq_throw;print(false);}catch(e){print(e===reason);}throwing=false;
Object.defineProperty(target,'qq_throw',{value:17,configurable:true});try{qq_throw;print(false);}catch(e){print(e===reason);}delete target.qq_throw;
(function(){'use strict';qq_virtual=23;})();print(sets===1);print(globalThis.qq_virtual===23);print(receivers[receivers.length-1]===globalThis);delete globalThis.qq_virtual;
(function(){'use strict';qq_virtual={value:43};})();print(sets===2 && globalThis.qq_virtual.value===43);delete globalThis.qq_virtual;
Object.defineProperty(globalThis,'qq_own',{value:undefined,configurable:true});print(qq_own===undefined);print(typeof qq_own==='undefined');delete globalThis.qq_own;
var receiver;target.qq_callable=function(){receiver=this;return 29;};print(qq_callable()===29 && receiver===globalThis);target.qq_callable=function(){'use strict';return this;};print(qq_callable()===undefined);
var count=0, shadowProxy=new Proxy(target,{has(t,k){if(k==='qq_shadow')count++;return Reflect.has(t,k);},get(t,k,r){if(k==='qq_shadow')count++;return Reflect.get(t,k,r);}});Object.setPrototypeOf(globalThis,shadowProxy);
(function(){let qq_shadow=31;print(qq_shadow===31);print(typeof qq_shadow==='number');qq_shadow=37;print(qq_shadow===37);})();print(count===0);
Object.setPrototypeOf(globalThis,target);print(eval('qq_plain')===undefined);print((0,eval)('qq_plain')===undefined);print(Function('return qq_plain')()===undefined);
Object.setPrototypeOf(globalThis,old);try{qq_plain;print(false);}catch(e){print(e instanceof ReferenceError);}print(typeof qq_plain==='undefined');
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "global-proxy.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-global-proxy-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(output.0.borrow().as_slice(), &["true"; 34]);
            }
        }
    }

    #[test]
    fn regression_symbol_constructor_prototype_is_constant_in_each_realm() {
        let source = r#"var foreign=$262.createRealm().global;
for(var global of [globalThis,foreign]){
 var C=global.Symbol,proto=C.prototype,d=Object.getOwnPropertyDescriptor(C,'prototype');
 print(d.value===proto && !d.writable && !d.enumerable && !d.configurable);
 var g=Object.getOwnPropertyDescriptor(global,'Symbol');print(g.value===C && g.writable && !g.enumerable && g.configurable);
 var d=Object.getOwnPropertyDescriptor(proto,'constructor');print(d.value===C && d.writable && !d.enumerable && d.configurable);
 var replacement={};C.prototype=replacement;print(C.prototype===proto);
 try{(function(){'use strict';C.prototype=replacement;})();print(false);}catch(e){print(e instanceof TypeError);}
 print(Reflect.set(C,'prototype',replacement)===false && C.prototype===proto);
 print(Reflect.deleteProperty(C,'prototype')===false && C.prototype===proto);
 print(Reflect.defineProperty(C,'prototype',{value:proto})===true);
 for(var desc of [{value:replacement},{writable:true},{configurable:true},{enumerable:true},{get(){return proto;}}]){
  print(Reflect.defineProperty(C,'prototype',desc)===false);
  try{Object.defineProperty(C,'prototype',desc);print(false);}catch(e){print(e instanceof TypeError);}
 }
 var proxy=new Proxy(C,{get(){return replacement;}});try{proxy.prototype;print(false);}catch(e){print(e instanceof TypeError);}
 var proxy=new Proxy(C,{defineProperty(){return true;}});try{Reflect.defineProperty(proxy,'prototype',{value:replacement});print(false);}catch(e){print(e instanceof TypeError);}
 var sym=C('value'),wrapped=Object(sym);$262.gc();print(proto.valueOf.call(wrapped)===sym);
 proto.extra=17;print(proto.extra===17 && Object.isExtensible(proto));delete proto.extra;
 print(C.iterator===Symbol.iterator && C.toPrimitive===Symbol.toPrimitive);
}
print(Symbol.prototype!==foreign.Symbol.prototype && Symbol!==foreign.Symbol);
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "symbol-descriptor.js").unwrap();
            let path = std::env::temp_dir()
                .join(format!("quench-symbol-descriptor-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(output.0.borrow().as_slice(), &["true"; 47]);
            }
        }
    }

    #[test]
    fn regression_object_literal_prototype_mutation_follows_oxc_property_kind() {
        let source = r#"var __proto__='shorthand';
var values=[{__proto__(){}},{'__proto__'(){}},{*__proto__(){}},{async __proto__(){}},{async *__proto__(){}},{['__proto__'](){}},{*['__proto__'](){}},{__proto__},{['__proto__']:null},{__proto__:null},{'__proto__':null},{__proto__:17}];
for(var i=0;i<values.length;i++){var o=values[i],own=i<9;print(Object.getPrototypeOf(o)===(i===9 || i===10?null:Object.prototype) && Object.hasOwn(o,'__proto__')===own);}
for(var i=0;i<7;i++){var f=values[i].__proto__,d=Object.getOwnPropertyDescriptor(values[i],'__proto__');print(f.name==='__proto__' && d.writable && d.enumerable && d.configurable);try{new f;print(false);}catch(e){print(e instanceof TypeError);}}
var o={__proto__:null,__proto__(){return 13;},extra:17};print(Object.getPrototypeOf(o)===null && o.__proto__()===13 && o.extra===17);
var o={__proto__(){return 19;},__proto__:null};print(Object.getPrototypeOf(o)===null && o.__proto__()===19);
var o={get __proto__(){return 23;},set __proto__(v){this.value=v;},__proto__:null};var d=Object.getOwnPropertyDescriptor(o,'__proto__');o.__proto__=29;print(Object.getPrototypeOf(o)===null && o.__proto__===23 && o.value===29 && typeof d.get==='function' && typeof d.set==='function');
var o={['__proto__']:null,extra:17};print(Object.getPrototypeOf(o)===Object.prototype && Object.hasOwn(o,'__proto__') && o.__proto__===null && o.extra===17);
var o={__proto__,extra:19};print(Object.getPrototypeOf(o)===Object.prototype && o.__proto__==='shorthand' && o.extra===19);
var prototype={value:31}, o={__proto__:prototype,__proto__(){return super.value;}};print(o.__proto__()===31 && Object.getPrototypeOf(o)===prototype);Object.setPrototypeOf(o,{value:37});print(o.__proto__()===37);
var log=[],key={toString(){log.push('key');$262.gc();return '__proto__';}};var o={[key]:(log.push('value'),$262.gc(),41),__proto__:(log.push('prototype'),null)};print(log.join('|')==='key|value|prototype' && o.__proto__===41 && Object.getPrototypeOf(o)===null);
var o={...{['__proto__']:43}};print(Object.getPrototypeOf(o)===Object.prototype && o.__proto__===43 && Object.hasOwn(o,'__proto__'));
var o=JSON.parse('{"__proto__":47}');print(Object.getPrototypeOf(o)===Object.prototype && o.__proto__===47);
for(var source of ['({__proto__(){return 53;},__proto__:null})','({__proto__:null,*__proto__(){yield 59;}})']){var o=eval(source),f=Function('return '+source)();print(Object.getPrototypeOf(o)===null && Object.getPrototypeOf(f)===null && Object.hasOwn(o,'__proto__') && Object.hasOwn(f,'__proto__'));}
try{Function('return {__proto__:null,"__proto__":null}');print(false);}catch(e){print(e instanceof SyntaxError);}
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "object-proto.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-object-proto-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(output.0.borrow().as_slice(), &["true"; 39]);
            }
        }
    }

    #[test]
    fn regression_date_primitive_conversion_uses_method_hint_and_ordinary_fallback() {
        let source = r#"var hook=Date.prototype[Symbol.toPrimitive], d=new Date(123), seen=[];
Object.defineProperty(d,Symbol.toPrimitive,{configurable:true,value:function(h){seen.push(h);$262.gc();return h==='default'?7:h==='number'?11:'text';}});
print(0+d===7);print(d==7);print(Number(d)===11);print(String(d)==='text');print(seen.join('|')==='default|default|number|string');
delete d[Symbol.toPrimitive];print(0+d==='0'+d.toString());print(Number(d)===123);
for(var absent of [undefined,null]){
 Object.defineProperty(d,Symbol.toPrimitive,{configurable:true,value:absent});
 var log=[];d.valueOf=function(){log.push('valueOf');$262.gc();return 17;};d.toString=function(){log.push('toString');$262.gc();return 'text';};
 print(0+d===17 && log.join('|')==='valueOf');log=[];print(String(d)==='text' && log.join('|')==='toString');
 d.valueOf=function(){log.push('valueOf');$262.gc();return {};};log=[];print(0+d==='0text' && log.join('|')==='valueOf|toString');
}
delete d[Symbol.toPrimitive];delete d.valueOf;delete d.toString;delete Date.prototype[Symbol.toPrimitive];
print(0+d===123);print(d==123);print(String(d)===d.toString());print(0+new Date(NaN)!==0+new Date(NaN));
var log=[], object={valueOf(){log.push('valueOf');$262.gc();return 19;},toString(){log.push('toString');$262.gc();return 'text';}};
print(hook.call(object,'default')==='text' && log.join('|')==='toString');log=[];print(hook.call(object,'number')===19 && log.join('|')==='valueOf');
object[Symbol.toPrimitive]=function(){throw 'must not be used';};log=[];print(hook.call(object,'string')==='text' && log.join('|')==='toString');
for(var hint of [undefined,null,1,{},'invalid']){try{hook.call(object,hint);print(false);}catch(e){print(e instanceof TypeError);}}
for(var receiver of [undefined,null,1,'text',true]){try{hook.call(receiver,'default');print(false);}catch(e){print(e instanceof TypeError);}}
var log=[], proxy=new Proxy(Object.create(null),{get(t,k,r){log.push(k);$262.gc();return undefined;}});
try{0+proxy;print(false);}catch(e){print(e instanceof TypeError && log.length===3 && log[0]===Symbol.toPrimitive && log[1]==='valueOf' && log[2]==='toString');}
var log=[], reason={}, date=new Date(0);Object.defineProperty(date,Symbol.toPrimitive,{get(){log.push('hook');$262.gc();throw reason;}});
try{0+date;print(false);}catch(e){print(e===reason && log.join('|')==='hook');}
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "date-primitive.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-date-primitive-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(output.0.borrow().as_slice(), &["true"; 32]);
            }
        }
    }

    #[test]
    fn regression_error_constructors_resolve_prototype_before_message_and_cause() {
        let source = r#"var constructors=[Error,EvalError,RangeError,ReferenceError,SyntaxError,TypeError,URIError,SuppressedError,AggregateError];
function argumentsFor(C, message, options, errors){return C===AggregateError?[errors,message,options]:C===SuppressedError?[17,19,message,options]:[message,options];}
for(var C of constructors){
 var log=[], marker={}, options=new Proxy({cause:marker},{has(t,k){log.push('has:'+k);return Reflect.has(t,k);},get(t,k,r){log.push('cause');$262.gc();return Reflect.get(t,k,r);}});
 var message={toString(){log.push('message');$262.gc();return 'value';}};
 var errors={[Symbol.iterator](){log.push('iterator');var i=0;return {next(){log.push('next');return {done:i++>0,value:23};}};}};
 var target=new Proxy(C,{get(t,k,r){if(k==='prototype'){log.push('prototype');$262.gc();}return Reflect.get(t,k,r);}});
 var value=new target(...argumentsFor(C,message,options,errors));
 var expected=C===SuppressedError?'prototype|message':C===AggregateError?'prototype|message|has:cause|cause|iterator|next|next':'prototype|message|has:cause|cause';
 print(log.join('|')===expected);print(Object.getPrototypeOf(value)===C.prototype);print(value.message==='value');print(Error.isError(value));
 var descriptor=Object.getOwnPropertyDescriptor(value,'message');print(descriptor.writable && descriptor.configurable && !descriptor.enumerable);
 if(C===SuppressedError){print(value.error===17 && value.suppressed===19 && !Object.hasOwn(value,'cause'));}else{print(value.cause===marker);}
 var reason={}, calls=0, message={toString(){calls++;return 'unused';}}, target=new Proxy(function NewTarget(){},{get(t,k,r){if(k==='prototype')throw reason;return Reflect.get(t,k,r);}});
 try{Reflect.construct(C,argumentsFor(C,message,options,[]),target);print(false);}catch(e){print(e===reason && calls===0);}
 var log=[], reason={}, target=new Proxy(function NewTarget(){},{get(t,k,r){if(k==='prototype')log.push('prototype');return Reflect.get(t,k,r);}}), message={toString(){log.push('message');throw reason;}};
 try{Reflect.construct(C,argumentsFor(C,message,options,[]),target);print(false);}catch(e){print(e===reason && log.join('|')==='prototype|message');}
 class Derived extends C {};var value=Reflect.construct(C,argumentsFor(C,'derived',{},[]),Derived);print(value instanceof Derived && Error.isError(value));
}
var custom={}, reentrant=0, target=new Proxy(function Target(){},{get(t,k,r){if(k==='prototype'){reentrant++;new Error('inner');$262.gc();return custom;}return Reflect.get(t,k,r);}});
var value=Reflect.construct(Error,['outer'],target);print(Object.getPrototypeOf(value)===custom && reentrant===1 && Error.isError(value));
var realm=$262.createRealm(), foreign=realm.global, intrinsic=foreign.Error.prototype, foreignType=foreign.TypeError.prototype;
var Target=foreign.Function('');Target.prototype=1;foreign.Error=function Replaced(){throw 'wrong';};
var value=Reflect.construct(Error,['foreign'],Target);print(Object.getPrototypeOf(value)===intrinsic && value.message==='foreign');
var value=Reflect.construct(TypeError,['foreign'],Target.bind(null));print(Object.getPrototypeOf(value)===foreignType);
var cause={}, E=Error;Error=function(){throw 'replaced';};try{null.x;}catch(e){print(e instanceof TypeError);}Error=E;
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "error-construction-order.js").unwrap();
            let path = std::env::temp_dir().join(format!(
                "quench-error-construction-order-{}",
                std::process::id()
            ));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(
                    output.0.borrow().as_slice(),
                    &[
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true"
                    ]
                );
            }
        }
    }

    #[test]
    fn regression_destructuring_iterator_steps_validate_results_and_mark_abrupt_completion() {
        let source = r#"var cases=[['array-spread',v=>[...v]],['call-spread',v=>(()=>{})(...v)],['for-of',v=>{for(var x of v){}}],['destructure',v=>{var [a]=v;}],['rest',v=>{var [...a]=v;}],['array-from',v=>Array.from(v)],['map',v=>new Map(v)],['set',v=>new Set(v)],['weak-map',v=>new WeakMap(v)],['weak-set',v=>new WeakSet(v)],['typed-array',v=>new Int8Array(v)],['typed-from',v=>Int8Array.from(v)],['yield-star',v=>{var g=(function*(){yield* v;})();g.next();}]];
for(var entry of cases){var iterable={[Symbol.iterator](){return {next(){return 1;}};}};try {entry[1](iterable);print(entry[0]+':no-error');}catch(e){print(entry[0]+':'+(e instanceof TypeError));}}
var patterns=[
 value=>{var [x]=value;},value=>{let [x]=value;},value=>{const [x]=value;},
 value=>{var x;[x]=value;},value=>(function([x]){})(value),value=>{try{throw value;}catch([x]){}},
 value=>{for(var [x] of [value]){}},value=>(([x])=>{})(value),value=>(function*([x]){})(value)
];
for(var bad of [null,undefined,1,true,'a',Symbol.iterator]){
 for(var apply of patterns.slice(0,9)){var closed=0;var iterable={[Symbol.iterator](){return {next(){return bad;},return(){closed++;return {};}};}};try {apply(iterable);print(false);}catch(e){print(e instanceof TypeError && closed===0);}}
}
var variants=[value=>{var [x]=value;},value=>{var [...x]=value;},value=>{var x;[x]=value;},value=>{var x;[...x]=value;}];
for(var apply of variants){for(var phase of ['next','done','value']){var closed=0, reason={};var iterable={[Symbol.iterator](){return {next(){if(phase==='next')throw reason;return {get done(){if(phase==='done')throw reason;return false;},get value(){if(phase==='value')throw reason;$262.gc();return 1;}};},return(){closed++;throw 'close';}};}};try{apply(iterable);print(false);}catch(e){print(e===reason && closed===0);}}}
var closed=0, reason={}, iterator={[Symbol.iterator](){return this;},next(){return {done:false,value:undefined};},return(){closed++;return {};}};
try {var [x=(()=>{throw reason;})()]=iterator;}catch(e){print(e===reason && closed===1);}
var log=[], count=0, iterable={[Symbol.iterator](){return this;},get next(){log.push('next');return function(){count++;return {done:count>2,value:count};};},return(){log.push('return');return {};}};
var [a,b]=iterable; print(a===1 && b===2 && log.join('|')==='next|return');
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "destructuring-completion.js").unwrap();
            let path = std::env::temp_dir().join(format!(
                "quench-destructuring-completion-{}",
                std::process::id()
            ));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(
                    output.0.borrow().as_slice(),
                    &[
                        "array-spread:true",
                        "call-spread:true",
                        "for-of:true",
                        "destructure:true",
                        "rest:true",
                        "array-from:true",
                        "map:true",
                        "set:true",
                        "weak-map:true",
                        "weak-set:true",
                        "typed-array:true",
                        "typed-from:true",
                        "yield-star:true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true"
                    ]
                );
            }
        }
    }

    #[test]
    fn regression_generator_start_abrupt_completion_skips_body_handlers() {
        let source = r#"var log=[], reason={token:7};
function* source(){try {log.push('body');yield 11;}catch(e){log.push('catch');return e;}finally{log.push('finally');}}
var g=source();$262.gc();try {g.throw(reason);print(false);}catch(e){print(e===reason);}print(log.length===0);print(g.next().done===true);
var g=source(), result=g.return(19);print(result.done===true && result.value===19);print(log.length===0);print(g.next().value===undefined);
var g=source();print(g.next().value===11);print(g.throw(reason).value===reason);print(log.join('|')==='body|catch|finally');
var params=0;function* defaults(x=(params++,23)){try{yield x;}finally{log.push('unexpected');}}
var g=defaults();print(params===1);try{g.throw(reason);}catch(e){print(e===reason);}print(log.includes('unexpected')===false);
var delegated=(function*(){yield* source();})();try{delegated.throw(reason);print(false);}catch(e){print(e===reason);}print(delegated.next().done===true);
async function check(){
 var effects=[];async function* asyncSource(){try{effects.push('body');yield 31;}catch(e){effects.push('catch');return e;}finally{effects.push('finally');}}
 var g=asyncSource();$262.gc();try{await g.throw(reason);print(false);}catch(e){print(e===reason);}print(effects.length===0);print((await g.next()).done===true);
 var g=asyncSource();var result=await g.return(Promise.resolve(37));print(result.done===true && result.value===37);print(effects.length===0);
 var g=asyncSource();print((await g.next()).value===31);print((await g.throw(reason)).value===reason);print(effects.join('|')==='body|catch|finally');
}
check();
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "generator-completion.js").unwrap();
            let path = std::env::temp_dir().join(format!(
                "quench-generator-completion-{}",
                std::process::id()
            ));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(
                    output.0.borrow().as_slice(),
                    &[
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true"
                    ]
                );
            }
        }
    }

    #[test]
    fn regression_static_field_initializers_keep_eval_context_and_caller_boundary() {
        let source = r#"var outside=41, log=[];
class Base {static value=7;}
class Fields extends Base {
 static first=(log.push('first'),11);
 static result=eval('super.value + this.first');
 static target=eval('new.target');
 static identity=eval('this');
 static binding=eval('Fields');
 static local=eval('var outside=13; outside');
 static captured=eval('() => super.value + this.first');
 static rejected=(() => {try {eval('arguments');return false;}catch(e){return e instanceof SyntaxError;}})();
 static last=(log.push('last'),19);
}
print(log.join('|')); print(Fields.result===18); print(Fields.target===undefined); print(Fields.identity===Fields); print(Fields.binding===Fields); print(Fields.local===13); print(outside===41); print(Fields.rejected===true);
$262.gc(); print(Fields.captured()===18);
Fields.first=23; print(Fields.captured()===30);
var events=[];
try {class Abrupt { static before=events.push('before'); static error=(() => {throw 37;})(); static after=events.push('after'); }} catch(e){print(e===37);}
print(events.join('|')==='before');
var retained=[];
for(var i=0;i<2;i++){let value=i; class Scoped {static field=eval('() => value');} retained.push(Scoped);}
$262.gc(); print(retained[0].field()===0); print(retained[1].field()===1);
function sloppy(){return sloppy.caller;}
function outer(){class Caller {static value=sloppy();} return Caller.value;}
print(outer()===null);
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "static-field-context.js").unwrap();
            let path = std::env::temp_dir().join(format!(
                "quench-static-field-context-{}",
                std::process::id()
            ));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(
                    output.0.borrow().as_slice(),
                    &[
                        "first|last",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true"
                    ]
                );
            }
        }
    }

    #[test]
    fn regression_auto_accessor_backing_fields_preserve_static_order_and_private_identity() {
        let source = r#"var log=[], symbol=Symbol('slot'), key={ [Symbol.toPrimitive]() {log.push('key');return 'computed';} }; class Base {static base=7;}
class C extends Base {
 static before=log.push('before');
 static accessor x=(log.push('x'),11);
 static accessor empty;
 static accessor [key]=(log.push('computed'),super.base);
 static accessor [symbol]=19;
 static accessor #private=23;
 static accessor evaluated=eval('super.base + this.x');
 static read(){return this.#private;} static write(v){this.#private=v;}
 static {log.push(this.x+this.computed+this.read());}
 static after=log.push('after');
}

print(log.join('|'));
print(C.x === 11); print(C.empty === undefined); print(C.computed === 7); print(C[symbol] === 19); print(C.read() === 23); print(C.evaluated === 18);
C.x=31; C.write(37); print(C.x===31); print(C.read()===37);
var descriptor=Object.getOwnPropertyDescriptor(C,'x');
print(descriptor.enumerable===false); print(descriptor.configurable===true); print(descriptor.get.name==='get x'); print(descriptor.set.name==='set x'); print(descriptor.get.length===0); print(descriptor.set.length===1);
try {descriptor.get.call({});print(false);}catch(e){print(e instanceof TypeError);}
try {descriptor.set.call({},1);print(false);}catch(e){print(e instanceof TypeError);}
class D extends C {};
try {D.x;print(false);}catch(e){print(e instanceof TypeError);}
print(Object.getOwnPropertyNames(C).includes('private')===false);
var classes=[];
for(var i=0;i<2;i++) { class Local { static accessor value=i; } classes.push(Local); }
$262.gc(); print(classes[0].value===0); print(classes[1].value===1); classes[0].value=41; print(classes[1].value===1);
class Names {
 static accessor plain=function(){};
 static accessor [symbol]=()=>{};
 static accessor #secret=function(){};
 static read(){return this.#secret.name;}
 accessor [symbol]=function(){};
}
print(Names.plain.name==='plain'); print(Names[symbol].name==='[slot]'); print(Names.read()==='#secret'); print(new Names()[symbol].name==='[slot]');
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "auto-accessors.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-auto-accessors-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(
                    output.0.borrow().as_slice(),
                    &[
                        "key|before|x|computed|41|after",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true"
                    ]
                );
            }
        }
    }

    #[test]
    fn regression_function_arguments_retain_original_values_after_residual_round_trip() {
        let source = r#"function plain(a,b) {
  var first=plain.arguments, second=plain.arguments;
  print(first !== second); print(first.length === 4); print(first[0] === 7); print(first[1] === undefined); print(first[2] === 9); print(first[3] === 11);
  print(first.callee === plain); print(Object.prototype.toString.call(first) === '[object Arguments]'); print(Object.getPrototypeOf(first) === Object.prototype);
  print(Reflect.ownKeys(first).map(String).join(',') === '0,1,2,3,length,callee,Symbol(Symbol.iterator)');
  first[0]=17; delete first[2]; first.length=0;
  print(second[0] === 7); print(second[2] === 9); print(plain.arguments[0] === 7); print(a === 7);
  a=19; arguments[1]=23; arguments=null; $262.gc();
  print(plain.arguments[0] === 7); print(plain.arguments[1] === undefined); print(plain.arguments.length === 4);
}
print(plain.arguments === null); plain(7,undefined,9,11); print(plain.arguments === null);
function defaults(a=7) { var snapshot=defaults.arguments; print(snapshot.length === 2); print(snapshot[0] === undefined); print(snapshot[1] === 19); try {snapshot.callee;print(false);}catch(e){print(e instanceof TypeError);} }
defaults(undefined,19);
function rest(...values) { var snapshot=rest.arguments; print(snapshot[0] === 1); print(snapshot.length === 3); try {snapshot.callee;print(false);}catch(e){print(e instanceof TypeError);} }
rest(1,2,3);
function pattern({value}) { var snapshot=pattern.arguments; print(snapshot[0].value === 31); try {snapshot.callee;print(false);}catch(e){print(e instanceof TypeError);} }
pattern({value:31});
function recursive(n) { if(n) { recursive(n-1); print(recursive.arguments[0] === n); } else print(recursive.arguments[0] === 0); }
recursive(2);
function collecting(a) { a=null; arguments=null; $262.gc(); print(collecting.arguments[0].token === 37); }
collecting({token:37});
function constructor(a) { a=null; arguments=null; $262.gc(); print(constructor.arguments[0].token === 41); }
new constructor({token:41});
function arithmetic(a) { return a + 1; }
print(arithmetic({valueOf(){print(arithmetic.arguments[0] === this);return 43;}}) === 44);
var functions=[];
for(var index=0;index<2;index++) functions.push(function self(expected){print(self.arguments.callee === expected);});
for(var f of functions) f(f);
function shadow(arguments) { print(shadow.arguments[0] === 47); }
shadow(47);
"#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "function-arguments.js").unwrap();
            let path = std::env::temp_dir()
                .join(format!("quench-function-arguments-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            for program in [program, decoded] {
                let host = Capture::default();
                let output = host.clone();
                let mut runtime = Runtime::new(host);
                runtime.execute(&program).unwrap();
                assert_eq!(
                    output.0.borrow().as_slice(),
                    &[
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true", "true", "true", "true", "true", "true", "true", "true",
                        "true", "true"
                    ]
                );
            }
        }
    }

    #[test]
    fn regression_caller_uses_execution_context_boundaries() {
        assert_output_in_execution_modes(
            r#"function inner() { return inner.caller; }
function outer() { return inner(); }
print(inner() === null); print(outer() === outer);
function throughCall() { return inner.call(null); }
function throughApply() { return inner.apply(null, []); }
function throughReflect() { return Reflect.apply(inner, null, []); }
var bound = inner.bind(null);
function throughBound() { return bound(); }
print(throughCall() === throughCall); print(throughApply() === throughApply);
print(throughReflect() === throughReflect); print(throughBound() === throughBound);
print([0].map(inner)[0] === null);
print(Array.from([0], inner)[0] === null);
function strictOuter() { 'use strict'; var answer=inner(); return answer; }
print(strictOuter() === null);
function direct() { return eval('inner()'); }
function nestedEval() { return eval('eval("inner()")'); }
print(direct() === direct); print(nestedEval() === nestedEval);
function recursive(n) { if(n) return recursive(n-1); return recursive.caller; }
print(recursive(2) === recursive);
var functions=[];
for(var i=0;i<2;i++) functions.push(function same() { return inner(); });
for(var f of functions) { $262.gc(); print(f() === f); }
print(inner.caller === null);
function getterOuter() { return {get value() { return inner(); }}.value; }
print(typeof getterOuter() === 'function');
function nestedNative() { return [0].map(function callback() { return inner(); })[0]; }
print(nestedNative().name === 'callback');
function throwsThenCalls() { try {[0].map(function(){throw 7;});}catch(e){} return inner(); }
print(throwsThenCalls() === throwsThenCalls);
"#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_caller_censors_script_and_async_contexts() {
        assert_output_in_execution_modes(
            r#"function leaf() { return leaf.caller; }
function indirectCaller() { return (0,eval)('leaf()'); }
print(indirectCaller() === null);
(async function asyncCaller() { print(leaf() === null); })();
print((function* generatorCaller() { yield leaf(); })().next().value === null);
"#,
            &["true", "true", "true"],
        );
    }

    #[test]
    fn regression_activation_retains_exact_callable_identity() {
        assert_output_in_execution_modes(
            r#"
            var functions = [];
            for (var index = 0; index < 2; index++) {
              functions.push(function self() { return [self, arguments.callee]; });
            }
            for (var f of functions) { var result = f(); print(result[0] === f); print(result[1] === f); }
            $262.gc();
            for (var f of functions) { var result = f(); print(result[0] === f); print(result[1] === f); }
            var generators = [];
            for (var index = 0; index < 2; index++) { generators.push(function* self() { yield self; yield arguments.callee; }); }
            for (var f of generators) { var iterator = f(); $262.gc(); print(iterator.next().value === f); $262.gc(); print(iterator.next().value === f); }
            var asynchronous = [];
            for (var index = 0; index < 2; index++) { asynchronous.push(async function self() { print(self === asynchronous[index]); }); }
            for (var index = 0; index < 2; index++) { asynchronous[index](); }
            var firstPrototype = {label: 17}, secondPrototype = {label: 19};
            generators[0].prototype = firstPrototype; generators[1].prototype = secondPrototype;
            print(Object.getPrototypeOf(generators[0]()) === firstPrototype);
            print(Object.getPrototypeOf(generators[1]()) === secondPrototype);
            var simple = [], recursive = [];
            for (var index = 0; index < 2; index++) {
              simple.push(function self() { return self; });
              recursive.push(function self(n) { if (n) return self(n - 1); return [self, arguments.callee]; });
            }
            for (var f of simple) print(f() === f);
            for (var f of recursive) { var result = f(3); print(result[0] === f); print(result[1] === f); }
            var awaited = [], asyncGenerators = [];
            for (var index = 0; index < 2; index++) {
              awaited.push(async function self(expected) { await 0; $262.gc(); print(self === expected); print(arguments.callee === expected); });
              asyncGenerators.push(async function* self() { yield self; yield arguments.callee; });
            }
            for (var f of awaited) f(f);
            (async function() {
              for (var f of asyncGenerators) {
                var iterator = f(); $262.gc(); print((await iterator.next()).value === f);
                $262.gc(); print((await iterator.next()).value === f);
              }
            })();
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_restricted_function_caller_uses_property_authority() {
        assert_output_in_execution_modes(
            r#"
            function throws(read) { try { read(); print(false); } catch(e) { print(e instanceof TypeError); } }
            var object = {method() {}, get value() {}, set value(x) {}, async asyncMethod() {}, *generatorMethod() {}};
            var descriptor = Object.getOwnPropertyDescriptor(object, 'value');
            var functions = [object.method, descriptor.get, descriptor.set, object.asyncMethod, object.generatorMethod,
              () => {}, async function() {}, function*() {}, class {}, function() {}.bind(),
              Function, Array, Function.prototype.bind, Object.getOwnPropertyDescriptor];
            for (var f of functions) { throws(() => f.caller); throws(() => f.arguments); }
            var home = object.method;
            Object.setPrototypeOf(home, {get caller() { print(this === home); return 7; }});
            print(home.caller);
            var native = Object.getOwnPropertyDescriptor;
            Object.setPrototypeOf(native, {caller: 11}); print(native.caller);
            Object.defineProperty(object.generatorMethod, 'caller', {value: 13}); print(object.generatorMethod.caller);
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "7", "11",
                "13",
            ],
        );
    }

    #[test]
    fn regression_object_home_does_not_shadow_constructor_super() {
        assert_output_in_execution_modes(
            r#"
            class Base { constructor() { this.value = 7; } method() { return this.value; } }
            class Plain extends Base { constructor() { var plain = {}; super(); print(this.value); } }
            try { new Plain(); } catch (e) { print(e.name); }
            class WithMethod extends Base { constructor() { var key = {toString() { return ''; }}; super(); print(this.value); print(key.toString() === ''); } }
            try { new WithMethod(); } catch (e) { print(e.name); }
            class Deletes extends Base {
              constructor() {
                var coercions = 0, reads = 0;
                var key = {toString() { coercions++; return ''; }};
                var check = () => { try { delete super[(reads++, key)]; } catch (e) { print(e instanceof ReferenceError); } print(reads); print(coercions); };
                check(); super(); check();
                Object.setPrototypeOf(Deletes.prototype, null); check();
                print(this.value);
              }
            }
            try { new Deletes(); } catch (e) { print(e.name); }
            class Nested extends Base {
              constructor() {
                var first = {__proto__: {value: 11}, method() { return super.value; }};
                var second = {__proto__: {value: 13}, get method() { return super.value; }};
                $262.gc(); print(first.method()); print(second.method);
                var arrow = () => super(); arrow();
                print(this.value); print(super.method());
                print(first.method()); print(second.method);
              }
            }
            try { new Nested(); } catch (e) { print(e.name); }
            "#,
            // GetThisBinding precedes key evaluation; local Node differs before super().
            &[
                "7", "7", "true", "true", "0", "0", "true", "1", "0", "true", "2", "0", "7", "11",
                "13", "7", "7", "11", "13",
            ],
        );
    }

    #[test]
    fn regression_object_method_homes_remain_distinct_from_constructor_home() {
        assert_output_in_execution_modes(
            r#"
            class Base { constructor() { this.value = 7; } method() { return this.value; } }
            class Plain extends Base { constructor() { var plain = {}; super(); print(this.value); } }
            try { new Plain(); } catch (e) { print(e.name); }
            class WithMethod extends Base { constructor() { var key = {toString() { return ''; }}; super(); print(this.value); print(key.toString() === ''); } }
            try { new WithMethod(); } catch (e) { print(e.name); }
            class Deletes extends Base {
              constructor() {
                var coercions = 0, reads = 0;
                var key = {toString() { coercions++; return ''; }};
                var check = () => { try { delete super[key]; } catch (e) { print(e instanceof ReferenceError); } print(reads); print(coercions); };
                check(); super(); check();
                Object.setPrototypeOf(Deletes.prototype, null); check();
                print(this.value);
              }
            }
            try { new Deletes(); } catch (e) { print(e.name); }
            class Nested extends Base {
              constructor() {
                var first = {__proto__: {value: 11}, method() { return super.value; }};
                var second = {__proto__: {value: 13}, get method() { return super.value; }};
                $262.gc(); print(first.method()); print(second.method);
                var arrow = () => super(); arrow();
                print(this.value); print(super.method());
                print(first.method()); print(second.method);
              }
            }
            try { new Nested(); } catch (e) { print(e.name); }
            "#,
            &[
                "7", "7", "true", "true", "0", "0", "true", "0", "0", "true", "0", "0", "7", "11",
                "13", "7", "7", "11", "13",
            ],
        );
    }

    #[test]
    fn regression_assignment_retains_reference_before_rhs_eval() {
        assert_output_in_execution_modes(
            r#"
            function simple(declaration) {
              var x = 0;
              var inner = (function() { x = (eval(declaration), 1); return x; })();
              print(inner); print(x);
            }
            simple('var x;'); simple('var x = 2;');
            function compound() {
              var x = 4;
              var inner = (function() { x += (eval('var x = 9;'), 3); return x; })();
              print(inner); print(x);
            }
            compound();
            function logical() {
              var x = 0;
              var inner = (function() { x ||= (eval('var x = 9;'), 3); return x; })();
              print(inner); print(x);
            }
            logical();
            var referenceGlobal = 4;
            function globalReference() { referenceGlobal = (eval('var referenceGlobal = 9;'), 7); print(referenceGlobal); }
            globalReference(); print(referenceGlobal);
            function ownLocal() { var x = 1; x = (eval('var x = 2;'), 3); print(x); }
            ownLocal();
            function lexical() { let x = 4; function inner() { x = (eval('var x = 9;'), $262.gc(), 7); print(x); } inner(); print(x); }
            lexical();
            function immutable() { const x = 4; function inner() { try { x = (eval('var x = 9;'), 7); } catch(e) { print(e instanceof TypeError); } print(x); } inner(); print(x); }
            immutable();
            function mapped(x) { function inner() { x = (eval('var x = 9;'), $262.gc(), 7); print(x); } inner(); print(x); print(arguments[0]); }
            mapped(4);
            function block() { let x = 4; { let x = 5; x = (eval(''), 7); print(x); } print(x); }
            block();
            "#,
            // Pinned Test262 retains lref; local Node re-resolves after eval.
            &[
                "undefined",
                "1",
                "2",
                "1",
                "9",
                "7",
                "9",
                "3",
                "9",
                "7",
                "3",
                "9",
                "7",
                "true",
                "9",
                "4",
                "9",
                "7",
                "7",
                "7",
                "4",
            ],
        );
    }

    #[test]
    fn regression_captured_parameter_stores_preserve_argument_mapping() {
        assert_output_in_execution_modes(
            r#"
            function escaped(x) {
              var args = arguments;
              return {args: args, set() { eval(''); x = 7; return x; }};
            }
            var retained = escaped(4); $262.gc(); print(retained.set()); print(retained.args[0]);
            function disconnected(x) {
              delete arguments[0];
              function inner() { eval(''); x = 7; }
              inner(); print(x); print(arguments[0]);
            }
            disconnected(4);
            function unmapped(x) {
              'use strict';
              function inner() { eval(''); x = 7; }
              inner(); print(x); print(arguments[0]);
            }
            unmapped(4);
            "#,
            &["7", "7", "7", "undefined", "7", "4"],
        );
    }

    #[test]
    fn regression_direct_eval_writes_captured_bindings() {
        assert_output_in_execution_modes(
            r#"
            function strict(p) {
                'use strict';
                function inner() {eval('p = 17');}
                inner(); print(p); print(arguments[0]);
            }
            strict(); strict(1);
            function onlyEval(a) {eval('arguments[0] = 28'); return a;}
            print(onlyEval(1));
            function strictOnly(a) {'use strict'; return eval('arguments[0]');}
            print(strictOnly(29));
            function nestedArguments(a) {
                'use strict';
                function inner() {eval('arguments[0] = 30'); return eval('arguments.length');}
                print(inner()); print(arguments[0]);
            }
            nestedArguments(31);
            function escaped(p) {
                return function() {eval('p = 18'); return p;};
            }
            print(escaped(1)());
            function shadow(p) {
                return function() {var p = 2; eval('p = 19'); return p;};
            }
            print(shadow(1)());
            function declare(p) {
                function inner() {eval('var p = 20'); return p;}
                print(inner()); print(p);
            }
            declare(1);
            function lexical() {
                const fixed = 21;
                let mutable = 1;
                function inner() {
                    let local = 1;
                    eval('mutable = 22');
                    try {eval('fixed = 0');} catch (error) {print(error instanceof TypeError);}
                    try {eval('var local');} catch (error) {print(error instanceof SyntaxError);}
                    eval('var fixed = 26'); print(eval('fixed'));
                }
                inner(); print(mutable); print(fixed);
                function tdz() {eval('pending = 0');}
                try {tdz();} catch (error) {print(error instanceof ReferenceError);}
                let pending;
            }
            lexical();
            var named = function self() {
                function inner() {eval('self = 0'); return eval('self');}
                return inner();
            };
            print(named() === named);
            var strictNamed = function self() {
                'use strict';
                function inner() {eval('self = 0');}
                try {inner();} catch (error) {print(error instanceof TypeError);}
            };
            strictNamed();
            var globalValue = 23;
            function globalWrite() {eval('globalValue = 24'); print(eval('globalValue'));}
            globalWrite(); print(globalValue);
            var gets = 0, stored = 23;
            Object.defineProperty(globalThis, 'guestValue', {
                configurable: true, get() {gets++; return stored;}, set(value) {stored = value;}
            });
            function propertyWrite() {eval('guestValue = 25'); print(eval('guestValue'));}
            propertyWrite(); print(gets); print(stored);
            "#,
            &[
                "17",
                "undefined",
                "17",
                "1",
                "28",
                "29",
                "0",
                "31",
                "18",
                "19",
                "20",
                "1",
                "true",
                "true",
                "26",
                "22",
                "21",
                "true",
                "true",
                "true",
                "24",
                "24",
                "25",
                "1",
                "25",
            ],
        );
    }

    #[test]
    fn regression_parameter_arguments_bindings_survive_body_shadowing() {
        assert_output_in_execution_modes(
            r#"
            function replace(h = () => arguments) {
                var arguments = 0;
                print(h().length); print(arguments); print(arguments === h());
            }
            replace();
            function copy(h = () => arguments) {
                var arguments;
                print(h() === arguments);
                arguments = 42;
                print(h() === arguments); print(h().length);
            }
            copy();
            function parameter(arguments = 41, h = () => arguments) {
                print(arguments); print(h());
            }
            parameter();
            function write(a = (arguments = 43), h = () => arguments) {
                var arguments;
                print(a); print(arguments); arguments = 44; print(h());
            }
            write();
            function lexical(h = () => arguments) {
                try { print(arguments); } catch (error) {print(error instanceof ReferenceError);}
                let arguments = 44;
                print(arguments); print(h().length);
            }
            lexical();
            function nested(h = () => () => arguments) {
                var arguments = 0;
                print(h()().length);
            }
            nested();
            function mutate(h = () => {arguments = 45; return arguments;}) {
                var arguments;
                print(arguments.length); print(h()); print(arguments.length);
            }
            mutate();
            "#,
            &[
                "0", "0", "false", "true", "false", "0", "41", "41", "43", "43", "43", "true",
                "44", "0", "0", "0", "45", "0",
            ],
        );
    }

    #[test]
    fn regression_native_function_source_uses_installed_names_without_guest_reads() {
        assert_output_in_execution_modes(
            r#"
            var functions = [Array, Object.prototype.toString, decodeURI, Math.asin,
                String.prototype.blink, RegExp.prototype[Symbol.split],
                Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get,
                Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get,
                Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set];
            var stringify = Function.prototype.toString;
            for (var fn of functions) print(stringify.call(fn));
            var original = stringify.call(Array);
            var bound = Array.bind();
            Object.defineProperty(Array, 'name', {get() {
                $262.gc(); throw new Error('name must not be read');
            }});
            $262.gc();
            print(stringify.call(Array) === original);
            print(stringify.call(new Proxy(Array, {get() {
                throw new Error('proxy trap must not run');
            }})));
            print(stringify.call(bound));
            print(stringify.call(function actual() { return 42; }));
            print(stringify.call(async function /* async */ () { return 42; }));
            print(stringify.call(function* /* generator */ () { yield 42; }));
            Object.freeze(Array);
            print(Object.isFrozen(Array));
            print(Object.isFrozen(new Proxy(Array, {})));
            var thrower = (function() {
                'use strict';
                return Object.getOwnPropertyDescriptor(arguments, 'callee').get;
            })();
            print(Object.isFrozen(thrower));
            print(Reflect.ownKeys(thrower).sort().join(','));
            "#,
            &[
                "function Array() { [native code] }",
                "function toString() { [native code] }",
                "function decodeURI() { [native code] }",
                "function asin() { [native code] }",
                "function blink() { [native code] }",
                "function [Symbol.split]() { [native code] }",
                "function get flags() { [native code] }",
                "function get __proto__() { [native code] }",
                "function set __proto__() { [native code] }",
                "true",
                "function () { [native code] }",
                "function () { [native code] }",
                "function actual() { return 42; }",
                "async function /* async */ () { return 42; }",
                "function* /* generator */ () { yield 42; }",
                "true",
                "true",
                "true",
                "length,name",
            ],
        );
    }

    #[test]
    fn regression_dynamic_function_display_name_is_not_a_lexical_binding() {
        assert_output_in_execution_modes(
            r#"
            var created = Function('return typeof anonymous');
            print(created.name);
            print(created.length);
            print(created());
            print(created.toString().startsWith('function anonymous('));
            print(Function('return function() {return typeof anonymous;}')()());
            print(Function("return function() {eval(''); return typeof anonymous;}")()());
            print(Function("return eval('(typeof anonymous)')")());
            print(Function("'use strict'; return typeof anonymous")());
            try {Function('return anonymous')(); print(false);}
            catch (error) {print(error instanceof ReferenceError);}
            globalThis.anonymous = 42;
            print(Function('return anonymous')());
            print(Function('return function() {return anonymous;}')()());
            delete globalThis.anonymous;
            print(Function('anonymous', 'return anonymous')(43));
            print(Function('var anonymous = 44; return anonymous')());
            print((function anonymous() {return typeof anonymous;})());
            var Generator = (function*() {}).constructor;
            var Async = (async function() {}).constructor;
            var AsyncGenerator = (async function*() {}).constructor;
            for (var Constructor of [Generator, Async, AsyncGenerator]) {
                print(Constructor('return typeof anonymous').name);
            }
            print(Generator('return typeof anonymous')().next().value);
            Async('return typeof anonymous')().then(print);
            AsyncGenerator('return typeof anonymous')().next().then(function(step) {
                print(step.value);
            });
            "#,
            &[
                "anonymous",
                "0",
                "undefined",
                "true",
                "undefined",
                "undefined",
                "undefined",
                "undefined",
                "true",
                "42",
                "42",
                "43",
                "44",
                "function",
                "anonymous",
                "anonymous",
                "anonymous",
                "undefined",
                "undefined",
                "undefined",
            ],
        );
    }

    #[test]
    fn regression_function_constructor_classes_use_parameters_and_class_bodies() {
        assert_output_in_execution_modes(
            r#"
            class Base {constructor(value) {this.value = value;}}
            var factory = Function('Parent', 'return class Derived extends Parent { constructor(value) {super(value); this.extra = 1;} #private = 8; get twice() {return this.value * 2;} getPrivate() {return this.#private;} static marker() {return 42;} }');
            var Derived = factory(Base);
            var value = new Derived(21);
            print(value.twice);
            print(value.extra);
            print(value.getPrivate());
            print(Derived.marker());
            print(Derived.name);
            print(Object.getPrototypeOf(Derived) === Base);
            print(value instanceof Base);
            var Generator = (function*() {}).constructor;
            var generatorFactory = Generator('Parent', 'return class Derived extends Parent {field = 42;}');
            var Generated = generatorFactory(Base).next().value;
            print(new Generated().field);
            var Async = (async function() {}).constructor;
            Async('Parent', 'return class Derived extends Parent {field = 43;}')(Base).then(function(Generated) {print(new Generated().field);});
            var AsyncGenerator = (async function*() {}).constructor;
            AsyncGenerator('Parent', 'return class Derived extends Parent {field = 44;}')(Base).next().then(function(step) {print(new step.value().field);});
            "#,
            &[
                "42", "1", "8", "42", "Derived", "true", "true", "42", "43", "44",
            ],
        );
    }

    #[test]
    fn regression_dynamic_class_heritage_is_evaluated_when_the_factory_runs() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            var Current = class First {};
            Object.defineProperty(globalThis, 'Parent', {configurable: true, get() {reads++; $262.gc(); return Current;}});
            var factory = Function('return class Derived extends Parent {field = 42; static value = 7;}');
            print(reads);
            Current = class Second {};
            var Derived = factory();
            print(reads);
            print(Object.getPrototypeOf(Derived) === Current);
            print(new Derived().field);
            print(Derived.value);
            print(Derived.name);
            try {Derived(); print(false);} catch (error) {print(error instanceof TypeError);}
            var Missing = Function('return class Derived extends MissingParent {}');
            print(typeof Missing);
            try {Missing(); print(false);} catch (error) {print(error instanceof ReferenceError);}
            try {Function('return class Derived extends Object { missing('); print(false);}
            catch (error) {print(error instanceof SyntaxError);}
            "#,
            &[
                "0", "1", "true", "42", "7", "Derived", "true", "function", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_buffer_intrinsics_ignore_replaced_globals() {
        assert_output_in_execution_modes(
            r#"
            var constructors = [ArrayBuffer, SharedArrayBuffer, DataView];
            var prototypes = constructors.map(function(Constructor) {return Constructor.prototype;});
            var buffer = new ArrayBuffer(8);
            var reads = 0;
            for (var name of ['ArrayBuffer', 'SharedArrayBuffer', 'DataView']) {
                Object.defineProperty(globalThis, name, {configurable: true, get() {reads++; $262.gc(); throw 'global getter';}});
            }
            function Target() {}
            Target.prototype = null;
            for (var index = 0; index < constructors.length; index++) {
                var args = index === 2 ? [buffer] : [8];
                var result = Reflect.construct(constructors[index], args, Target);
                print(Object.getPrototypeOf(result) === prototypes[index]);
                print(result.byteLength);
            }
            print(new constructors[2](buffer).byteLength);
            print(reads);
            "#,
            &["true", "8", "true", "8", "true", "8", "8", "0"],
        );
    }

    #[test]
    fn regression_buffer_fallback_uses_foreign_intrinsics() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm();
            var names = ['ArrayBuffer', 'SharedArrayBuffer', 'DataView'];
            var constructors = [ArrayBuffer, SharedArrayBuffer, DataView];
            var prototypes = names.map(function(name) {return foreign.global[name].prototype;});
            foreign.evalScript('globalThis.Target = function Target() {}; Target.prototype = null;');
            var buffer = new ArrayBuffer(8);
            var reads = 0;
            for (var name of names) {
                Object.defineProperty(foreign.global, name, {configurable: true, get() {reads++; $262.gc(); throw 'foreign getter';}});
            }
            $262.gc();
            for (var index = 0; index < constructors.length; index++) {
                var result = Reflect.construct(constructors[index], index === 2 ? [buffer] : [8], foreign.global.Target);
                print(Object.getPrototypeOf(result) === prototypes[index]);
                print(result.byteLength);
            }
            print(reads);
            "#,
            &["true", "8", "true", "8", "true", "8", "0"],
        );
    }

    #[test]
    fn regression_group_by_retains_values_across_collecting_callbacks() {
        assert_output_in_execution_modes(
            r#"
            for (var group of [Map.groupBy, Object.groupBy]) {
                var nextReads = 0;
                var source = {[Symbol.iterator]() {
                    var index = 0;
                    return {get next() {
                        nextReads++; $262.gc();
                        return function() {
                            $262.gc();
                            if (index === 3) return {done: true};
                            return {get done() {$262.gc(); return false;}, get value() {$262.gc(); return {rank: ++index};}};
                        };
                    }};
                }};
                var result = group(source, function(value, index) {$262.gc(); return value.rank % 2;});
                var odd = group === Map.groupBy ? result.get(1) : result[1];
                var even = group === Map.groupBy ? result.get(0) : result[0];
                print(odd.map(value => value.rank).join(','));
                print(even[0].rank);
                print(nextReads);
            }
            var grouped = Map.groupBy([1, 2], function(value) {$262.gc(); return {rank: value};});
            print(Array.from(grouped.keys()).map(key => key.rank).join(','));
            print(Map.groupBy([1], () => -0).keys().next().value === 0);
            print(Object.is(Map.groupBy([1], () => -0).keys().next().value, -0));
            "#,
            &["1,3", "2", "1", "1,3", "2", "1", "1,2", "true", "false"],
        );
    }

    #[test]
    fn regression_group_by_preserves_iterator_abrupt_completion() {
        assert_output_in_execution_modes(
            r#"
            for (var group of [Map.groupBy, Object.groupBy]) {
                for (var phase of ['next', 'done', 'value', 'callback']) {
                    var closes = 0;
                    var source = {[Symbol.iterator]() {return {
                        next() {
                            $262.gc();
                            if (phase === 'next') throw {kind: phase};
                            return {get done() {$262.gc(); if (phase === 'done') throw {kind: phase}; return false;},
                                get value() {$262.gc(); if (phase === 'value') throw {kind: phase}; return 1;}};
                        },
                        get return() {closes++; $262.gc(); return function() {$262.gc(); throw {kind: 'close'};};}
                    };}};
                    try {group(source, function() {$262.gc(); throw {kind: phase};}); print(false);}
                    catch (error) {print(error.kind);}
                    print(closes);
                }
            }
            var closes = 0;
            var source = {[Symbol.iterator]() {return {next() {return {value: 1, done: false};},
                get return() {closes++; $262.gc(); throw {kind: 'close'};}};}};
            try {Object.groupBy(source, function() {return {[Symbol.toPrimitive]() {$262.gc(); throw {kind: 'key'};}};}); print(false);}
            catch (error) {print(error.kind);}
            print(closes);
            var result = Object.groupBy([{rank: 42}], function() {return {[Symbol.toPrimitive]() {$262.gc(); return '__proto__';}};});
            print(Object.getPrototypeOf(result) === null);
            print(result.__proto__[0].rank);
            var descriptor = Object.getOwnPropertyDescriptor(result, '__proto__');
            print(descriptor.writable && descriptor.enumerable && descriptor.configurable);
            "#,
            &[
                "next", "0", "done", "0", "value", "0", "callback", "1", "next", "0", "done", "0",
                "value", "0", "callback", "1", "key", "1", "true", "42", "true",
            ],
        );
    }

    #[test]
    fn regression_from_entries_retains_collecting_entry_operands() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            var symbol = Symbol('entry');
            var events = [];
            var source = {[Symbol.iterator]() {var index = 0; return {get next() {
                reads++; $262.gc(); return function() {
                    $262.gc(); if (index === 2) return {done:true};
                    var rank = ++index;
                    return {get done() {$262.gc(); return false;}, get value() {
                        $262.gc(); return {get 0() {
                            events.push('key'); $262.gc();
                            return {[Symbol.toPrimitive]() {events.push('convert'); $262.gc(); return rank === 1 ? '__proto__' : symbol;}};
                        }, get 1() {events.push('value'); $262.gc(); return {rank:rank};}};
                    }};
                };
            }};}};
            var result = Object.fromEntries(source);
            print(reads);
            print(events.join(','));
            print(Object.getPrototypeOf(result) === Object.prototype);
            print(result.__proto__.rank);
            print(result[symbol].rank);
            var descriptor = Object.getOwnPropertyDescriptor(result, '__proto__');
            print(descriptor.writable && descriptor.enumerable && descriptor.configurable);
            print(Reflect.ownKeys(result).length);
            var realm = $262.createRealm();
            var foreignObject = realm.global.Object;
            var fromEntries = foreignObject.fromEntries;
            var prototype = foreignObject.prototype;
            Object.defineProperty(realm.global, 'Object', {configurable:true, get() {$262.gc(); throw 'global-read';}});
            var foreign = fromEntries([['answer', 42]]);
            print(Object.getPrototypeOf(foreign) === prototype);
            print(foreign.answer);
            "#,
            &[
                "1",
                "key,value,convert,key,value,convert",
                "true",
                "1",
                "2",
                "true",
                "2",
                "true",
                "42",
            ],
        );
    }

    #[test]
    fn regression_from_entries_avoids_descriptor_prototype_effects() {
        assert_output_in_execution_modes(
            r#"
            var calls = 0;
            var names = ['value', 'writable', 'enumerable', 'configurable', 'get', 'set', 'entry'];
            for (var name of names) {
                var descriptor = Object.create(null);
                descriptor.configurable = true;
                descriptor.set = function() {calls++; $262.gc(); throw {kind:'prototype-hook'};};
                Object.defineProperty(Object.prototype, name, descriptor);
            }
            var result;
            try {result = Object.fromEntries([['entry', {rank:42}], ['entry', {rank:43}]]);}
            finally {for (var name of names) delete Object.prototype[name];}
            print(calls);
            print(result.entry.rank);
            var descriptor = Object.getOwnPropertyDescriptor(result, 'entry');
            print(descriptor.writable && descriptor.enumerable && descriptor.configurable);
            "#,
            &["0", "43", "true"],
        );
    }

    #[test]
    fn regression_from_entries_preserves_iterator_abrupt_completion() {
        assert_output_in_execution_modes(
            r#"
            for (var phase of ['next-getter', 'next', 'done', 'value', 'entry', 'key', 'entry-value', 'convert']) {
                var closes = 0;
                var source = {[Symbol.iterator]() {return {
                    get next() {$262.gc(); if (phase === 'next-getter') throw {kind:phase}; return function() {
                        $262.gc(); if (phase === 'next') throw {kind:phase};
                        return {get done() {$262.gc(); if (phase === 'done') throw {kind:phase}; return false;},
                            get value() {$262.gc(); if (phase === 'value') throw {kind:phase}; if (phase === 'entry') return 1;
                                return {get 0() {$262.gc(); if (phase === 'key') throw {kind:phase};
                                    return {[Symbol.toPrimitive]() {$262.gc(); throw {kind:phase};}};},
                                    get 1() {$262.gc(); if (phase === 'entry-value') throw {kind:phase}; return {rank:42};}};}}
                    ;};},
                    get return() {closes++; $262.gc(); return function() {$262.gc(); throw {kind:'close'};};}
                };}};
                try {Object.fromEntries(source); print(false);}
                catch (error) {print(phase === 'entry' ? error instanceof TypeError : error.kind === phase);}
                print(closes);
            }
            "#,
            &[
                "true", "0", "true", "0", "true", "0", "true", "0", "true", "1", "true", "1",
                "true", "1", "true", "1",
            ],
        );
    }

    #[test]
    fn regression_descriptor_fields_survive_later_collecting_getters() {
        assert_output_in_execution_modes(
            r#"
            for (var define of [Object.defineProperty, Reflect.defineProperty]) {
                var target = {};
                var events = [];
                var descriptor = {enumerable:true, configurable:true,
                    get value() {events.push('value'); $262.gc(); return {rank:42};},
                    get writable() {events.push('writable'); $262.gc(); return true;}};
                define(target, 'answer', descriptor);
                print(target.answer.rank);
                print(events.join(','));
                var descriptor = {enumerable:true, configurable:true,
                    get get() {events.push('get'); $262.gc(); return function() {return 43;};},
                    get set() {events.push('set'); $262.gc(); return function(value) {this.saved = value;};}};
                define(target, 'accessor', descriptor);
                print(target.accessor);
                target.accessor = 44;
                print(target.saved);
                print(events.join(','));
            }
            "#,
            &[
                "42",
                "value,writable",
                "43",
                "44",
                "value,writable,get,set",
                "42",
                "value,writable",
                "43",
                "44",
                "value,writable,get,set",
            ],
        );
    }

    #[test]
    fn regression_descriptor_validation_stops_before_setter_lookup() {
        assert_output_in_execution_modes(
            r#"
            for (var define of [Object.defineProperty, Reflect.defineProperty]) {
                var reads = 0;
                try {define({}, 'answer', {get get() {$262.gc(); return 1;},
                    get set() {reads++; $262.gc(); throw {kind:'setter'};}});}
                catch (error) {print(error instanceof TypeError);}
                print(reads);
            }
            var reads = 0;
            try {Reflect.defineProperty(1, {[Symbol.toPrimitive]() {reads++; throw 'key';}}, {});}
            catch (error) {print(error instanceof TypeError);}
            print(reads);
            "#,
            &["true", "0", "true", "0", "true", "0"],
        );
    }

    #[test]
    fn regression_descriptor_projection_survives_collecting_proxy_lookup() {
        assert_output_in_execution_modes(
            r#"
            var target = {};
            var proxy = new Proxy(target, {get defineProperty() {
                $262.gc(); return function(target, key, descriptor) {
                    $262.gc(); print(Reflect.ownKeys(descriptor).join(','));
                    print(descriptor.value.rank);
                    print(typeof key === 'symbol');
                    Object.defineProperty(target, key, descriptor);
                    return true;
                };
            }});
            print(Reflect.defineProperty(proxy, {[Symbol.toPrimitive]() {$262.gc(); return Symbol('entry');}},
                {enumerable:true, configurable:true, get value() {$262.gc(); return {rank:45};},
                    get writable() {$262.gc(); return true;}}));
            print(target[Object.getOwnPropertySymbols(target)[0]].rank);
            var realm = $262.createRealm();
            var descriptor = realm.global.Object.getOwnPropertyDescriptor({answer:46}, 'answer');
            print(Object.getPrototypeOf(descriptor) === realm.global.Object.prototype);
            print(descriptor.value);
            var revocable = Proxy.revocable({rank:47}, {get defineProperty() {
                revocable.revoke(); $262.gc(); return function(target, key, descriptor) {
                    $262.gc(); print(target.rank); print(descriptor.value.rank); return true;
                };
            }});
            print(Reflect.defineProperty(revocable.proxy, 'entry', {configurable:true,
                get value() {$262.gc(); return {rank:48};}, get writable() {$262.gc(); return true;}}));
            "#,
            &[
                "value,writable,enumerable,configurable",
                "45",
                "true",
                "true",
                "45",
                "true",
                "46",
                "47",
                "48",
                "true",
            ],
        );
    }

    #[test]
    fn regression_definition_modes_execute_after_residual_round_trip() {
        let source = r#"
            var calls = 0, savedDefine = Object.defineProperty;
            Object.defineProperty = function() {calls++; throw 'guest define';};
            var instance, literal;
            try {
                class Encoded {
                    #privateMethod() {return 41;}
                    method() {return this.#privateMethod() + 1;}
                    get value() {return this.stored;}
                    set value(next) {this.stored = next;}
                }
                instance = new Encoded(); instance.value = 43;
                literal = {get value() {return this.stored;}, set value(next) {this.stored = next;}};
                literal.value = 44;
            } finally {Object.defineProperty = savedDefine;}
            print(calls); print(instance.method()); print(instance.value); print(literal.value);
            var prototype = Object.getPrototypeOf(instance);
            var method = Object.getOwnPropertyDescriptor(prototype,'method');
            var accessor = Object.getOwnPropertyDescriptor(prototype,'value');
            var own = Object.getOwnPropertyDescriptor(literal,'value');
            print(method.writable); print(method.enumerable); print(method.configurable);
            print(typeof accessor.get); print(typeof accessor.set); print(accessor.enumerable);
            print(typeof own.get); print(typeof own.set); print(own.enumerable);
        "#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "definition-modes.js").unwrap();
            let path = std::env::temp_dir()
                .join(format!("quench-definition-modes-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path);
            std::fs::remove_file(path).unwrap();
            let decoded = decoded.unwrap();
            let host = Capture::default();
            let output = host.clone();
            let mut runtime = Runtime::new(host);
            runtime.execute(&decoded).unwrap();
            assert_eq!(
                output.0.borrow().as_slice(),
                &[
                    "0", "42", "43", "44", "true", "false", "true", "function", "function",
                    "false", "function", "function", "true"
                ]
            );
        }
    }

    #[test]
    fn regression_callable_validation_uses_proxy_target_kind() {
        assert_output_in_execution_modes(
            r#"
            var live = new Proxy(function() {return 42;}, {}), target = {};
            Object.prototype.__defineGetter__.call(target,'live',live); print(target.live);
            var revoked = Proxy.revocable(function() {}, {}); revoked.revoke();
            Object.prototype.__defineGetter__.call(target,'revoked',revoked.proxy);
            print(Object.getOwnPropertyDescriptor(target,'revoked').get === revoked.proxy);
            try {target.revoked; print(false);} catch (error) {print(error instanceof TypeError);}
            print([].map(revoked.proxy).length);
            try {JSON.parse('1',revoked.proxy); print(false);} catch (error) {print(error instanceof TypeError);}
            new Map([['entry',1]]).forEach(new Proxy(function(value) {$262.gc(); print(value);}, {}));
            new Set([2]).forEach(new Proxy(function(value) {$262.gc(); print(value);}, {}));
            print(new WeakMap().getOrInsertComputed({},new Proxy(function() {$262.gc(); return {rank:3};},{})).rank);
            var nested = function() {};
            for (var index = 0; index < 5000; index++) nested = new Proxy(nested, {});
            $262.gc(); Object.prototype.__defineSetter__.call(target,'nested',nested);
            print(Object.getOwnPropertyDescriptor(target,'nested').set === nested);
            "#,
            &["42", "true", "true", "0", "true", "1", "2", "3", "true"],
        );
    }

    #[test]
    fn regression_proxy_set_shares_string_and_symbol_semantics() {
        assert_output_in_execution_modes(
            r#"
            function outcome(action) {
                try {return String(action());} catch (error) {return error instanceof TypeError ? 'TypeError' : 'other error';}
            }
            for (var key of ['entry', Symbol('entry')]) {
                var denied = new Proxy({}, {set() {return false;}});
                print(outcome(function() {return Reflect.set(denied, key, 42);}));
                print(outcome(function() {denied[key] = 42; return 'sloppy';}));
                print(outcome(function() {'use strict'; denied[key] = 42;}));
                print(outcome(function() {return Reflect.set(new Proxy({}, {set: 1}), key, 42);}));
                for (var pair of [[42, 43], [NaN, NaN], [0, -0]]) {
                    var target = {}; Object.defineProperty(target, key, {value:pair[0]});
                    print(outcome(function() {return Reflect.set(new Proxy(target, {set() {return true;}}), key, pair[1]);}));
                }
                var accessor = {}; Object.defineProperty(accessor, key, {get() {return 99;}});
                print(outcome(function() {return Reflect.set(new Proxy(accessor, {set() {return true;}}), key, 42);}));
                var receiver = {rank:43}, events = [];
                var target = {}; Object.defineProperty(target, key, {set(value) {
                    $262.gc(); events.push(this === receiver); events.push(value.rank);
                }});
                print(Reflect.set(new Proxy(target, {set:null}), key, {rank:42}, receiver));
                print(events.join(','));
                var writes = 0, target = {};
                var forwarded = new Proxy(target, {defineProperty(object, observed, descriptor) {
                    $262.gc(); writes++; print(observed === key); print(Object.keys(descriptor).join(','));
                    return Reflect.defineProperty(object, observed, descriptor);
                }});
                print(Reflect.set(forwarded, key, 44)); print(target[key]); print(writes);
                print(Reflect.set(forwarded, key, 45)); print(target[key]); print(writes);
                var revoked = Proxy.revocable({}, {}); revoked.revoke();
                print(outcome(function() {return Reflect.set(revoked.proxy, key, 42);}));
            }
            "#,
            &[
                "false",
                "sloppy",
                "TypeError",
                "TypeError",
                "TypeError",
                "true",
                "TypeError",
                "TypeError",
                "true",
                "true,42",
                "true",
                "value,writable,enumerable,configurable",
                "true",
                "44",
                "1",
                "true",
                "value",
                "true",
                "45",
                "2",
                "TypeError",
                "false",
                "sloppy",
                "TypeError",
                "TypeError",
                "TypeError",
                "true",
                "TypeError",
                "TypeError",
                "true",
                "true,42",
                "true",
                "value,writable,enumerable,configurable",
                "true",
                "44",
                "1",
                "true",
                "value",
                "true",
                "45",
                "2",
                "TypeError",
            ],
        );
    }

    #[test]
    fn regression_static_property_reads_respect_exotic_index_storage() {
        assert_output_in_execution_modes(
            r#"
            function first(value) {return value['0'];}
            print(first([42]));
            print(first(Object.create([43])));
            print(first(new Uint8Array([44])));
            print(first(Object.create(new Uint8Array([45]))));
            print(first(new String('x')));
            print(first(Object.create(new String('y'))));
            var prototype = {'0': 99};
            var array = []; Object.setPrototypeOf(array, prototype);
            print(first(array)); print(first(array));
            array[0] = 46;
            print(first(array));
            function invalid(value) {return value['-0'];}
            var reads = 0;
            Object.defineProperty(Uint8Array.prototype, '-0', {get() {reads++; return 99;}});
            print(invalid(new Uint8Array([47])));
            print(reads);
            "#,
            &[
                "42",
                "43",
                "44",
                "45",
                "x",
                "y",
                "99",
                "99",
                "46",
                "undefined",
                "0",
            ],
        );
    }

    #[test]
    fn regression_promise_keyed_combinators_resolve_each_property_before_reading_the_next() {
        assert_output_in_execution_modes(
            r#"
            function verify(name) {
                var events = [];
                var source = {get first() {events.push('get:first'); return 42;},
                    get later() {events.push('get:later'); return 43;}};
                class Constructor extends Promise {static resolve(value) {
                    events.push('resolve'); delete source.later; $262.gc();
                    return {then(resolve) {events.push('then'); resolve(value);}};
                }}
                Promise[name].call(Constructor, source).then(function(result) {
                    print(events.join(',')); print(Object.keys(result).join(','));
                    print(name === 'allKeyed' ? result.first : result.first.value);
                });
            }
            Object.defineProperty(Array.prototype, Symbol.iterator, {get() {throw 'synthetic iterator';}, configurable:true});
            verify('allKeyed'); verify('allSettledKeyed');
            "#,
            &[
                "get:first,resolve,then",
                "first",
                "42",
                "get:first,resolve,then",
                "first",
                "42",
            ],
        );
    }

    #[test]
    fn regression_promise_combinators_keep_original_error_through_iterator_close() {
        assert_output_in_execution_modes(
            r#"
            class Constructor extends Promise {static resolve() {throw {rank:42};}}
            for (var name of ['all', 'allSettled', 'any', 'race']) {
                var iterable = {[Symbol.iterator]() {return {
                    next() {return {done:false, value:1};},
                    return() {$262.gc(); throw {rank:99};}
                };}};
                (function(name) {Promise[name].call(Constructor, iterable).catch(function(error) {print(name + ':' + error.rank);});})(name);
            }
            "#,
            &["all:42", "allSettled:42", "any:42", "race:42"],
        );
    }

    #[test]
    fn regression_promise_keyed_combinators_root_earlier_values_during_later_getters() {
        assert_output_in_execution_modes(
            r#"
            var symbol = Symbol('entry');
            for (var name of ['allKeyed', 'allSettledKeyed']) {
                var source = {
                    get first() {return {rank:42};},
                    get [symbol]() {$262.gc(); return {rank:43};},
                    get last() {$262.gc(); return {rank:44};}
                };
                (function(name) {Promise[name](source).then(function(result) {
                    print(name === 'allKeyed' ? result.first.rank : result.first.value.rank);
                    print(name === 'allKeyed' ? result[symbol].rank : result[symbol].value.rank);
                    print(name === 'allKeyed' ? result.last.rank : result.last.value.rank);
                });})(name);
            }
            "#,
            &["42", "43", "44", "42", "43", "44"],
        );
    }

    #[test]
    fn regression_promise_combinators_cache_and_root_iterator_next() {
        assert_output_in_execution_modes(
            r#"
            function source() {
                return {[Symbol.iterator]() {
                    var reads = 0, complete = false;
                    return {get next() {
                        print('next:' + ++reads);
                        return function() {
                            $262.gc();
                            Object.defineProperty(this, 'next', {value:function() {throw 'replacement';}});
                            if (complete) return {done:true};
                            complete = true;
                            return {done:false, value:42};
                        };
                    }};
                }};
            }
            for (var name of ['all', 'allSettled', 'any', 'race']) {
                (function(name) {Promise[name](source()).then(function(result) {
                    print(name + ':' + JSON.stringify(result));
                }, function(error) {print('rejected:' + error);});})(name);
            }
            "#,
            &[
                "next:1",
                "next:1",
                "next:1",
                "next:1",
                "all:[42]",
                "allSettled:[{\"status\":\"fulfilled\",\"value\":42}]",
                "any:42",
                "race:42",
            ],
        );
    }

    #[test]
    fn regression_promise_combinators_root_fresh_resolve_methods() {
        assert_output_in_execution_modes(
            r#"
            class Constructor extends Promise {
                static get resolve() {return function(value) {print(this === Constructor); return Promise.resolve(value);};}
            }
            function iterable() {
                return {get [Symbol.iterator]() {$262.gc(); return function() {
                    var complete = false;
                    return {next() {$262.gc(); if (complete) return {done:true}; complete = true; return {done:false, value:42};}};
                };}};
            }
            for (var name of ['all', 'allSettled', 'any', 'race']) {
                (function(name) {Promise[name].call(Constructor, iterable()).then(function(value) {
                    print(name + ':' + JSON.stringify(value));
                }, function(error) {print(name + ':rejected:' + error.name);});})(name);
            }
            "#,
            &[
                "true",
                "true",
                "true",
                "true",
                "all:[42]",
                "allSettled:[{\"status\":\"fulfilled\",\"value\":42}]",
                "any:42",
                "race:42",
            ],
        );
    }

    #[test]
    fn regression_promise_combinators_root_custom_capabilities_before_resolve_lookup() {
        assert_output_in_execution_modes(
            r#"
            function Constructor(executor) {
                executor(function(value) {$262.gc(); print(value.length);}, function(error) {print(error.name);});
                return {rank:42};
            }
            Object.defineProperty(Constructor, 'resolve', {get() {$262.gc(); return function(value) {return value;};}});
            print(Promise.all.call(Constructor, []).rank);
            "#,
            &["0", "42"],
        );
    }

    #[test]
    fn regression_promise_then_observes_state_after_species_and_capability_effects() {
        assert_output_in_execution_modes(
            r#"
            var ordinal = 0;
            for (var phase of ['constructor', 'species', 'capability']) {
                for (var rejected of [false, true]) {
                    (function(phase, rejected, index) {
                        var settle;
                        var source = new Promise(function(resolve, reject) {settle = rejected ? reject : resolve;});
                        var value = {rank: index};
                        function effect() {
                            print('effect:' + index); $262.gc(); settle(value);
                            Object.defineProperty(source, 'constructor', {value:Promise, writable:true, configurable:true});
                            source.then(function(actual) {print('inner:' + index + ':' + (actual === value));},
                                function(actual) {print('inner:' + index + ':' + (actual === value));});
                        }
                        if (phase === 'constructor') {
                            Object.defineProperty(source, 'constructor', {get() {effect(); return Promise;}, configurable:true});
                        } else if (phase === 'species') {
                            source.constructor = {get [Symbol.species]() {effect(); return Promise;}};
                        } else {
                            source.constructor = {[Symbol.species]: function(executor) {effect(); return new Promise(executor);}};
                        }
                        source.then(function(actual) {print('outer:' + index + ':' + (actual === value)); return index;},
                            function(actual) {print('outer:' + index + ':' + (actual === value)); return index;})
                            .then(function(actual) {print('next:' + actual);});
                    })(phase, rejected, ordinal++);
                }
            }
            "#,
            &[
                "effect:0",
                "effect:1",
                "effect:2",
                "effect:3",
                "effect:4",
                "effect:5",
                "inner:0:true",
                "outer:0:true",
                "inner:1:true",
                "outer:1:true",
                "inner:2:true",
                "outer:2:true",
                "inner:3:true",
                "outer:3:true",
                "inner:4:true",
                "outer:4:true",
                "inner:5:true",
                "outer:5:true",
                "next:0",
                "next:1",
                "next:2",
                "next:3",
                "next:4",
                "next:5",
            ],
        );
    }

    #[test]
    fn regression_promise_default_species_uses_method_realm_intrinsics() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var home = foreign.Promise, local = Promise;
            var foreignThen = home.prototype.then, localThen = local.prototype.then;
            home.prototype.constructor = function Poison() {throw 'prototype constructor';};
            foreign.Promise = function Poison() {throw 'global constructor';};
            globalThis.Promise = function Poison() {throw 'local global constructor';};
            for (var constructor of [undefined, {[Symbol.species]: null}, {get [Symbol.species]() {$262.gc(); return undefined;}}]) {
                var source = local.resolve(42); source.constructor = constructor;
                var result = foreignThen.call(source);
                print(Object.getPrototypeOf(result) === home.prototype);
                var other = home.resolve(43); other.constructor = constructor;
                print(Object.getPrototypeOf(localThen.call(other)) === local.prototype);
            }
            "#,
            &["true", "true", "true", "true", "true", "true"],
        );
    }

    #[test]
    fn regression_promise_finally_selects_species_before_then_for_every_handler() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var finallyMethod = foreign.Promise.prototype.finally;
            for (var handler of [undefined, 1, function() {}]) {
                var order = [];
                var source = {
                    get constructor() {order.push('constructor'); $262.gc(); return {
                        get [Symbol.species]() {order.push('species'); $262.gc(); return undefined;}
                    };},
                    get then() {order.push('then'); $262.gc(); return function(a, b) {
                        order.push('call'); print(handler === undefined || handler === 1 ? a === handler && b === handler : typeof a === 'function' && typeof b === 'function');
                        return 42;
                    };}
                };
                print(finallyMethod.call(source, handler)); print(order.join(','));
            }
            var reads = 0, marker = {};
            var invalid = {constructor: {[Symbol.species]: () => {}}, get then() {reads++; throw marker;}};
            try {finallyMethod.call(invalid);} catch (error) {print(error instanceof foreign.TypeError);}
            print(reads);
            var abrupt = {get constructor() {throw marker;}, get then() {reads++;}};
            try {finallyMethod.call(abrupt, null);} catch (error) {print(error === marker);}
            print(reads);
            for (var primitive of [1, true, 's', Symbol(), 1n]) {
                try {finallyMethod.call(primitive);} catch (error) {print(error instanceof foreign.TypeError);}
            }
            "#,
            &[
                "true",
                "42",
                "constructor,species,then,call",
                "true",
                "42",
                "constructor,species,then,call",
                "true",
                "42",
                "constructor,species,then,call",
                "true",
                "0",
                "true",
                "0",
                "true",
                "true",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_foreign_promise_installs_distinct_methods_and_descriptors() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var constructor = foreign.Promise;
            print(constructor !== Promise);
            var prototype = Object.getOwnPropertyDescriptor(constructor, 'prototype');
            print(prototype.writable + ',' + prototype.enumerable + ',' + prototype.configurable);
            var owners = [[constructor, 'resolve', 1], [constructor, 'reject', 1],
                [constructor, 'all', 1], [constructor, 'race', 1], [constructor, 'allSettled', 1],
                [constructor, 'any', 1], [constructor, 'try', 1], [constructor, 'withResolvers', 0],
                [constructor.prototype, 'then', 2], [constructor.prototype, 'catch', 1],
                [constructor.prototype, 'finally', 1]];
            for (var entry of owners) {
                var owner = entry[0], name = entry[1];
                var d = Object.getOwnPropertyDescriptor(owner, name);
                var local = owner === constructor ? Promise : Promise.prototype;
                print(d.value.name === name && d.value.length === entry[2] && d.value !== local[name]
                    && d.writable && !d.enumerable && d.configurable);
            }
            var getter = Object.getOwnPropertyDescriptor(constructor, Symbol.species).get;
            print(getter.call(constructor) === constructor);
            print(constructor.prototype.constructor === constructor);
            print(Object.prototype.toString.call(constructor.prototype));
            try {constructor.any.call({}, []);} catch (error) {print(error instanceof foreign.TypeError);}
            try {constructor.prototype.then.call({});} catch (error) {print(error instanceof foreign.TypeError);}
            $262.gc();
            print(constructor.prototype.then !== Promise.prototype.then);
            "#,
            &[
                "true",
                "false,false,false",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "[object Promise]",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_promise_any_uses_intrinsics_after_global_replacement() {
        assert_output_in_execution_modes(
            r#"
            var home = AggregateError.prototype, arrayHome = Array.prototype;
            globalThis.AggregateError = {prototype: {poison: true}};
            var reason = {rank: 42};
            class Immediate extends Promise {
                static resolve(value) {return {then(resolve, reject) {reject(value);}};}
            }
            function inspect(error) {
                $262.gc();
                var d = Object.getOwnPropertyDescriptor(error, 'errors');
                return [Error.isError(error), Object.getPrototypeOf(error) === home,
                    Object.getPrototypeOf(error.errors) === arrayHome,
                    d.writable, d.enumerable, d.configurable,
                    error.errors.length, error.errors.length === 0 || error.errors[0] === reason].join(',');
            }
            Promise.all([
                Promise.any([]).catch(inspect),
                Promise.any([Promise.reject(reason)]).catch(inspect),
                Promise.any.call(Immediate, [reason]).catch(inspect)
            ]).then(function(results) {for (var result of results) print(result);});
            "#,
            &[
                "true,true,true,true,false,true,0,true",
                "true,true,true,true,false,true,1,true",
                "true,true,true,true,false,true,1,true",
            ],
        );
    }

    #[test]
    fn regression_promise_any_borrowing_preserves_error_and_array_realms() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var localError = AggregateError.prototype, localArray = Array.prototype;
            var foreignError = foreign.AggregateError.prototype, foreignArray = foreign.Array.prototype;
            foreign.AggregateError = {prototype: {poison: true}};
            globalThis.AggregateError = {prototype: {poison: true}};
            function inspect(error, errorHome, arrayHome) {
                $262.gc();
                return [Error.isError(error), Object.getPrototypeOf(error) === errorHome,
                    Object.getPrototypeOf(error.errors) === arrayHome, error.errors.join(',')].join(':');
            }
            function foreignCheck(error) {return inspect(error, foreignError, foreignArray);}
            function localCheck(error) {return inspect(error, localError, localArray);}
            Promise.all([
                foreign.Promise.any.call(Promise, []).catch(foreignCheck),
                foreign.Promise.any.call(Promise, [Promise.reject(1), Promise.reject(2)]).catch(foreignCheck),
                Promise.any.call(foreign.Promise, []).catch(localCheck),
                Promise.any.call(foreign.Promise, [foreign.Promise.reject(3)]).catch(localCheck)
            ]).then(function(results) {for (var result of results) print(result);});
            "#,
            &[
                "true:true:true:",
                "true:true:true:1,2",
                "true:true:true:",
                "true:true:true:3",
            ],
        );
    }

    #[test]
    fn regression_error_brand_is_not_a_guest_property() {
        assert_output_in_execution_modes(
            r#"
            var marker = '\0quench:error-brand';
            var forged = {[marker]: true};
            print(Error.isError(forged));
            print(Object.prototype.toString.call(forged));
            var error = new Error('real');
            print(Object.hasOwn(error, marker));
            error[marker] = false;
            print(Error.isError(error));
            print(Object.prototype.toString.call(error));
            delete error[marker];
            print(Error.isError(error));
            print(Error.isError(Object.create(error)));
            print(Error.isError(Error.prototype));
            var traps = 0;
            var proxy = new Proxy(error, {get() {traps++; throw 'get';}, getOwnPropertyDescriptor() {traps++; throw 'descriptor';}});
            print(Error.isError(proxy));
            var revoked = Proxy.revocable(error, {}); revoked.revoke();
            print(Error.isError(revoked.proxy)); print(traps);
            class Custom extends Error {}
            var custom = new Custom('subclass');
            Object.setPrototypeOf(custom, null);
            $262.gc(); print(Error.isError(custom)); print(Error.isError(error));
            error[Symbol.toStringTag] = 'Custom';
            print(Object.prototype.toString.call(error));
            for (var constructor of [Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError])
                print(Error.isError(new constructor('native')));
            print(Error.isError(new AggregateError([], 'aggregate')));
            print(Error.isError(new SuppressedError(1, 2, 'suppressed')));
            try {null.value;} catch (caught) {print(Error.isError(caught));}
            "#,
            &[
                "false",
                "[object Object]",
                "false",
                "true",
                "[object Error]",
                "true",
                "false",
                "false",
                "false",
                "false",
                "0",
                "true",
                "true",
                "[object Custom]",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_error_brand_survives_realms_and_aggregate_jobs() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var error = new foreign.Error('foreign');
            print(Error.isError(error)); print(foreign.Error.isError(new Error('local')));
            print(Error.isError(new Proxy(error, {})));
            var stack = Object.getOwnPropertyDescriptor(Error.prototype, 'stack').get;
            print(stack.call({'\0quench:error-brand':true}) === undefined);
            error['\0quench:error-brand'] = false;
            $262.gc(); print(stack.call(error));
            Promise.any([]).catch(function(error) {
                print(Error.isError(error)); print(Object.hasOwn(error, '\0quench:error-brand'));
                error['\0quench:error-brand'] = false;
                $262.gc(); print(Error.isError(error)); print(Object.prototype.toString.call(error));
            });
            for (var value of [Object('s'), Object(1), Object(true), Object(1n), Object(Symbol()), [], function() {}])
                print(Error.isError(value));
            "#,
            &[
                "true",
                "true",
                "false",
                "true",
                "Error: foreign",
                "false",
                "false",
                "false",
                "false",
                "false",
                "false",
                "false",
                "true",
                "false",
                "true",
                "[object Error]",
            ],
        );
    }

    #[test]
    fn regression_error_stack_setter_uses_intrinsic_home_and_records() {
        assert_output_in_execution_modes(
            r#"
            var home = Error.prototype;
            var setter = Object.getOwnPropertyDescriptor(home, 'stack').set;
            var reads = 0;
            Object.defineProperty(globalThis, 'Error', {get() {reads++; $262.gc(); throw 'replaced Error';}, configurable:true});
            var poison = Object.create(null);
            poison.get = function() {throw 'inherited descriptor getter';};
            poison.configurable = true;
            Object.defineProperty(Object.prototype, 'get', poison);
            var target = {get inherited() {throw 'unused';}};
            print(setter.call(target, 'saved') === undefined);
            var d = Object.getOwnPropertyDescriptor(target, 'stack');
            print(d.value); print(d.writable); print(d.enumerable); print(d.configurable);
            try {setter.call(home, 'invalid');} catch (error) {print(error instanceof TypeError);}
            try {setter.call({}, 1);} catch (error) {print(error instanceof TypeError);}
            print(reads);
            delete Object.prototype.get;
            "#,
            &["true", "saved", "true", "true", "true", "true", "true", "0"],
        );
    }

    #[test]
    fn regression_error_stack_setter_shares_assignment_and_realm_identity() {
        assert_output_in_execution_modes(
            r#"
            var setter = Object.getOwnPropertyDescriptor(Error.prototype, 'stack').set;
            var data = {};
            Object.defineProperty(data, 'stack', {value:'old', writable:true, configurable:false});
            var observed;
            var accessor = {set stack(v) {$262.gc(); observed = v;}};
            var emptyAccessor = {};
            Object.defineProperty(emptyAccessor, 'stack', {get:undefined, set:undefined});
            var poison = Object.create(null);
            poison.get = function() {throw 'inherited descriptor getter';};
            poison.configurable = true;
            Object.defineProperty(Object.prototype, 'get', poison);
            setter.call(data, 'updated'); print(data.stack);
            print(Object.getOwnPropertyDescriptor(data, 'stack').enumerable);
            setter.call(accessor, 'accessor'); print(observed);
            try {setter.call(emptyAccessor, 'invalid');} catch (error) {print(error instanceof TypeError);}
            delete Object.prototype.get;
            var trace = [];
            var proxy = new Proxy({}, {
                getOwnPropertyDescriptor(t,k) {$262.gc(); trace.push('get:' + k); return Reflect.getOwnPropertyDescriptor(t,k);},
                defineProperty(t,k,d) {$262.gc(); trace.push('define:' + k + ':' + Object.keys(d)); return Reflect.defineProperty(t,k,d);}
            });
            setter.call(proxy, 'fresh' + '-stack'); print(proxy.stack); print(trace.join(';'));
            var foreign = $262.createRealm().global;
            var foreignHome = foreign.Error.prototype;
            var foreignSetter = Object.getOwnPropertyDescriptor(foreignHome, 'stack').set;
            Object.defineProperty(foreign, 'Error', {get() {$262.gc(); throw 'foreign Error';}, configurable:true});
            try {foreignSetter.call(foreignHome, 'invalid');} catch (error) {print(error instanceof foreign.TypeError);}
            var foreignTarget = new foreign.Object();
            foreignSetter.call(foreignTarget, 'foreign'); print(foreignTarget.stack);
            "#,
            &[
                "updated",
                "false",
                "accessor",
                "true",
                "fresh-stack",
                "get:stack;define:stack:value,writable,enumerable,configurable",
                "true",
                "foreign",
            ],
        );
    }

    #[test]
    fn regression_integrity_records_preserve_proxy_order_and_isolation() {
        assert_output_in_execution_modes(
            r#"
            var trace = [];
            var target = {answer:42};
            var sealed = new Proxy(target, {
                preventExtensions(t) {trace.push('prevent'); return Reflect.preventExtensions(t);},
                ownKeys(t) {trace.push('keys'); return Reflect.ownKeys(t);},
                getOwnPropertyDescriptor() {throw 'unexpected descriptor read';},
                defineProperty(t, k, d) {trace.push('define:' + k + ':' + Object.keys(d)); Object.setPrototypeOf(d, null); return Reflect.defineProperty(t, k, d);}
            });
            print(Object.seal(sealed) === sealed);
            print(trace.join(';'));
            trace = [];
            target = {answer:43, get accessor() {return 44;}};
            var frozen = new Proxy(target, {
                preventExtensions(t) {trace.push('prevent'); return Reflect.preventExtensions(t);},
                ownKeys(t) {trace.push('keys'); return Reflect.ownKeys(t);},
                getOwnPropertyDescriptor(t, k) {trace.push('get:' + k); var d = Reflect.getOwnPropertyDescriptor(t, k); Object.setPrototypeOf(d, null); return d;},
                defineProperty(t, k, d) {trace.push('define:' + k + ':' + Object.keys(d)); Object.setPrototypeOf(d, null); return Reflect.defineProperty(t, k, d);}
            });
            var poisonValue = {get() {throw 'inherited descriptor value';}, configurable:true};
            var poisonGet = {get() {throw 'inherited descriptor getter';}, configurable:true};
            Object.setPrototypeOf(poisonGet, null);
            Object.defineProperty(Object.prototype, 'value', poisonValue);
            Object.defineProperty(Object.prototype, 'get', poisonGet);
            print(Object.freeze(frozen) === frozen);
            delete Object.prototype.value;
            delete Object.prototype.get;
            print(trace.join(';'));
            print(target.answer);
            print(target.accessor);
            print(Object.isFrozen(target));
            "#,
            &[
                "true",
                "prevent;keys;define:answer:configurable",
                "true",
                "prevent;keys;get:answer;define:answer:writable,configurable;get:accessor;define:accessor:configurable",
                "43",
                "44",
                "true",
            ],
        );
    }

    #[test]
    fn regression_integrity_key_snapshot_survives_collecting_traps() {
        assert_output_in_execution_modes(
            r#"
            for (var freeze of [false, true]) {
                var trace = [];
                var target = {alpha:42, beta:43};
                var proxy = new Proxy(target, {
                    preventExtensions(t) {$262.gc(); return Reflect.preventExtensions(t);},
                    ownKeys() {return ['al' + 'pha', 'be' + 'ta'];},
                    getOwnPropertyDescriptor(t,k) {$262.gc(); return Reflect.getOwnPropertyDescriptor(t,k);},
                    defineProperty(t,k,d) {$262.gc(); trace.push(k); return Reflect.defineProperty(t,k,d);}
                });
                print((freeze ? Object.freeze(proxy) : Object.seal(proxy)) === proxy);
                print(trace.join(','));
                print(freeze ? Object.isFrozen(proxy) : Object.isSealed(proxy));
                print(target.alpha + target.beta);
            }
            "#,
            &[
                "true",
                "alpha,beta",
                "true",
                "85",
                "true",
                "alpha,beta",
                "true",
                "85",
            ],
        );
    }

    #[test]
    fn regression_integrity_observes_disappearing_snapshot_properties() {
        assert_output_in_execution_modes(
            r#"
            for (var freeze of [false, true]) {
                var target = {alpha:42, beta:43};
                Object.defineProperty(target, 'alpha', {configurable:false, writable:!freeze});
                Object.preventExtensions(target);
                var trace = [];
                var proxy = new Proxy(target, {
                    isExtensible(t) {$262.gc(); trace.push('extensible'); return Reflect.isExtensible(t);},
                    ownKeys(t) {$262.gc(); trace.push('keys'); return Reflect.ownKeys(t);},
                    getOwnPropertyDescriptor(t,k) {$262.gc(); trace.push(k); delete t.beta; return Reflect.getOwnPropertyDescriptor(t,k);}
                });
                print(freeze ? Object.isFrozen(proxy) : Object.isSealed(proxy));
                print(trace.join(','));
                target = {alpha:42, beta:43};
                trace = [];
                proxy = new Proxy(target, {
                    defineProperty(t,k,d) {$262.gc(); trace.push(k); delete t.beta; return Reflect.defineProperty(t,k,d);}
                });
                try {print((freeze ? Object.freeze(proxy) : Object.seal(proxy)) === proxy);}
                catch (error) {print(error instanceof TypeError);}
                print(trace.join(','));
                print(Object.isExtensible(target));
                print(Object.getOwnPropertyDescriptor(target, 'alpha').configurable);
                print('beta' in target);
            }
            var extensible = new Proxy({}, {ownKeys() {throw 'unexpected ownKeys';}});
            print(Object.isFrozen(extensible));
            print(Object.isSealed(extensible));
            "#,
            &[
                "true",
                "extensible,keys,alpha,beta",
                "true",
                "alpha,beta",
                "false",
                "false",
                "false",
                "true",
                "extensible,keys,alpha,beta",
                "true",
                "alpha",
                "false",
                "false",
                "false",
                "false",
                "false",
            ],
        );
    }

    #[test]
    fn regression_integrity_namespace_records_preserve_live_exports() {
        const DEPENDENCY: &str = "export let answer = 42; export let method = function() {return answer;}; export function change() {answer = 43; method = function() {return answer + 1;};}";
        const SOURCE: &str = r#"
            import * as ns from './integrity-dependency.mjs';
            var poison = Object.create(null);
            poison.get = function() {throw 'inherited descriptor getter';};
            poison.configurable = true;
            Object.defineProperty(Object.prototype, 'get', poison);
            print(Object.seal(ns) === ns);
            print(Object.isSealed(ns));
            print(Object.isFrozen(ns));
            try {Object.freeze(ns);}
            catch (error) {print(error instanceof TypeError);}
            delete Object.prototype.get;
            var inherited = Object.create(ns);
            function read(value) {return value.answer;}
            function call(value) {return value.method();}
            for (var index = 0; index < 5; index++) {read(ns); read(inherited); call(ns); call(inherited);}
            print(ns.answer);
            ns.change();
            print(ns.answer);
            print(read(ns)); print(read(inherited));
            print(call(ns)); print(call(inherited));
            print(Object.defineProperty(ns, 'answer', {value:43}) === ns);
            print(Reflect.defineProperty(ns, 'answer', {value:42}));
            print(Object.getOwnPropertyDescriptor(ns, 'answer').value);
            print(Object.getOwnPropertyDescriptor(ns, 'answer').writable);
            print(Object.getOwnPropertyDescriptor(ns, Symbol.toStringTag).writable);
        "#;
        struct ModuleCapture(Capture);
        impl Host for ModuleCapture {
            fn write_line(&mut self, text: &str) {
                self.0.write_line(text);
            }
            fn clock_millis(&mut self) -> f64 {
                0.0
            }
            fn resolve_dynamic_import(
                &mut self,
                _: &str,
                specifier: &str,
            ) -> Result<Option<crate::host::ModuleSource>, String> {
                assert_eq!(specifier, "./integrity-dependency.mjs");
                Ok(Some(crate::host::ModuleSource {
                    name: "integrity-dependency.mjs".into(),
                    source: DEPENDENCY.into(),
                    bytes: vec![],
                }))
            }
        }
        for compile in [
            Engine::specialize_module as fn(&str, &str) -> _,
            Engine::specialize_module_unspecialized,
        ] {
            let view = Capture::default();
            let mut runtime = Runtime::new(ModuleCapture(view.clone()));
            let program = compile(SOURCE, "integrity-namespace.mjs").unwrap();
            runtime.execute(&program).unwrap();
            assert_eq!(
                view.0.borrow().as_slice(),
                &[
                    "true", "true", "false", "true", "42", "43", "43", "43", "44", "44", "true",
                    "false", "43", "true", "false"
                ]
            );
        }
    }

    #[test]
    fn regression_legacy_accessor_records_preserve_order_and_roots() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            for (var method of [Object.prototype.__defineGetter__, Object.prototype.__defineSetter__]) {
                try {method.call({}, {toString() {reads++; return 'entry';}}, undefined); print(false);}
                catch (error) {print(error instanceof TypeError);}
                print(method.call(1, {toString() {$262.gc(); return 'entry';}}, function() {}) === undefined);
            }
            print(reads);
            var getter = Object.prototype.__defineGetter__, setter = Object.prototype.__defineSetter__;
            var target = {}, value, keys = [];
            var proxy = new Proxy(target, {defineProperty(object,key,descriptor) {
                keys.push(Object.keys(descriptor).join(',')); $262.gc();
                Object.setPrototypeOf(descriptor,null); return Reflect.defineProperty(object,key,descriptor);
            }});
            Object.defineProperty(Object.prototype, 'value', {configurable:true, get() {$262.gc(); throw 'inherited value';}});
            try {
                getter.call(proxy,'entry',function() {return 42;});
                setter.call(proxy,'entry',function(next) {value = next;});
                print(proxy.entry); proxy.entry = 43; print(value);
            } finally {delete Object.prototype.value;}
            print(keys.join(';'));
            "#,
            &[
                "true",
                "true",
                "true",
                "true",
                "0",
                "42",
                "43",
                "get,enumerable,configurable;set,enumerable,configurable",
            ],
        );
    }

    #[test]
    fn regression_internal_data_records_bypass_inherited_descriptor_fields() {
        assert_output_in_execution_modes(
            r#"
            var constructorSetter = Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor').set;
            var tagSetter = Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag).set;
            var symbol = Symbol('field'), ordinary, derived, iterator = {}, binding;
            Object.defineProperty(Object.prototype,'get',{configurable:true,get() {$262.gc(); throw 'inherited get';}});
            try {
                class Ordinary {[symbol] = {rank:42}; plain = 43;}
                ordinary = new Ordinary();
                class Base {constructor() {return new Proxy({}, {defineProperty(object,key,descriptor) {
                    $262.gc(); object[key] = descriptor.value; return true;
                }});}}
                class Derived extends Base {entry = {rank:44};}
                derived = new Derived();
                constructorSetter.call(iterator,{rank:45}); tagSetter.call(iterator,'custom');
                (0,eval)('var internalRecordBinding = 46;'); binding = internalRecordBinding;
            } finally {delete Object.prototype.get; delete globalThis.internalRecordBinding;}
            print(ordinary[symbol].rank); print(ordinary.plain); print(derived.entry.rank);
            print(iterator.constructor.rank); print(iterator[Symbol.toStringTag]); print(binding);
            var descriptor = Object.getOwnPropertyDescriptor(iterator,'constructor');
            print(descriptor.writable); print(descriptor.enumerable); print(descriptor.configurable);
            "#,
            &[
                "42", "43", "44", "45", "custom", "46", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_with_unscopables_retains_block_lexical_fallbacks() {
        assert_output_in_execution_modes(
            r#"
                var object = {value: 90, [Symbol.unscopables]: {value: true}};
                {let value = 11; with (object) {
                    print(value); value = 12; print(value); print(typeof value); print(delete value);
                } print(value);}
                {let value = 21; with (object) {print(value);}}
                {let value = 31; with (object) {
                    {let value = 32; print(value);}
                    print(value);
                }}
                {const value = 41; with (object) {
                    print(value); try {value = 42;} catch (error) {print(error instanceof TypeError);}
                }}
                object[Symbol.unscopables].value = false;
                {const value = 51; with (object) {value = 52; print(value); print(delete value);} print(value);}
                {with (object) {try {print(later);} catch (error) {print(error instanceof ReferenceError);}}
                    let later = 61;}
                object.value = 71; object[Symbol.unscopables].value = true;
                {let value = 72; with (object) {print(eval('value')); eval('value = 73'); print(value);} print(value);}
                {let value = 81; with (object) {
                    function read() {return value;} print(read()); $262.gc(); print(read());
                }}
                {let fill = 33; with (Array.prototype) {print(fill);}}
                var collecting = {callable() {return 0;},
                    get [Symbol.unscopables]() {$262.gc(); return {callable: true};}};
                {let callable = function () {'use strict'; print(this === undefined); return {rank: 91};};
                    with (collecting) {print(callable().rank);}}
            "#,
            &[
                "11", "12", "number", "false", "12", "21", "32", "31", "41", "true", "52", "true",
                "51", "true", "72", "73", "73", "81", "81", "33", "true", "91",
            ],
        );
    }

    #[test]
    fn regression_array_length_remains_own_after_prevent_extensions() {
        assert_output_in_execution_modes(
            r#"
                function setLength(array, value) {'use strict'; array.length = value;}
                for (var initial of [0, 3]) {
                    var array = Object.preventExtensions(Array(initial));
                    setLength(array, initial); print(array.length);
                    setLength(array, 0); print(array.length);
                    setLength(array, 5); print(array.length);
                    print(Object.hasOwn(array, 'length'));
                    print(Object.getOwnPropertyDescriptor(array, 'length').configurable);
                }
                var array = Object.freeze([]), conversions = 0;
                try {setLength(array, 0);} catch (error) {print(error instanceof TypeError);}
                try {setLength(array, {valueOf() {conversions++; return 0;}});}
                catch (error) {print(error instanceof TypeError);}
                print(conversions); print(Reflect.set(array, 'length', 0));
                print(Reflect.defineProperty(array, 'length', {value: 0}));
                var prototype = Object.freeze([]), child = [];
                Object.setPrototypeOf(child, prototype);
                setLength(child, 2); print(child.length); print(Reflect.set(child, 'length', 3));
                print(child.length);
                var sealed = Object.seal([1, 2, 3]);
                setLength(sealed, 5); print(sealed.length);
                try {setLength(sealed, 0);} catch (error) {print(error instanceof TypeError);}
                print(sealed.length); print(sealed[2]);
                function args() {var value = arguments; Object.preventExtensions(value); setLength(value, 0); print(value.length);}
                args(1, 2);
            "#,
            &[
                "0", "0", "5", "true", "false", "3", "0", "5", "true", "false", "true", "true",
                "0", "false", "true", "2", "true", "3", "5", "true", "3", "3", "0",
            ],
        );
    }

    #[test]
    fn regression_array_species_receives_large_lengths_before_array_limits() {
        assert_output_in_execution_modes(
            r#"
                var sentinel = {}, events = [];
                function source(length, species) {
                    return new Proxy([], {
                        get(target, key) {
                            if (key === 'length') {events.push('length'); return length;}
                            if (key === 'constructor') {
                                events.push('constructor');
                                return {get [Symbol.species]() {events.push('species'); return species;}};
                            }
                            return target[key];
                        },
                        has() {throw sentinel;}
                    });
                }
                function Stop(length) {events.push('construct:' + length); throw sentinel;}
                for (var method of ['map', 'slice']) {
                    events = [];
                    var input = source({valueOf() {events.push('convert'); return Infinity;}}, Stop);
                    try {Array.prototype[method].call(input, method === 'map' ? x => x : undefined);}
                    catch (error) {print(error === sentinel);}
                    print(events.join(','));
                    events = [];
                    try {Array.prototype[method].call(source(2 ** 32, Stop), method === 'map' ? x => x : undefined);}
                    catch (error) {print(error === sentinel);}
                    print(events.join(','));
                    function ObjectResult(length) {print(length); return {};}
                    try {Array.prototype[method].call(source(Infinity, ObjectResult), method === 'map' ? x => x : undefined);}
                    catch (error) {print(error === sentinel);}
                }
                events = [];
                try {Array.prototype.map.call(source(Infinity, Stop), null);}
                catch (error) {print(error instanceof TypeError);}
                print(events.join(','));
                for (var method of ['map', 'slice']) {
                    try {Array.prototype[method].call({length: Infinity}, method === 'map' ? x => x : undefined);}
                    catch (error) {print(error instanceof RangeError);}
                }
                events = [];
                try {Array.prototype.slice.call(source(Infinity, Stop), Number.MAX_SAFE_INTEGER - 3);}
                catch (error) {print(error === sentinel);}
                print(events.join(','));
            "#,
            &[
                "true",
                "length,convert,constructor,species,construct:9007199254740991",
                "true",
                "length,constructor,species,construct:4294967296",
                "9007199254740991",
                "true",
                "true",
                "length,convert,constructor,species,construct:9007199254740991",
                "true",
                "length,constructor,species,construct:4294967296",
                "9007199254740991",
                "true",
                "true",
                "length",
                "true",
                "true",
                "true",
                "length,constructor,species,construct:3",
            ],
        );
    }

    #[test]
    fn regression_array_from_array_like_length_conversion_and_final_set() {
        assert_output_in_execution_modes(
            r#"
                var events = [];
                var source = {
                    get [Symbol.iterator]() {events.push('iterator'); return undefined;},
                    get length() {events.push('length'); return {valueOf() {events.push('convert'); return 2.9;}};},
                    get 0() {events.push('get:0'); return 4;},
                    get 1() {events.push('get:1'); return 5;}
                };
                function Target(length) {
                    events.push('construct:' + arguments.length + ':' + length);
                    return new Proxy({}, {
                        defineProperty(target, key, descriptor) {
                            events.push('define:' + key + ':' + descriptor.value);
                            return Reflect.defineProperty(target, key, descriptor);
                        },
                        set(target, key, value) {
                            events.push('set:' + key + ':' + value);
                            target[key] = value; return true;
                        }
                    });
                }
                var result = Array.from.call(Target, source, (value, index) => {
                    events.push('map:' + index); return value * 2;
                });
                print(events.join(',')); print(result.length); print(result[0] + result[1]);
                events = []; Array.from.call(Target, {length: -Infinity}); print(events.join(','));
                var sentinel = {};
                function Stop(length) {print(length); throw sentinel;}
                for (var length of [Infinity, Number.MAX_SAFE_INTEGER + 1]) {
                    try {Array.from.call(Stop, {length});} catch (error) {print(error === sentinel);}
                }
                try {Array.from({length: Infinity});} catch (error) {print(error instanceof RangeError);}
                function Reject() {return new Proxy({}, {set() {return false;}});}
                try {Array.from.call(Reject, {length: 0});} catch (error) {print(error instanceof TypeError);}
                function Throw() {return new Proxy({}, {set() {throw sentinel;}});}
                try {Array.from.call(Throw, {length: 1, 0: 9});} catch (error) {print(error === sentinel);}
                function Fixed() {return Object.defineProperty({}, 'length', {value: 1, writable: false});}
                try {Array.from.call(Fixed, {length: 1, 0: 9});} catch (error) {print(error instanceof TypeError);}
                function Typed(length) {
                    var result = new Uint8Array(length);
                    Object.defineProperty(result, 'length', {set() {throw sentinel;}});
                    return result;
                }
                var typed = Uint8Array.from.call(Typed, {length: 1.9, 0: 260}); print(typed[0]);
            "#,
            &[
                "iterator,length,convert,construct:1:2,get:0,map:0,define:0:8,get:1,map:1,define:1:10,set:length:2",
                "2",
                "18",
                "construct:1:0,set:length:0",
                "9007199254740991",
                "true",
                "9007199254740991",
                "true",
                "true",
                "true",
                "true",
                "true",
                "4",
            ],
        );
    }

    #[test]
    fn regression_array_slice_sets_species_result_length_after_copying() {
        assert_output_in_execution_modes(
            r#"
                var events = [], retained;
                function Species(length) {
                    events.push('construct:' + length);
                    retained = new Proxy({}, {
                        defineProperty(target, key, descriptor) {
                            events.push('define:' + key + ':' + descriptor.value);
                            return Reflect.defineProperty(target, key, descriptor);
                        },
                        set(target, key, value) {
                            events.push('set:' + key + ':' + value);
                            target[key] = value;
                            return true;
                        }
                    });
                    return retained;
                }
                var source = [4, , 6];
                source.constructor = {[Symbol.species]: Species};
                var result = source.slice();
                print(events.join(','));
                print(result === retained); print(result.length); print(1 in result);
                events = []; source.slice(2, 1); print(events.join(','));
                var sentinel = {};
                source.constructor = {[Symbol.species]: function () {
                    return new Proxy({}, {set() {throw sentinel;}});
                }};
                try {source.slice();} catch (error) {print(error === sentinel);}
                source.constructor = {[Symbol.species]: function () {
                    return new Proxy({}, {set() {return false;}});
                }};
                try {source.slice();} catch (error) {print(error instanceof TypeError);}
                source.constructor = {[Symbol.species]: function () {
                    return Object.defineProperty({}, 'length', {value: 3, writable: false});
                }};
                try {source.slice();} catch (error) {print(error instanceof TypeError);}
            "#,
            &[
                "construct:3,define:0:4,define:2:6,set:length:3",
                "true",
                "3",
                "false",
                "construct:0,set:length:0",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_array_results_use_the_method_realm_intrinsics() {
        assert_output_in_execution_modes(
            r#"
                var other = $262.createRealm().global;
                var Original = Array, Foreign = other.Array;
                print(Array.isArray(Foreign.prototype));
                print(Object.getOwnPropertyDescriptor(Foreign.prototype, 'length').configurable);
                for (var name of ['with', 'toReversed', 'toSorted', 'toSpliced']) {
                    var result = name === 'with' ? Foreign.prototype[name].call([3, 1, 2], 1, 9) :
                        name === 'toSpliced' ? Foreign.prototype[name].call([3, 1, 2], 0, 1, 7) :
                        Foreign.prototype[name].call([3, 1, 2]);
                    print(Object.getPrototypeOf(result) === Foreign.prototype);
                    print(result instanceof Original); print(result.join(','));
                }
                var foreign = Foreign(1, 2, 3);
                print(Object.getPrototypeOf(Original.prototype.map.call(foreign, x => x)) === Original.prototype);
                print(Object.getPrototypeOf(Foreign.prototype.map.call([1, 2], x => x)) === Foreign.prototype);
                foreign.constructor = undefined;
                print(Object.getPrototypeOf(Foreign.prototype.slice.call(foreign)) === Foreign.prototype);
                var species = {}; species[Symbol.species] = Foreign;
                var input = [1, 2]; input.constructor = species;
                print(Object.getPrototypeOf(input.map(x => x)) === Foreign.prototype);
                other.eval('function arrays(...args) { return [[1, 2], args]; }');
                var views = other.arrays(3);
                print(Object.getPrototypeOf(views) === Foreign.prototype);
                print(Object.getPrototypeOf(views[0]) === Foreign.prototype);
                print(Object.getPrototypeOf(views[1]) === Foreign.prototype);
                other.Array = {get prototype() {throw new Error('mutable global Array consulted');}};
                print(Object.getPrototypeOf(Foreign.prototype.with.call([1], 0, 2)) === Foreign.prototype);
                print(Object.getPrototypeOf(Foreign.prototype.slice.call({0: 1, length: 1})) === Foreign.prototype);
                $262.gc(); print(foreign.length);
            "#,
            &[
                "true", "false", "true", "false", "3,9,2", "true", "false", "2,1,3", "true",
                "false", "1,2,3", "true", "false", "7,1,2", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "3",
            ],
        );
    }

    #[test]
    fn regression_json_intrinsics_and_results_belong_to_their_realm() {
        assert_output_in_execution_modes(
            r#"
                var other = $262.createRealm().global;
                print(other.JSON !== JSON);
                for (var name of ['parse', 'stringify', 'rawJSON', 'isRawJSON']) {
                    print(other.JSON[name] !== JSON[name]);
                    print(Object.getPrototypeOf(other.JSON[name]) === other.Function.prototype);
                    var descriptor = Object.getOwnPropertyDescriptor(other.JSON, name);
                    print(descriptor.writable && !descriptor.enumerable && descriptor.configurable);
                }
                print(Object.getPrototypeOf(other.JSON) === other.Object.prototype);
                print(Object.getPrototypeOf(other.JSON.parse('{}')) === other.Object.prototype);
                other.JSON.parse('1', function (key, value, context) {
                    print(Object.getPrototypeOf(context) === other.Object.prototype);
                    print(Object.getPrototypeOf(this) === other.Object.prototype);
                    return value;
                });
                try { other.JSON.rawJSON(Symbol('x')); }
                catch (error) { print(error instanceof other.TypeError); print(error instanceof TypeError); }
                try { other.JSON.rawJSON(undefined); }
                catch (error) { print(error instanceof other.SyntaxError); print(error instanceof SyntaxError); }
                try { other.JSON.parse('invalid'); }
                catch (error) { print(error instanceof other.SyntaxError); }
                try { other.JSON.stringify(1n); }
                catch (error) { print(error instanceof other.TypeError); }
                try { other.JSON.rawJSON({toString() { throw new SyntaxError('guest'); }}); }
                catch (error) { print(error instanceof SyntaxError); print(error instanceof other.SyntaxError); }
                JSON.parse = 0; other.JSON.rawJSON = 0;
                var fresh = $262.createRealm().global;
                print(typeof fresh.JSON.parse); print(typeof fresh.JSON.rawJSON);
                print(fresh.JSON !== JSON && fresh.JSON !== other.JSON);
                $262.gc(); print(fresh.JSON.stringify([1]));
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "false", "true",
                "false", "true", "true", "true", "false", "function", "function", "true", "[1]",
            ],
        );
    }

    #[test]
    fn regression_json_numbers_share_javascript_number_representation() {
        assert_output_in_execution_modes(
            r#"
                print(JSON.stringify([1e20, 2**63, 123456789012345680000]));
                print(JSON.stringify([1e-6, 1e-7, 1e21, 1e300, 1.5e-7, 0.1]));
                print(JSON.stringify([-0, 0, NaN, Infinity, -Infinity]));
                print(JSON.parse('1e400')); print(JSON.parse('-1e400'));
                print(Object.is(JSON.parse('-0'), -0));
                print(Object.is(JSON.parse('-1e-400'), -0));
                JSON.parse('1e400', function (key, value, context) { print(context.source); return value; });
                print(JSON.stringify({n: new Number(2**63)}));
                print(JSON.stringify([0], function (key, value) { return key === '0' ? 1e20 : value; }));
                print(JSON.stringify(JSON.rawJSON('1e400')));
                JSON.parse('{"first":0,"target":-0}', function (key, value, context) {
                    if (key === 'first') this.target = 0;
                    if (key === 'target') print(context.source);
                    return value;
                });
            "#,
            &[
                "[100000000000000000000,9223372036854776000,123456789012345680000]",
                "[0.000001,1e-7,1e+21,1e+300,1.5e-7,0.1]",
                "[0,0,null,null,null]",
                "Infinity",
                "-Infinity",
                "true",
                "true",
                "1e400",
                "{\"n\":9223372036854776000}",
                "[100000000000000000000]",
                "1e400",
                "undefined",
            ],
        );
    }

    #[test]
    fn regression_raw_json_brand_is_not_a_guest_property() {
        assert_output_in_execution_modes(
            r#"
            var forged = {rawJSON:'not-json', ['\0quench:raw-json']:true};
            print(JSON.isRawJSON(forged)); print(JSON.stringify(forged).startsWith('{'));
            var raw = JSON.rawJSON('42'); $262.gc();
            print(JSON.isRawJSON(raw)); print(JSON.isRawJSON(Object.create(raw)));
            print(JSON.isRawJSON(new Proxy(raw,{})));
            print(JSON.stringify(new Proxy(raw,{}))); print(JSON.stringify(raw));
            "#,
            &[
                "false",
                "true",
                "true",
                "false",
                "false",
                "{\"rawJSON\":\"42\"}",
                "42",
            ],
        );
    }

    #[test]
    fn regression_json_internal_definitions_ignore_inherited_descriptor_fields() {
        assert_output_in_execution_modes(
            r#"
            var parsed, raw;
            Object.defineProperty(Object.prototype, 'get', {configurable:true, get() {$262.gc(); throw 'inherited descriptor';}});
            try {
                parsed = JSON.parse('{"entry":1}', function(key,value) {$262.gc(); return key === 'entry' ? {rank:42} : value;});
                raw = JSON.rawJSON('43');
            } finally {delete Object.prototype.get;}
            print(parsed.entry.rank); print(raw.rawJSON);
            var descriptor = Object.getOwnPropertyDescriptor(raw,'rawJSON');
            print(descriptor.writable); print(descriptor.enumerable); print(descriptor.configurable);
            print(Object.isFrozen(raw)); print(Object.getPrototypeOf(raw) === null);
            print(Object.keys(raw).join(',')); print(JSON.stringify(raw));
            "#,
            &[
                "42", "43", "false", "true", "false", "true", "true", "rawJSON", "43",
            ],
        );
    }

    #[test]
    fn regression_json_reviver_ignores_rejection_and_preserves_throw() {
        assert_output_in_execution_modes(
            r#"
            var value = JSON.parse('{"first":1,"second":2}', function(key,value) {
                if (key === 'first') {delete this.second; Object.preventExtensions(this);}
                $262.gc(); return key === 'second' ? {rank:42} : value;
            });
            print(value.first); print(Object.hasOwn(value,'second'));
            var calls = 0;
            value = JSON.parse('{"first":0,"target":{}}', function(key,value) {
                if (key === 'first') this.target = new Proxy({entry:1}, {defineProperty(object,key,descriptor) {
                    $262.gc(); calls++; print(descriptor.value.rank); return false;
                }});
                return key === 'entry' ? {rank:43} : value;
            });
            print(calls); print(value.target.entry);
            var marker = {rank:44};
            try {JSON.parse('{"first":0,"target":{}}', function(key,value) {
                if (key === 'first') this.target = new Proxy({entry:1}, {defineProperty() {$262.gc(); throw marker;}});
                return value;
            });}
            catch (error) {print(error === marker);}
            "#,
            &["1", "false", "43", "1", "1", "true"],
        );
    }

    #[test]
    fn regression_define_properties_collects_before_applying() {
        assert_output_in_execution_modes(
            r#"
            var target = {}, calls = 0;
            var proxy = new Proxy(target, {defineProperty() {calls++; return true;}});
            try {Object.defineProperties(proxy, {first:{value:1}, second:{get:1}});}
            catch (error) {print(error instanceof TypeError);}
            print(calls); print(Object.keys(target).length);
            var events = [];
            var input = {
                get first() {events.push('get-first'); return {get value() {events.push('value-first'); $262.gc(); return {rank:42};}, configurable:true};},
                get second() {events.push('get-second'); $262.gc(); return {get value() {events.push('value-second'); return 43;}, configurable:true};}
            };
            target = {};
            proxy = new Proxy(target, {defineProperty(object,key,descriptor) {
                events.push('define-' + key); $262.gc(); return Reflect.defineProperty(object,key,descriptor);
            }});
            print(Object.defineProperties(proxy,input) === proxy);
            print(events.join(',')); print(target.first.rank); print(target.second);
            "#,
            &[
                "true",
                "0",
                "0",
                "true",
                "get-first,value-first,get-second,value-second,define-first,define-second",
                "42",
                "43",
            ],
        );
    }

    #[test]
    fn regression_string_replace_preserves_utf16_and_shares_substitutions() {
        assert_output_in_execution_modes(
            r#"
            function units(value){var result=[];for(var i=0;i<value.length;i++)result.push(value.charCodeAt(i));return result.join(',');}
            for(var name of ['replace','replaceAll']){
                var input=String.fromCharCode(55296,97,55297,97),search=String.fromCharCode(55296),replacement=String.fromCharCode(56320);
                print(units(String.prototype[name].call(input,search,replacement)));
                print(units(String.prototype[name].call(String.fromCharCode(55296,97),String.fromCharCode(65533),'X')));
                var input=String.fromCharCode(55296,97,55297),template=String.fromCharCode(56320)+'$$$&$`'+"$'"+'$1$<x>';
                print(units(String.prototype[name].call(input,'a',template)));
                print(String.prototype[name].call('ab','',"[$`][$&][$']$$$1"));
                var log=[],proxy=new Proxy(function(){},{apply(target,thisArg,args){$262.gc();log.push(args[0].charCodeAt(0)+':'+args[1]+':'+args[2].charCodeAt(0)+':'+(thisArg===undefined));return {[Symbol.toPrimitive](hint){$262.gc();log.push(hint);return String.fromCharCode(57343);}};}});
                var input=String.fromCodePoint(128512)+'x'+String.fromCodePoint(128512);
                print(units(String.prototype[name].call(input,String.fromCharCode(55357),proxy)));print(log.join(','));
            }
        "#,
            &[
                "56320,97,55297,97",
                "55296,97",
                "55296,56320,36,97,55296,55297,36,49,36,60,120,62,55297",
                "[][][ab]$$1ab",
                "57343,56832,120,55357,56832",
                "55357:0:55357:true,string",
                "56320,97,55297,97",
                "55296,97",
                "55296,56320,36,97,55296,55297,36,49,36,60,120,62,55297",
                "[][][ab]$$1a[a][][b]$$1b[ab][][]$$1",
                "57343,56832,120,57343,56832",
                "55357:0:55357:true,string,55357:3:55357:true,string",
            ],
        );
    }

    #[test]
    fn regression_iterator_close_throw_bypasses_exited_inner_catch() {
        assert_output_in_execution_modes(
            r#"
            for (var abrupt of ['return', 'break']) {
                var calls=0, caught=0, finalized=0, steps=0;
                var iterable={[Symbol.iterator](){return {
                    next(){if(++steps>3)throw 'continued after close';return {done:false};},
                    return(){calls++;throw 42;}
                };}};
                var body=abrupt==='return'
                    ? 'for(var x of iterable){try{return;}catch(e){caught++;}finally{finalized++;}}'
                    : 'for(var x of iterable){try{break;}catch(e){caught++;}finally{finalized++;}}';
                try { Function('iterable', body)(iterable); print(false); }
                catch(e) { print(e===42); }
                print(calls);print(caught);print(finalized);
            }
            "#,
            &["true", "1", "0", "1", "true", "1", "0", "1"],
        );
    }

    #[test]
    fn regression_number_format_legacy_views_obey_brand_and_method_realm() {
        assert_output_in_execution_modes(
            r#"
            var wrapper=Object.create(Intl.NumberFormat.prototype);Intl.NumberFormat.call(wrapper);
            var symbol=Object.getOwnPropertySymbols(wrapper)[0];
            var formatter=new Intl.NumberFormat('en',{style:'percent'});
            var bound=formatter.format;
            print(Intl.NumberFormat.call(formatter,'fr')===formatter && formatter.format===bound && formatter.format(1)==='100%');
            formatter=new Intl.NumberFormat('en',{style:'percent'});
            Object.defineProperty(formatter,symbol,{get(){throw 'read actual formatter fallback';}});
            print(formatter.format(1)==='100%' && formatter.resolvedOptions().style==='percent');
            var reads=0;
            var fresh=Object.create(Intl.NumberFormat.prototype);
            Object.defineProperty(fresh,symbol,{get(){reads++;$262.gc();return new Intl.NumberFormat('en',{style:'percent'});}});
            var getter=Object.getOwnPropertyDescriptor(Intl.NumberFormat.prototype,'format').get;
            print(getter.call(fresh)(1)==='100%' && reads===1);
            print(Intl.NumberFormat.prototype.resolvedOptions.call(fresh).style==='percent' && reads===2);
            for(var name of ['formatToParts','formatRange','formatRangeToParts']){
                var touched=false;
                try{Intl.NumberFormat.prototype[name].call(fresh,{valueOf(){touched=true;return 1;}},2);print(false);}catch(e){print(e instanceof TypeError && !touched && reads===2);}
            }
            var plain={};Object.defineProperty(plain,symbol,{get(){throw 'read non-instance fallback';}});
            try{getter.call(plain);print(false);}catch(e){print(e instanceof TypeError);}
            var foreign=$262.createRealm().global;
            var foreignWrapper=Object.create(foreign.Intl.NumberFormat.prototype);foreign.Intl.NumberFormat.call(foreignWrapper);
            print(Intl.NumberFormat.call(foreignWrapper)!==foreignWrapper);
            try{getter.call(foreignWrapper);print(false);}catch(e){print(e instanceof TypeError);}
            var other=new foreign.Intl.NumberFormat('en');
            print(getter.call(other)(1)==='1');
            print(Object.getPrototypeOf(foreign.Intl.NumberFormat.prototype.resolvedOptions.call(formatter))===foreign.Object.prototype);
            for(var name of ['formatToParts','formatRangeToParts']){
                var parts=foreign.Intl.NumberFormat.prototype[name].call(formatter,1,2);
                print(Object.getPrototypeOf(parts)===foreign.Array.prototype && parts.every(part=>Object.getPrototypeOf(part)===foreign.Object.prototype));
            }
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_number_format_coerces_mathematical_inputs_once() {
        assert_output_in_execution_modes(
            r#"
            var formatter=new Intl.NumberFormat('en');
            for(var method of ['formatRange','formatRangeToParts']){
                var trace=[];var startCalls=0,endCalls=0;
                var start={[Symbol.toPrimitive](hint){trace.push('start:'+hint);$262.gc();if(startCalls++)throw 'coerced start twice';return '9007199254740993';}};
                var end={[Symbol.toPrimitive](hint){trace.push('end:'+hint);$262.gc();if(endCalls++)throw 'coerced end twice';return '9007199254740994';}};
                var result=formatter[method](start,end);
                print(trace.join(',')==='start:number,end:number');
                print(method==='formatRange'?result==='9,007,199,254,740,993–9,007,199,254,740,994':result.filter(part=>part.source==='startRange'&&part.type==='integer').map(part=>part.value).join('')==='9007199254740993' && result.filter(part=>part.source==='endRange'&&part.type==='integer').map(part=>part.value).join('')==='9007199254740994');
                var touched=false;
                try{formatter[method](Symbol(),{valueOf(){touched=true;return 2;}});print(false);}catch(e){print(e instanceof TypeError && !touched);}
                var thrown={};
                try{formatter[method](NaN,{valueOf(){$262.gc();throw thrown;}});print(false);}catch(e){print(e===thrown);}
            }
            print(formatter.format({[Symbol.toPrimitive](hint){$262.gc();return '9007199254740993';}})==='9,007,199,254,740,993');
            print(formatter.format({valueOf(){$262.gc();return 9007199254740993n;}})==='9,007,199,254,740,993');
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_segmenter_derives_fresh_records_from_owned_input() {
        assert_output_in_execution_modes(
            r#"
            for(var granularity of ['grapheme','word','sentence']){
                var source=granularity==='grapheme'?'AB':granularity==='word'?'hi there':'Hi. Bye.';
                var segments=new Intl.Segmenter('en',{granularity}).segment(source);
                var first=segments.containing(0);var again=segments.containing(0);
                print(first!==again && first.segment===again.segment && first.input===source);
                first.segment='changed';first.index=999;first.input='changed';first.isWordLike=false;
                print(segments.containing(0).segment===again.segment && segments.containing(0).index===0);
                var left=segments[Symbol.iterator](),right=segments[Symbol.iterator]();
                var a=left.next().value,b=right.next().value;
                print(a!==b && a!==again && a.segment===again.segment);
                a.segment='changed';a.index=999;
                print(segments.containing(0).segment===again.segment && right.next().value.index>0);
                var record=segments.containing({[Symbol.toPrimitive](hint){$262.gc();print(hint==='number');return 0;}});
                print(record.segment===again.segment);
                print(Object.keys(record).join(',')===(granularity==='word'?'segment,index,input,isWordLike':'segment,index,input'));
                print(Object.keys(record).every(key=>{var d=Object.getOwnPropertyDescriptor(record,key);return d.writable&&d.enumerable&&d.configurable;}));
                print(segments.containing(NaN).index===0 && segments.containing(-0.9).index===0 && segments.containing(Infinity)===undefined && segments.containing(-Infinity)===undefined && segments.containing(source.length)===undefined);
            }
            var segments=new Intl.Segmenter('en').segment({[Symbol.toPrimitive](hint){$262.gc();print(hint==='string');return '\ud800😀\udfff';}});
            var list=Array.from(segments);
            print(list.length===3 && list[0].segment.charCodeAt(0)===55296 && list[1].segment==='😀' && list[2].segment.charCodeAt(0)===57343);
            print(list.map(value=>value.index).join(',')==='0,1,3' && segments.containing(2).segment==='😀');
            var iterator=segments[Symbol.iterator](),prototype=Object.getPrototypeOf(iterator);
            print(Object.getPrototypeOf(prototype)===Iterator.prototype && Object.prototype.toString.call(iterator)==='[object Segmenter String Iterator]' && Object.prototype.toString.call(segments)==='[object Object]');
            var next=Object.getOwnPropertyDescriptor(prototype,'next');var tag=Object.getOwnPropertyDescriptor(prototype,Symbol.toStringTag);
            print(next.value.name==='next' && next.value.length===0 && next.writable && !next.enumerable && next.configurable && !tag.writable && !tag.enumerable && tag.configurable);
            print(iterator[Symbol.iterator]()===iterator);
            try{prototype.next.call([][Symbol.iterator]());print(false);}catch(e){print(e instanceof TypeError);}
            try{Object.getPrototypeOf([][Symbol.iterator]()).next.call(iterator);print(false);}catch(e){print(e instanceof TypeError);}
            try{segments.containing(Symbol());print(false);}catch(e){print(e instanceof TypeError);}
            try{segments.containing(1n);print(false);}catch(e){print(e instanceof TypeError);}
            var thrown={};try{segments.containing({valueOf(){$262.gc();throw thrown;}});print(false);}catch(e){print(e===thrown);}
            print(Array.from(new Intl.Segmenter().segment('')).length===0);
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_segmenter_and_iterator_results_use_method_realm() {
        assert_output_in_execution_modes(
            r#"
            var foreign=$262.createRealm().global;
            var segmenter=new Intl.Segmenter('en');
            var local=segmenter.segment('AB');
            print(Object.getPrototypeOf(foreign.Intl.Segmenter.prototype.resolvedOptions.call(segmenter))===foreign.Object.prototype);
            var source=foreign.Intl.Segmenter.prototype.segment.call(segmenter,'AB');
            var foreignSegments=new foreign.Intl.Segmenter('en').segment('AB');
            print(Object.getPrototypeOf(source)===Object.getPrototypeOf(foreignSegments));
            print(Object.getPrototypeOf(foreignSegments.containing.call(local,0))===foreign.Object.prototype);
            var foreignIterator=foreignSegments[Symbol.iterator].call(local);
            var iterator=local[Symbol.iterator]();
            print(Object.getPrototypeOf(foreignIterator)===Object.getPrototypeOf(foreignSegments[Symbol.iterator]()));
            print(Object.getPrototypeOf(Object.getPrototypeOf(foreignIterator))===foreign.Iterator.prototype);
            var next=Object.getPrototypeOf(foreignIterator).next;
            var result=next.call(iterator);
            print(Object.getPrototypeOf(result)===foreign.Object.prototype && Object.getPrototypeOf(result.value)===foreign.Object.prototype);
            var result=Object.getPrototypeOf(iterator).next.call(foreignIterator);
            print(Object.getPrototypeOf(result)===Object.prototype && Object.getPrototypeOf(result.value)===Object.prototype);
            print(Object.getPrototypeOf(foreign.Iterator.from([1]).map(value=>value).next())===foreign.Object.prototype);
            var prototype=foreign.Iterator.prototype;var object=foreign.Object.prototype;
            foreign.Iterator={};foreign.Object={};$262.gc();
            var iterator=foreignSegments[Symbol.iterator]();
            print(Object.getPrototypeOf(Object.getPrototypeOf(iterator))===prototype && Object.getPrototypeOf(iterator.next())===object);
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_array_join_owns_box_and_preserves_string_units() {
        assert_output_in_execution_modes(
            r#"
            var trace=[];
            function item(){return {toString(){trace.push('item-string');$262.gc();return '\udfff';}};}
            Object.defineProperty(Number.prototype,'length',{configurable:true,get(){trace.push('length');$262.gc();return 2;}});
            Object.defineProperty(Number.prototype,'0',{configurable:true,get(){trace.push('index:0');$262.gc();return item();}});
            Object.defineProperty(Number.prototype,'1',{configurable:true,get(){trace.push('index:1');$262.gc();return 'B';}});
            var separator={toString(){trace.push('separator');$262.gc();return '\ud800';}};
            var result=Array.prototype.join.call(7,separator);
            print(result.length===3 && result.charCodeAt(0)===57343 && result.charCodeAt(1)===55296 && result.charCodeAt(2)===66);
            print(trace.join(',')==='length,separator,index:0,item-string,index:1');
            var thrown={};
            try{Array.prototype.join.call(7,{toString(){$262.gc();throw thrown;}});print(false);}catch(e){print(e===thrown);}
            print(['\ud800',null,undefined,'\udfff'].join('\udfff')==='\ud800\udfff\udfff\udfff\udfff');
            "#,
            &["true", "true", "true", "true"],
        );
    }

    #[test]
    fn regression_list_format_uses_cached_iterator_and_exact_string_views() {
        assert_output_in_execution_modes(
            r#"
            var formatter=new Intl.ListFormat('en');
            for(var method of ['format','formatToParts']){
                var trace=[];
                function step(index){return {get done(){trace.push('done');$262.gc();return index===2;},get value(){trace.push('value');$262.gc();return index===0?'A':'B';}};}
                function next(){trace.push('next');$262.gc();Object.defineProperty(this,'next',{value:function(){throw 'reread next';},configurable:true});return step(this.index++);}
                var source={get [Symbol.iterator](){trace.push('iterator');$262.gc();return function(){trace.push('factory');$262.gc();return {index:0,get next(){trace.push('get-next');$262.gc();return next;}};};}};
                var result=formatter[method](source);
                print((method==='format'?result:result.map(part=>part.value).join(''))==='A and B');
                print(trace.join(',')==='iterator,factory,get-next,next,done,value,next,done,value,next,done');
                for(var phase of ['next','done','value','non-object','invalid']){
                    var closed=0;var thrown={};
                    var source={[Symbol.iterator](){return {next(){if(phase==='next')throw thrown;if(phase==='non-object')return 7;return {get done(){if(phase==='done')throw thrown;return false;},get value(){if(phase==='value')throw thrown;return 7;}};},return(){closed++;$262.gc();throw 'close failure';}};}};
                    try{formatter[method](source);print(false);}catch(e){print((phase==='invalid'||phase==='non-object'?e instanceof TypeError:e===thrown) && closed===(phase==='invalid'?1:0));}
                }
                for(var close of ['throw-getter','throw-call','primitive','noncallable','missing']){
                    var closed=0;
                    var source={[Symbol.iterator](){return {next(){return {done:false,value:7};},get return(){closed++;$262.gc();if(close==='throw-getter')throw {};if(close==='noncallable')return 7;if(close==='missing')return undefined;return function(){closed++;$262.gc();if(close==='throw-call')throw {};return 7;};}};}};
                    try{formatter[method](source);print(false);}catch(e){print(e instanceof TypeError && closed===(close==='throw-call'||close==='primitive'?2:1));}
                }
                var units=['\ud800','\udfff'];
                var result=formatter[method](units);
                var text=method==='format'?result:result.map(part=>part.value).join('');
                print(text.length===7 && text.charCodeAt(0)===55296 && text.charCodeAt(6)===57343);
                print((method==='format'?formatter[method](undefined)==='':formatter[method](undefined).length===0));
                var touched=false;
                try{Intl.ListFormat.prototype[method].call({}, {get [Symbol.iterator](){touched=true;}});print(false);}catch(e){print(e instanceof TypeError && !touched);}
            }
            var foreign=$262.createRealm().global;
            var parts=foreign.Intl.ListFormat.prototype.formatToParts.call(formatter,['A','B']);
            print(Object.getPrototypeOf(parts)===foreign.Array.prototype && parts.every(part=>Object.getPrototypeOf(part)===foreign.Object.prototype));
            var parts=Intl.ListFormat.prototype.formatToParts.call(new foreign.Intl.ListFormat('en'),['A','B']);
            print(Object.getPrototypeOf(parts)===Array.prototype && parts.every(part=>Object.getPrototypeOf(part)===Object.prototype));
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true",
            ],
        );
    }

    #[test]
    fn regression_intl_supported_locales_share_order_validation_and_realms() {
        assert_output_in_execution_modes(
            r#"
            var kinds=['RelativeTimeFormat','Segmenter','Collator','NumberFormat','DateTimeFormat','PluralRules','DurationFormat','ListFormat','DisplayNames'];
            var foreign=$262.createRealm().global;
            for(var kind of kinds){
                var method=Intl[kind].supportedLocalesOf;
                var descriptor=Object.getOwnPropertyDescriptor(Intl[kind],'supportedLocalesOf');
                var trace=[];
                var list=new Proxy({0:{toString(){trace.push('locale');$262.gc();return 'EN';}},1:'en',2:'fr',length:3},{
                    get(value,key){trace.push(key==='length'?'length':'index:'+key);$262.gc();return Reflect.get(value,key);},
                    has(value,key){trace.push('has:'+key);$262.gc();return Reflect.has(value,key);}
                });
                var options={get localeMatcher(){trace.push('matcher');$262.gc();return {toString(){trace.push('matcher-string');$262.gc();return 'lookup';}};}};
                var result=method.call(null,list,options);
                print(result.join(',')==='en,fr' && trace.join(',')==='length,has:0,index:0,locale,has:1,index:1,has:2,index:2,matcher,matcher-string');
                print(method.name==='supportedLocalesOf' && method.length===1 && descriptor.writable && descriptor.configurable && !descriptor.enumerable);
                var element=Object.getOwnPropertyDescriptor(result,'0');
                var length=Object.getOwnPropertyDescriptor(result,'length');
                print(element.writable && element.enumerable && element.configurable && length.writable && !length.enumerable && !length.configurable && method(['en'])!==method(['en']));
                var touched=false;
                try{method(['bad_locale'],{get localeMatcher(){touched=true;}});print(false);}catch(e){print(e instanceof RangeError && !touched);}
                try{method([7],null);print(false);}catch(e){print(e instanceof TypeError);}
                try{method([],null);print(false);}catch(e){print(e instanceof TypeError);}
                try{method([],{localeMatcher:'invalid'});print(false);}catch(e){print(e instanceof RangeError);}
                try{method([],{localeMatcher:Symbol()});print(false);}catch(e){print(e instanceof TypeError);}
                touched=false;
                Object.defineProperty(Number.prototype,'localeMatcher',{configurable:true,get(){touched=true;$262.gc();return 'best fit';}});
                print(method([],7).length===0 && touched);
                delete Number.prototype.localeMatcher;
                var thrown={};
                try{method({get length(){throw thrown;}},null);print(false);}catch(e){print(e===thrown);}
                print(Object.getPrototypeOf(foreign.Intl[kind].supportedLocalesOf(['en']))===foreign.Array.prototype);
            }
            print(Object.getPrototypeOf(foreign.Intl.getCanonicalLocales(['en']))===foreign.Array.prototype);
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_intl_locale_clones_slots_and_orders_constructor_effects() {
        assert_output_in_execution_modes(
            r#"
            var original=new Intl.Locale('en-US');
            original.toString=function(){throw 'stringified existing Locale';};
            original[Symbol.toPrimitive]=function(){throw 'coerced existing Locale';};
            print(new Intl.Locale(original).toString());
            print(Intl.getCanonicalLocales([original]).join(','));
            var trace=[];
            var tag={toString(){trace.push('tag');$262.gc();return 'en';}};
            var options={get language(){trace.push('language');$262.gc();return 'fr';}};
            var target=new Proxy(function(){},{get(value,key){if(key==='prototype'){trace.push('prototype');$262.gc();return {marker:'kept'};}return Reflect.get(value,key);}});
            var value=Reflect.construct(Intl.Locale,[tag,options],target);
            print(trace.join(','));
            print(Object.getPrototypeOf(value).marker);
            print(Intl.Locale.prototype.toString.call(value));
            var record=Proxy.revocable(function(){},{get(value,key){if(key==='prototype'){record.revoke();return {marker:'revoked'};}return Reflect.get(value,key);}});
            print(Object.getPrototypeOf(Reflect.construct(Intl.Locale,['en'],record.proxy)).marker);
            try{new Intl.Locale('bad_locale',null);print(false);}catch(e){print(e instanceof TypeError);}
            try{new Intl.Locale('bad_locale',{get language(){throw 'read invalid tag options';}});print(false);}catch(e){print(e instanceof RangeError);}
            var thrown={};
            try{Reflect.construct(Intl.Locale,[],new Proxy(function(){},{get(){throw thrown;}}));print(false);}catch(e){print(e===thrown);}
            for(var prototype of [function(){},new Proxy({},{}),[]]) {
                var target=new Proxy(function(){},{get(value,key){return key==='prototype'?prototype:Reflect.get(value,key);}});
                print(Object.getPrototypeOf(Reflect.construct(Intl.Locale,['en'],target))===prototype);
            }
            var foreign=$262.createRealm().global;
            var target=foreign.Function('');target.prototype=undefined;
            print(Object.getPrototypeOf(Reflect.construct(Intl.Locale,['en'],target))===foreign.Intl.Locale.prototype);
            var foreignLocale=new foreign.Intl.Locale('de');foreignLocale.toString=function(){throw 'foreign toString';};
            print(new Intl.Locale(foreignLocale).toString());
            "#,
            &[
                "en-US",
                "en-US",
                "prototype,tag,language",
                "kept",
                "fr",
                "revoked",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "de",
            ],
        );
    }

    #[test]
    fn regression_intl_locale_derived_results_use_method_realm_intrinsics() {
        assert_output_in_execution_modes(
            r#"
            var source=new Intl.Locale('en');
            Object.setPrototypeOf(source,{marker:'source prototype'});
            var maximum=Intl.Locale.prototype.maximize.call(source);
            var minimum=Intl.Locale.prototype.minimize.call(source);
            print(Object.getPrototypeOf(maximum)===Intl.Locale.prototype);
            print(Object.getPrototypeOf(minimum)===Intl.Locale.prototype);
            print(maximum.toString());print(minimum.toString());
            class CustomLocale extends Intl.Locale {}
            var custom=new CustomLocale('en');
            print(Object.getPrototypeOf(custom.maximize())===Intl.Locale.prototype);
            print(Object.getPrototypeOf(custom.minimize())===Intl.Locale.prototype);
            var foreign=$262.createRealm().global;
            print(Object.getPrototypeOf(foreign.Intl.Locale.prototype.maximize.call(source))===foreign.Intl.Locale.prototype);
            print(Object.getPrototypeOf(foreign.Intl.Locale.prototype.minimize.call(source))===foreign.Intl.Locale.prototype);
            print(Object.getPrototypeOf(Intl.Locale.prototype.maximize.call(new foreign.Intl.Locale('en')))===Intl.Locale.prototype);
            "#,
            &[
                "true",
                "true",
                "en-Latn-US",
                "en",
                "true",
                "true",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_intl_constructor_prototypes_precede_option_effects() {
        assert_output_in_execution_modes(
            r#"
            var names=['Collator','NumberFormat','DateTimeFormat','PluralRules','RelativeTimeFormat','ListFormat','Segmenter','DisplayNames','DurationFormat'];
            for(var kind of names) {
                var trace=[];
                var locales={get length(){trace.push('length');$262.gc();return 1;},0:{toString(){trace.push('locale');$262.gc();return 'en';}}};
                var options={get localeMatcher(){trace.push('matcher');$262.gc();return 'lookup';},type:kind==='DisplayNames'?'language':kind==='ListFormat'?'conjunction':'cardinal'};
                var target=new Proxy(function(){},{get(value,key){if(key==='prototype'){trace.push('prototype');$262.gc();return {marker:'expected'};}return Reflect.get(value,key);}});
                var result=Reflect.construct(Intl[kind],[locales,options],target);
                print(trace.join(',')==='prototype,length,locale,matcher');
                print(Object.getPrototypeOf(result).marker==='expected');
            }
            for(var kind of names) {
                var record=Proxy.revocable(function(){},{get(value,key){if(key==='prototype'){record.revoke();return {marker:'kept'};}return Reflect.get(value,key);}});
                var result=Reflect.construct(Intl[kind],['en',{type:kind==='DisplayNames'?'language':kind==='ListFormat'?'conjunction':'cardinal'}],record.proxy);
                print(Object.getPrototypeOf(result).marker==='kept');
            }
            var foreign=$262.createRealm().global;
            for(var kind of names) {
                var target=foreign.Function('');target.prototype=undefined;
                var result=Reflect.construct(Intl[kind],['en',{type:kind==='DisplayNames'?'language':kind==='ListFormat'?'conjunction':'cardinal'}],target);
                print(Object.getPrototypeOf(result)===foreign.Intl[kind].prototype);
            }
            var first=new Intl.DurationFormat('en'),second=new Intl.DurationFormat('en');
            print(first.format===second.format);
            print(Object.prototype.hasOwnProperty.call(first,'format'));
            print(Object.getOwnPropertyDescriptor(Intl.DurationFormat.prototype,'format').value===first.format);
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "false", "true",
            ],
        );
    }

    #[test]
    fn regression_intl_get_options_object_distinguishes_coercing_apis() {
        assert_output_in_execution_modes(
            r#"
            var reads=0;
            Object.defineProperty(Number.prototype,'localeMatcher',{configurable:true,get(){reads++;throw 'primitive options were read';}});
            for(var kind of ['DisplayNames','DurationFormat','ListFormat','Segmenter']) {
                for(var value of [null,true,false,0,42,'x',Symbol('x'),1n]) {
                    try{new Intl[kind]('en',value);print(false);}catch(e){print(e instanceof TypeError);}
                }
            }
            print(reads);
            delete Number.prototype.localeMatcher;
            for(var kind of ['NumberFormat','PluralRules','RelativeTimeFormat','DateTimeFormat']) {
                print(new Intl[kind]('en',7).resolvedOptions().locale==='en');
            }
            for(var kind of ['DisplayNames','DurationFormat','ListFormat','Segmenter']) {
                var options=new Number(7);options.type=kind==='DisplayNames'?'language':'conjunction';options.style='short';options.granularity='word';
                print(new Intl[kind]('en',options).resolvedOptions().locale==='en');
            }
            print(new Intl.NumberFormat(new Intl.Locale('en')).resolvedOptions().locale);
            print(new Intl.NumberFormat([,'EN','en']).resolvedOptions().locale);
            "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "0", "true", "true", "true", "true", "true", "true", "true",
                "true", "en", "en",
            ],
        );
    }

    #[test]
    fn regression_collator_uses_one_canonical_locale_list() {
        assert_output_in_execution_modes(
            r#"
            var trace=[];
            var locales=new Proxy({length:3,0:{toString(){trace.push('locale');$262.gc();return 'EN';}},2:'en'}, {
                get(target,key){trace.push('get:'+String(key));$262.gc();return target[key];},
                has(target,key){trace.push('has:'+String(key));$262.gc();return key in target;}
            });
            var options={get usage(){trace.push('usage');$262.gc();return 'sort';}};
            print(Math.sign('a'.localeCompare('b',locales,options)));
            print(trace.join(','));
            trace=[];
            print(new Intl.Collator(locales,options).resolvedOptions().locale);
            print(trace.join(','));
            print(Intl.getCanonicalLocales([,new Intl.Locale('en'), 'EN']).join(','));
            print(new Intl.Collator(new Intl.Locale('en')).resolvedOptions().locale);
            try{new Intl.Collator(['en','bad_locale']);print('missed');}catch(e){print(e instanceof RangeError);}
            var once=0;
            print('a'.localeCompare('b',[{toString(){if(++once>1)throw 'twice';return 'en';}}]));
            print(once);
            "#,
            &[
                "-1",
                "get:length,has:0,get:0,locale,has:1,has:2,get:2,usage",
                "en",
                "get:length,has:0,get:0,locale,has:1,has:2,get:2,usage",
                "en",
                "en",
                "true",
                "-1",
                "1",
            ],
        );
    }

    #[test]
    fn regression_string_raw_preserves_utf16_and_conversion_order() {
        assert_output_in_execution_modes(
            r#"
            function units(text){var result=[];for(var i=0;i<text.length;i++)result.push(text.charCodeAt(i));return result.join(',');}
            var log=[];function text(name,value){return {[Symbol.toPrimitive](hint){$262.gc();log.push(name+':'+hint);return value;}};}
            var template={get raw(){$262.gc();log.push('raw');return {get length(){$262.gc();log.push('length');return {[Symbol.toPrimitive](hint){$262.gc();log.push('length:'+hint);return 3.9;}};},get 0(){$262.gc();log.push('0');return text('0','\ud800');},get 1(){$262.gc();log.push('1');return text('1','a');},get 2(){$262.gc();log.push('2');return text('2','\udc00');}};}};
            print(units(String.raw(template,text('sub0','\ud801'),text('sub1','\udc01'))));print(log.join(','));
            var calls=0,extra={[Symbol.toPrimitive](){calls++;throw 'unused';}};
            print(String.raw({raw:{length:1,0:'a'}},extra));print(calls);
            print(String.raw({raw:{length:-1,get 0(){throw 'segment';}}},extra));print(calls);
            print(String.raw({raw:['a','b','c']}));print(String.raw({raw:['a','b']},undefined));
            print(String.raw({raw:'abc'},text('boxed0','X'),text('boxed1','Y')));
            for(var length of [1n,Symbol()]){try{String.raw({raw:{length:length,get 0(){throw 'segment';}}},extra);}catch(error){print(error instanceof TypeError);}}
        "#,
            &[
                "55296,55297,97,56321,56320",
                "raw,length,length:number,0,0:string,sub0:string,1,1:string,sub1:string,2,2:string",
                "a",
                "0",
                "",
                "0",
                "abc",
                "aundefinedb",
                "aXbYc",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_shared_string_coercion_preserves_utf16_object_results() {
        assert_output_in_execution_modes(
            r#"
            function units(text){var result=[];for(var i=0;i<text.length;i++)result.push(text.charCodeAt(i));return result.join(',');}
            var text={[Symbol.toPrimitive](hint){$262.gc();if(hint!=='string')throw 'hint';return '\ud800';}};
            print(units(''.concat(text)));print(('\ud800a').indexOf(text));
            print(units('x'.anchor(text)));
            var json={[Symbol.toPrimitive](hint){$262.gc();return '"\ud800"';}};print(units(JSON.parse(json)));
        "#,
            &[
                "55296",
                "0",
                "60,97,32,110,97,109,101,61,34,55296,34,62,120,60,47,97,62",
                "55296",
            ],
        );
    }

    #[test]
    fn regression_native_string_receiver_conversion_preserves_utf16_and_order() {
        assert_output_in_execution_modes(
            r#"
            function units(text){var result=[];for(var i=0;i<text.length;i++)result.push(text.charCodeAt(i));return result.join(',');}
            for(var name of ['concat','anchor','substring']){
                var log=[],receiver={[Symbol.toPrimitive](hint){$262.gc();log.push('input:'+hint);return '\ud800a\udc00';}};
                var first={[Symbol.toPrimitive](hint){$262.gc();log.push('first:'+hint);return name==='substring'?0:'X';}},second={[Symbol.toPrimitive](hint){$262.gc();log.push('second:'+hint);return name==='substring'?1:'Y';}};
                print(units(String.prototype[name].call(receiver,first,second)));print(log.join(','));
            }
            var calls=0,argument={[Symbol.toPrimitive](){calls++;throw 'argument';}};
            try{String.prototype.concat.call(null,argument);}catch(error){print(error instanceof TypeError);}print(calls);
            var marker={},receiver={[Symbol.toPrimitive](){throw marker;}};try{String.prototype.concat.call(receiver,argument);}catch(error){print(error===marker);}print(calls);
        "#,
            &[
                "55296,97,56320,88,89",
                "input:string,first:string,second:string",
                "60,97,32,110,97,109,101,61,34,88,34,62,55296,97,56320,60,47,97,62",
                "input:string,first:string",
                "55296",
                "input:string,first:number,second:number",
                "true",
                "0",
                "true",
                "0",
            ],
        );
    }

    #[test]
    fn regression_regexp_compile_eligibility_survives_prototype_mutation() {
        assert_output_in_execution_modes(
            r#"
            var value=/a/;Object.setPrototypeOf(value,{});$262.gc();print(RegExp.prototype.compile.call(value,'b')===value);
            var value=new(class extends RegExp {})('a');Object.setPrototypeOf(value,RegExp.prototype);$262.gc();try{value.compile('b');print(false);}catch(error){print(error instanceof TypeError);}
            var target=new Proxy(function(){},{get(object,key,receiver){if(key==='prototype')return RegExp.prototype;return Reflect.get(object,key,receiver);}});
            var value=Reflect.construct(RegExp,['a'],target);$262.gc();try{value.compile('b');print(false);}catch(error){print(error instanceof TypeError);}
        "#,
            &["true", "true", "true"],
        );
    }

    #[test]
    fn regression_regexp_entrypoints_preserve_utf16_conversion_and_compile_state() {
        assert_output_in_execution_modes(
            r#"
            function units(text){var result=[];for(var i=0;i<text.length;i++)result.push(text.charCodeAt(i));return result.join(',');}
            var log=[],receiver={get source(){log.push('source');return {[Symbol.toPrimitive](hint){$262.gc();log.push('source:'+hint);return '\ud800';}};},get flags(){log.push('flags');return {[Symbol.toPrimitive](hint){$262.gc();log.push('flags:'+hint);return '\udc00';}};}};
            print(units(RegExp.prototype.toString.call(receiver)));print(log.join(','));
            var receiver=/old/g;Object.setPrototypeOf(receiver,{});var log=[];
            var input={[Symbol.toPrimitive](hint){$262.gc();log.push('input:'+hint);return '\ud800';}},flags={[Symbol.toPrimitive](hint){$262.gc();log.push('flags:'+hint);return 'i';}};
            print(RegExp.prototype.compile.call(receiver,input,flags)===receiver);print(log.join(','));print(Object.getOwnPropertyDescriptor(RegExp.prototype,'source').get.call(receiver).charCodeAt(0));
            var receiver=/old/g;receiver.lastIndex=7;try{receiver.compile('[');}catch(error){print(error instanceof SyntaxError);}print(receiver.source);print(receiver.lastIndex);
            Object.defineProperty(receiver,'lastIndex',{writable:false});try{receiver.compile('new','i');}catch(error){print(error instanceof TypeError);}print(receiver.source);print(receiver.flags);print(receiver.lastIndex);
            var receiver=/a/g,log=[];receiver.lastIndex={[Symbol.toPrimitive](hint){$262.gc();log.push(hint);return 0;}};print(receiver.exec('a')[0]);print(log.join(','));
            var receiver={},log=[];for(var name of ['sticky','unicodeSets','unicode','dotAll','multiline','ignoreCase','global','hasIndices']){(function(name){Object.defineProperty(receiver,name,{get(){$262.gc();log.push(name);return true;}});})(name);}
            print(Object.getOwnPropertyDescriptor(RegExp.prototype,'flags').get.call(receiver));print(log.join(','));
        "#,
            &[
                "47,55296,47,56320",
                "source,source:string,flags,flags:string",
                "true",
                "input:string,flags:string",
                "55296",
                "true",
                "old",
                "7",
                "true",
                "new",
                "i",
                "7",
                "a",
                "number",
                "dgimsuvy",
                "hasIndices,global,ignoreCase,multiline,dotAll,unicode,unicodeSets,sticky",
            ],
        );
    }

    #[test]
    fn regression_regexp_construction_preserves_order_identity_and_descriptors() {
        assert_output_in_execution_modes(
            r#"
            var log=[],pattern={get [Symbol.match](){log.push('is-regexp');return true;},get constructor(){log.push('constructor');return RegExp;},get source(){throw 'source';}};
            print(RegExp(pattern)===pattern);print(log.join(','));
            var log=[],prototype={},pattern={get [Symbol.match](){log.push('is-regexp');return true;},get source(){log.push('source');return {[Symbol.toPrimitive](hint){$262.gc();log.push('source:'+hint);return '\ud800';}};},get flags(){log.push('flags');return {[Symbol.toPrimitive](hint){$262.gc();log.push('flags:'+hint);return 'i';}};}},target=new Proxy(function(){},{get(object,key,receiver){if(key==='prototype'){$262.gc();log.push('prototype');return prototype;}return Reflect.get(object,key,receiver);}});
            var result=Reflect.construct(RegExp,[pattern],target);print(Object.getPrototypeOf(result)===prototype);print(log.join(','));
            print(Object.getOwnPropertyDescriptor(RegExp.prototype,'source').get.call(result).charCodeAt(0));
            var descriptor=Object.getOwnPropertyDescriptor(result,'lastIndex');print([descriptor.value,descriptor.writable,descriptor.enumerable,descriptor.configurable].join(','));
            var pattern=/a/g;Object.defineProperty(pattern,Symbol.match,{get(){return false;}});Object.defineProperty(pattern,'source',{get(){throw 'source';}});Object.defineProperty(pattern,'flags',{get(){throw 'flags';}});
            var result=new RegExp(pattern,'i');print(result.source);print(result.flags);
            var log=[],pattern={[Symbol.match]:true,get constructor(){log.push('constructor');return null;},get source(){log.push('source');return 'a';},get flags(){log.push('flags');return 'i';}};
            var result=RegExp(pattern);print(result.source);print(log.join(','));
        "#,
            &[
                "true",
                "is-regexp,constructor",
                "true",
                "is-regexp,source,flags,prototype,source:string,flags:string",
                "55296",
                "0,true,false,false",
                "a",
                "i",
                "a",
                "constructor,source,flags",
            ],
        );
    }

    #[test]
    fn regression_regexp_replace_observes_each_result_before_next_callback() {
        assert_output_in_execution_modes(
            r#"
            var log=[],calls=0,second={0:'a',length:2,index:2,1:'old',groups:undefined};
            var first={0:'a',length:2,index:0,get 1(){log.push('capture:first');return {[Symbol.toPrimitive](hint){$262.gc();log.push('convert:'+hint);return 'C';}};},groups:undefined};
            var pattern={flags:'g',exec(){log.push('exec');return calls++===0?first:calls===2?second:null;}};
            print(RegExp.prototype[Symbol.replace].call(pattern,'a-a',function(matched,capture,index,input){$262.gc();log.push('call:'+index+':'+capture);second[1]='new';return 'X';}));print(log.join(','));
            for(var callable of [false,true]){
                var calls=0,log=[],pattern={flags:'g',exec(){if(calls++===0)return {0:'aa',length:1,index:0};if(calls===2)return {0:'a',length:1,index:1,get groups(){log.push('groups');return {get x(){log.push('named');return 'X';}};}};return null;}};
                print(RegExp.prototype[Symbol.replace].call(pattern,'aaa',callable?function(matched,index){log.push('call:'+index);return 'X';}:'$<x>'));print(log.join(','));
            }
            var log=[],pattern={flags:'',exec(){return {0:'a',length:2,index:0,1:{[Symbol.toPrimitive](hint){log.push('capture:'+hint);return 'C';}},groups:null};}};
            print(RegExp.prototype[Symbol.replace].call(pattern,'a',new Proxy(function(){},{apply(target,receiver,args){$262.gc();print(receiver===undefined&&args[1]==='C'&&args[4]===null);return 'X';}})));print(log.join(','));
            try{RegExp.prototype[Symbol.replace].call(pattern,'a','X');}catch(error){print(error instanceof TypeError);}
            var pattern={flags:'gu',lastIndex:0,exec(){if(this.lastIndex>2)return null;return {0:'',length:1,index:this.lastIndex};}};
            print(RegExp.prototype[Symbol.replace].call(pattern,'\ud83d\ude00','X'));
            var key='\ud800',groups={};groups[key]='Y';var pattern={flags:'',exec(){return {0:'a',length:1,index:0,groups:groups};}};
            print(RegExp.prototype[Symbol.replace].call(pattern,'a','$<'+key+'>'));
            Object.defineProperty(Boolean.prototype,'x',{configurable:true,get:function(){'use strict';$262.gc();print(typeof this);return 'B';}});
            var pattern={flags:'',exec(){return {0:'a',length:1,index:0,groups:true};}};
            print(RegExp.prototype[Symbol.replace].call(pattern,'a','$<x>'));delete Boolean.prototype.x;
        "#,
            &[
                "X-X",
                "exec,exec,exec,capture:first,convert:string,call:0:C,call:2:new",
                "$<x>a",
                "groups,named",
                "Xa",
                "call:0,groups,call:1",
                "true",
                "X",
                "capture:string",
                "true",
                "X😀X",
                "Y",
                "object",
                "B",
            ],
        );
    }

    #[test]
    fn regression_string_split_conversion_order_and_utf16() {
        assert_output_in_execution_modes(
            r#"
            for(var n of [0,2,4294967296,4294967298]){
                var log=[],input={[Symbol.toPrimitive](hint){log.push('input:'+hint);$262.gc();return 'a-a-a';}},limit={[Symbol.toPrimitive](hint){log.push('limit:'+hint);$262.gc();return n;}},separator={get [Symbol.split](){log.push('method');return null;},[Symbol.toPrimitive](hint){log.push('separator:'+hint);$262.gc();return '-';}};
                print(String.prototype.split.call(input,separator,limit).join('|'));print(log.join(','));
            }
            var marker={},separator={[Symbol.split]:null,[Symbol.toPrimitive](){throw marker;}};
            try{'abc'.split(separator,0);}catch(error){print(error===marker);}
            var reads=0,separator={get [Symbol.split](){reads++;return function(){};}};
            try{String.prototype.split.call(null,separator);}catch(error){print(error instanceof TypeError);}print(reads);
            var input={},limit={},separator={[Symbol.split]:new Proxy(function(){},{apply(target,receiver,args){$262.gc();print(receiver===separator&&args[0]===input&&args[1]===limit);return marker;}})};
            print(String.prototype.split.call(input,separator,limit)===marker);
            function units(parts){return parts.map(function(part){var result=[];for(var i=0;i<part.length;i++)result.push(part.charCodeAt(i));return result.join(',');}).join('|');}
            print(units(('\ud800a\ud801').split('a')));print(units(('\ud800a').split('\ufffd')));
            print(units(('\ud83d\ude00x').split('',2)));print(''.split('').length);print(''.split('x').length);print('abc'.split(undefined,0).length);
        "#,
            &[
                "",
                "method,input:string,limit:number,separator:string",
                "a|a",
                "method,input:string,limit:number,separator:string",
                "",
                "method,input:string,limit:number,separator:string",
                "a|a",
                "method,input:string,limit:number,separator:string",
                "true",
                "true",
                "0",
                "true",
                "true",
                "55296|55297",
                "55296,97",
                "55357|56832",
                "0",
                "1",
                "0",
            ],
        );
    }

    #[test]
    fn regression_string_replace_protocol_identity_and_conversion_order() {
        assert_output_in_execution_modes(
            r#"
            for(var name of ['replace','replaceAll']){
                var log=[],input={[Symbol.toPrimitive](){throw 'input';}},replacement={[Symbol.toPrimitive](){throw 'replacement';}},record={};
                var search={get [Symbol.match](){log.push('is-regexp');return true;},get flags(){log.push('flags');return 'g';},get [Symbol.replace](){log.push('method');return function(arg,value){$262.gc();print(this===search&&arg===input&&value===replacement);return record;};}};
                print(String.prototype[name].call(input,search,replacement)===record);print(log.join(','));
                var log=[],receiver={[Symbol.toPrimitive](hint){log.push('input:'+hint);return 'abc';}},search={[Symbol.match]:false,[Symbol.replace]:null,[Symbol.toPrimitive](hint){log.push('search:'+hint);return 'z';}},replacement={[Symbol.toPrimitive](hint){log.push('replacement:'+hint);return 'X';}};
                print(String.prototype[name].call(receiver,search,replacement));print(log.join(','));
                var marker={},replacement={[Symbol.toPrimitive](){throw marker;}};try{String.prototype[name].call('abc','z',replacement);}catch(error){print(error===marker);}
                var reads=0,search={get [Symbol.replace](){reads++;return function(){};}};try{String.prototype[name].call(null,search,'X');}catch(error){print(error instanceof TypeError);}print(reads);
            }
            var methods=0,search={[Symbol.match]:true,flags:'i',get [Symbol.replace](){methods++;return function(){};}};
            try{String.prototype.replaceAll.call('abc',search,'X');}catch(error){print(error instanceof TypeError);}print(methods);
        "#,
            &[
                "true",
                "true",
                "method",
                "abc",
                "input:string,search:string,replacement:string",
                "true",
                "true",
                "0",
                "true",
                "true",
                "is-regexp,flags,method",
                "abc",
                "input:string,search:string,replacement:string",
                "true",
                "true",
                "0",
                "true",
                "0",
            ],
        );
    }

    #[test]
    fn regression_string_match_all_custom_dispatch_preserves_identity_and_flags_order() {
        assert_output_in_execution_modes(
            r#"
            var log=[],input={[Symbol.toPrimitive](){throw 'coerced';}},record={};
            var pattern={get [Symbol.match](){$262.gc();log.push('is-regexp');return true;},get flags(){$262.gc();log.push('flags');return {[Symbol.toPrimitive](hint){$262.gc();log.push('flags:'+hint);return 'g';}};},get [Symbol.matchAll](){$262.gc();log.push('method');return function(arg){$262.gc();log.push('call');print(this===pattern);print(arg===input);return record;};}};
            print(String.prototype.matchAll.call(input,pattern)===record);print(log.join(','));
            for(var flags of [null,undefined,'i',Symbol()]){
                var reads=0,pattern={[Symbol.match]:true,flags:flags,get [Symbol.matchAll](){reads++;return function(){};}};
                try{String.prototype.matchAll.call(input,pattern);}catch(error){print(error instanceof TypeError);}print(reads);
            }
            var reads=0,pattern={get [Symbol.match](){reads++;return true;}};
            try{String.prototype.matchAll.call(null,pattern);}catch(error){print(error instanceof TypeError);}print(reads);
            var bad={[Symbol.match]:false,[Symbol.matchAll]:42};try{String.prototype.matchAll.call(input,bad);}catch(error){print(error instanceof TypeError);}
            var marker={},throwing={[Symbol.match]:false,[Symbol.matchAll](){throw marker;}};
            try{String.prototype.matchAll.call(input,throwing);}catch(error){print(error===marker);}
        "#,
            &[
                "true",
                "true",
                "true",
                "is-regexp,flags,flags:string,method,call",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_string_match_all_fallback_uses_intrinsic_creation_and_input_first() {
        assert_output_in_execution_modes(
            r#"
            var original=RegExp,globalDescriptor=Object.getOwnPropertyDescriptor(globalThis,'RegExp'),globalReads=0,descriptor=Object.getOwnPropertyDescriptor(original.prototype,Symbol.matchAll);
            Object.defineProperty(globalThis,'RegExp',{configurable:true,get(){globalReads++;throw 'global';}});
            for(var kind of ['object','primitive']){
                var log=[];
                Object.defineProperty(original.prototype,Symbol.matchAll,{configurable:true,get(){$262.gc();log.push('invoke:get');return function(input){$262.gc();log.push('invoke:'+input.charCodeAt(0));return Object.getPrototypeOf(this)===original.prototype&&this.source==='a'&&this.flags==='g';};}});
                var input={[Symbol.toPrimitive](hint){$262.gc();log.push('input:'+hint);return String.fromCharCode(55296,97);}};
                var pattern=kind==='primitive'?'a':{get [Symbol.match](){log.push('is-regexp');return false;},get [Symbol.matchAll](){log.push('method');return null;},[Symbol.toPrimitive](hint){$262.gc();log.push('pattern:'+hint);return 'a';}};
                print(String.prototype.matchAll.call(input,pattern));print(log.join(','));
            }
            print(globalReads);Object.defineProperty(globalThis,'RegExp',globalDescriptor);Object.defineProperty(original.prototype,Symbol.matchAll,descriptor);
            var pattern=/a/g;pattern[Symbol.matchAll]=null;var iterator=String.prototype.matchAll.call('/a/g',pattern);print(iterator.next().value[0]);
            var hooks=0;Object.defineProperty(String.prototype,Symbol.matchAll,{configurable:true,get(){hooks++;throw 'hook';}});
            var iterator=String.prototype.matchAll.call('a','a');print(iterator.next().value[0]);print(hooks);delete String.prototype[Symbol.matchAll];
        "#,
            &[
                "true",
                "is-regexp,method,input:string,pattern:string,invoke:get,invoke:55296",
                "true",
                "input:string,invoke:get,invoke:55296",
                "0",
                "/a/g",
                "a",
                "0",
            ],
        );
    }

    #[test]
    fn regression_string_match_search_pass_original_receiver_to_custom_methods() {
        assert_output_in_execution_modes(
            r#"
            var input={[Symbol.toPrimitive](){throw 'coerced';}},record={tag:42};
            for(var name of ['match','search']) {
                var sym=Symbol[name],pattern={get [sym](){$262.gc();return function(arg){$262.gc();print(this===pattern);print(arg===input);return record;};}};
                print(String.prototype[name].call(input,pattern)===record);
                var primitiveReads=0;
                for(var pair of [[Number.prototype,1],[String.prototype,'a'],[Boolean.prototype,true],[BigInt.prototype,1n],[Symbol.prototype,Symbol()]]) {
                    Object.defineProperty(pair[0],sym,{configurable:true,get(){primitiveReads++;throw 'primitive-hook';}});
                    var conversions=0,plainInput={[Symbol.toPrimitive](){conversions++;return 'a1true';}};
                    try{var value=String.prototype[name].call(plainInput,pair[1]);print(name==='match'?value!==null:value>=0);}catch(error){print(error instanceof TypeError&&typeof pair[1]==='symbol');}
                    print(conversions===1);delete pair[0][sym];
                }
                print(primitiveReads);
                var reads=0,pattern={get [sym](){reads++;return function(){return 42;};}};
                try{String.prototype[name].call(null,pattern);}catch(error){print(error instanceof TypeError);}print(reads);
                var bad={[sym]:42};try{String.prototype[name].call(input,bad);}catch(error){print(error instanceof TypeError);}
            }
        "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "true",
                "true", "true", "true", "0", "true", "0", "true", "true", "true", "true", "true",
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "0",
                "true", "0", "true",
            ],
        );
    }

    #[test]
    fn regression_string_match_search_fallback_uses_intrinsics_and_conversion_order() {
        assert_output_in_execution_modes(
            r#"
            var original=RegExp,globalDescriptor=Object.getOwnPropertyDescriptor(globalThis,'RegExp'),globalReads=0;
            Object.defineProperty(globalThis,'RegExp',{configurable:true,get(){globalReads++;throw 'global';}});
            for(var name of ['match','search']) {
                var sym=Symbol[name],descriptor=Object.getOwnPropertyDescriptor(original.prototype,sym),log=[];
                Object.defineProperty(original.prototype,sym,{configurable:true,get(){$262.gc();log.push('invoke:get');return function(text){$262.gc();log.push('invoke:'+text.charCodeAt(0));return Object.getPrototypeOf(this)===original.prototype&&this.source==='a';};}});
                var input={[Symbol.toPrimitive](hint){$262.gc();log.push('input:'+hint);return String.fromCharCode(55296,97);}};
                var pattern={get [sym](){log.push('method');return null;},[Symbol.toPrimitive](hint){$262.gc();log.push('pattern:'+hint);return 'a';}};
                if(name==='search')Object.defineProperty(pattern,Symbol.match,{get(){throw 'IsRegExp';}});
                print(String.prototype[name].call(input,pattern));print(log.join(','));
                Object.defineProperty(original.prototype,sym,descriptor);
            }
            print(globalReads);Object.defineProperty(globalThis,'RegExp',globalDescriptor);
            var pattern=/a/;pattern[Symbol.match]=null;print(String.prototype.match.call('/a/',pattern)[0]);
            var pattern=/a/;pattern[Symbol.search]=null;print(String.prototype.search.call('/a/',pattern));
        "#,
            &[
                "true",
                "method,input:string,pattern:string,invoke:get,invoke:55296",
                "true",
                "method,input:string,pattern:string,invoke:get,invoke:55296",
                "0",
                "/a/",
                "0",
            ],
        );
    }

    #[test]
    fn regression_regexp_match_preserves_utf16_and_unicode_advancement() {
        assert_output_in_execution_modes(
            r#"
            for(var flags of ['g','gu','gv']) {
                var writes='',calls=0,hints='';
                var receiver={get flags(){$262.gc();return {[Symbol.toPrimitive](hint){$262.gc();hints+='flags:'+hint+',';return flags;}};},
                    set lastIndex(value){$262.gc();writes+=value+',';},
                    get lastIndex(){$262.gc();return {[Symbol.toPrimitive](hint){$262.gc();hints+='index:'+hint+',';return 0;}};},
                    get exec(){$262.gc();return function(input){$262.gc();if(calls++)return null;return {get 0(){$262.gc();return {[Symbol.toPrimitive](hint){$262.gc();hints+='match:'+hint+',';return '';}};}};};}
                };
                var result=RegExp.prototype[Symbol.match].call(receiver,String.fromCodePoint(128512)+'a');
                print(writes);print(result.length);print(hints);
            }
            var calls=0;
            var receiver={flags:'g',lastIndex:0,exec(input){$262.gc();if(input.charCodeAt(0)!==55296)throw 'input';if(calls++===2)return null;return {get 0(){$262.gc();return String.fromCharCode(55296+calls);}};}};
            var result=RegExp.prototype[Symbol.match].call(receiver,String.fromCharCode(55296,97));
            $262.gc();print(result.length);print(result[0].charCodeAt(0));print(result[1].charCodeAt(0));
        "#,
            &[
                "0,1,",
                "1",
                "flags:string,match:string,index:number,",
                "0,2,",
                "1",
                "flags:string,match:string,index:number,",
                "0,2,",
                "1",
                "flags:string,match:string,index:number,",
                "2",
                "55297",
                "55298",
            ],
        );
    }

    #[test]
    fn regression_regexp_match_keeps_non_global_identity_and_own_elements() {
        assert_output_in_execution_modes(
            r#"
            var reads=0,writes=0,record={get 0(){reads++;throw 'read';}};
            var receiver={flags:'',set lastIndex(v){writes++;},exec(){return record;}};
            print(RegExp.prototype[Symbol.match].call(receiver,'a')===record);print(reads);print(writes);
            var empty={flags:'g',lastIndex:3,exec(){return null;}};
            print(RegExp.prototype[Symbol.match].call(empty,'a')===null);print(empty.lastIndex);
            var count=0,readonly={flags:'g',exec(){count++;return null;}};
            Object.defineProperty(readonly,'lastIndex',{value:0,writable:false});
            try{RegExp.prototype[Symbol.match].call(readonly,'a');}catch(error){print(error instanceof TypeError);}print(count);
            var marker={},throwing={get flags(){throw marker;}};
            try{RegExp.prototype[Symbol.match].call(throwing,'a');}catch(error){print(error===marker);}
            var symbol={flags:'g',lastIndex:0,exec(){return {0:Symbol()};}};
            try{RegExp.prototype[Symbol.match].call(symbol,'a');}catch(error){print(error instanceof TypeError);}
            var setterCalls=0;
            Object.defineProperty(Array.prototype,'0',{configurable:true,set(value){setterCalls++;}});
            var result=RegExp.prototype[Symbol.match].call(/a/g,'a');
            var descriptor=Object.getOwnPropertyDescriptor(result,'0');
            delete Array.prototype[0];
            print(setterCalls);print(descriptor.value);print(descriptor.writable&&descriptor.enumerable&&descriptor.configurable);
        "#,
            &[
                "true", "0", "0", "true", "0", "true", "0", "true", "true", "0", "a", "true",
            ],
        );
    }

    #[test]
    fn regression_regexp_search_returns_index_without_conversion() {
        assert_output_in_execution_modes(
            r#"
            var reads=0,coercions=0;
            var object={valueOf(){coercions++;throw 'converted';},[Symbol.toPrimitive](){coercions++;throw 'converted';}};
            for(var index of [undefined,'index',-1.5,NaN,-0,Symbol(),1n,object,null]) {
                var receiver={lastIndex:0,exec(){return {get index(){reads++;return index;}};}};
                var actual=RegExp.prototype[Symbol.search].call(receiver,'a');
                print(Object.is(actual,index));
            }
            print(reads);print(coercions);
        "#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "true", "true", "9", "0",
            ],
        );
    }

    #[test]
    fn regression_regexp_search_restores_index_before_reading_fresh_result() {
        assert_output_in_execution_modes(
            r#"
            var log=[],reads=0;
            function previous(){return {tag:41};}
            function result(){return {get index(){$262.gc();log.push('index');return {tag:43};}};}
            var receiver={
                get lastIndex(){$262.gc();if(reads++===0){log.push('previous');return previous();}log.push('current');return 0;},
                set lastIndex(value){$262.gc();log.push(value===0?'reset':'restore:'+value.tag);},
                get exec(){$262.gc();log.push('exec:get');return function(input){$262.gc();log.push('exec:'+input.charCodeAt(0));return result();};}
            };
            var input={[Symbol.toPrimitive](hint){$262.gc();log.push('input:'+hint);return String.fromCharCode(55296,97);}};
            var value=RegExp.prototype[Symbol.search].call(receiver,input);
            print(value.tag);print(log.join(','));
            var writes=0,indexReads=0;
            var missing={get lastIndex(){return writes===0?7:0;},set lastIndex(v){writes++;},exec(){return null;}};
            print(RegExp.prototype[Symbol.search].call(missing,''));print(writes);
            for(var initial of [0,-0,NaN,undefined]) {
                var count=0;
                var stable={get lastIndex(){return initial;},set lastIndex(v){count++;},exec(){return null;}};
                RegExp.prototype[Symbol.search].call(stable,'');print(count);
            }
            var marker={},writesOnThrow=0;
            var throwing={lastIndex:7,exec(){throw marker;}};
            Object.defineProperty(throwing,'lastIndex',{get(){return 7;},set(v){writesOnThrow++;}});
            try{RegExp.prototype[Symbol.search].call(throwing,'a');}catch(error){print(error===marker);}
            print(writesOnThrow);
        "#,
            &[
                "43",
                "input:string,previous,reset,exec:get,exec:55296,current,restore:41,index",
                "-1",
                "2",
                "0",
                "1",
                "1",
                "1",
                "true",
                "1",
            ],
        );
    }

    #[test]
    fn regression_regexp_split_preserves_conversion_order_and_utf16() {
        assert_output_in_execution_modes(
            r#"
            var log=[];
            function Species(receiver,flags) {log.push('construct:'+flags.charCodeAt(0)+':'+flags.charAt(flags.length-1));return {
                set lastIndex(value) {log.push('set:'+value);},
                get lastIndex() {log.push('index');return {[Symbol.toPrimitive](hint) {log.push('index:'+hint);return 100;}};},
                exec(input) {log.push('exec:'+input.charCodeAt(0));return {get length() {log.push('length');return {[Symbol.toPrimitive](hint) {log.push('length:'+hint);return 2;}};},get 1() {log.push('capture');return 42;}};}
            };}
            var receiver={get constructor() {log.push('constructor');return {[Symbol.species]:Species};},get flags() {log.push('flags');return {[Symbol.toPrimitive](hint) {log.push('flags:'+hint);return String.fromCharCode(55296)+'g';}};}};
            var input={[Symbol.toPrimitive](hint) {log.push('input:'+hint);return String.fromCharCode(55296,97);}};
            var limit={[Symbol.toPrimitive](hint) {log.push('limit:'+hint);return 100;}};
            var output=RegExp.prototype[Symbol.split].call(receiver,input,limit);
            print(output.length);print(output[0]);print(output[1]);print(output[2]);print(log.join(','));
            for (var value of [0,1,2,-1,4294967297,Infinity,NaN]) {
                var result=RegExp.prototype[Symbol.split].call(/,/,'a,b',{[Symbol.toPrimitive]() {return value;}});
                print(result.join(':'));
            }
            var caught=0;
            for (var value of [Symbol(),1n]) {try {RegExp.prototype[Symbol.split].call(/,/,'a,b',{[Symbol.toPrimitive]() {return value;}});} catch(error) {if(error instanceof TypeError)caught++;}}
            print(caught);
        "#,
            &[
                "3",
                "",
                "42",
                "",
                "input:string,constructor,flags,flags:string,construct:55296:y,limit:number,set:0,exec:55296,index,index:number,length,length:number,capture",
                "",
                "a",
                "a:b",
                "a:b",
                "a",
                "",
                "",
                "2",
            ],
        );
    }

    #[test]
    fn regression_match_all_orders_species_and_preserves_strings_and_setters() {
        assert_output_in_execution_modes(
            r#"
            var log=[];
            function matcherFactory() {var index=0;return {
                get exec() {$262.gc();log.push('exec:get');return execute;},
                get lastIndex() {$262.gc();log.push('index:get');return {[Symbol.toPrimitive](hint) {$262.gc();log.push('index:'+hint);return index;}};},
                set lastIndex(value) {$262.gc();log.push('index:set:'+value);index=value;}
            };}
            function execute(input) {$262.gc();log.push('exec:'+input.charCodeAt(0));return resultFactory(input);}
            function resultFactory(input) {return {tag:42,input:input,get 0() {$262.gc();log.push('match:get');return {[Symbol.toPrimitive](hint) {$262.gc();log.push('match:'+hint);return '';}};}};}
            function speciesFactory() {return function Species(receiver,flags) {$262.gc();log.push('construct:'+flags.charCodeAt(0));return matcherFactory();};}
            var receiver={
                get constructor() {$262.gc();log.push('constructor');return {get [Symbol.species]() {$262.gc();log.push('species');return speciesFactory();}};},
                get flags() {$262.gc();log.push('flags:get');return {[Symbol.toPrimitive](hint) {$262.gc();log.push('flags:'+hint);return String.fromCharCode(55296)+'g';}};},
                get lastIndex() {$262.gc();log.push('lastIndex:get');return {[Symbol.toPrimitive](hint) {$262.gc();log.push('lastIndex:'+hint);return 0;}};}
            };
            var input={[Symbol.toPrimitive](hint) {$262.gc();log.push('input:'+hint);return String.fromCharCode(55296,97);}};
            var iterator=RegExp.prototype[Symbol.matchAll].call(receiver,input);
            var result=iterator.next();
            print(result.value.tag);
            print(result.value.input.charCodeAt(0));
            print(log.join(','));
            "#,
            &[
                "42",
                "55296",
                "input:string,constructor,species,flags:get,flags:string,construct:55296,lastIndex:get,lastIndex:number,index:set:0,exec:get,exec:55296,match:get,match:string,index:get,index:number,index:set:1",
            ],
        );
    }

    #[test]
    fn regression_match_all_observes_strict_set_errors_and_species_validation() {
        assert_output_in_execution_modes(
            r#"
            for (var phase of ['create','advance']) {
                var marker={},writes=0;
                function Species() {return {exec() {$262.gc();return {0:''};},get lastIndex() {return 0;},set lastIndex(value) {$262.gc();writes++;if (phase==='create'||writes===2) throw marker;}};}
                var receiver={constructor:{[Symbol.species]:Species},flags:'g',lastIndex:0};
                try {var iterator=RegExp.prototype[Symbol.matchAll].call(receiver,'a');iterator.next();print(false);} catch (error) {print(error===marker);}
                print(writes);
            }
            var reads=0;
            for (var method of [Symbol.matchAll,Symbol.split]) {
                var receiver={constructor:{[Symbol.species]:()=>({})},get flags() {reads++;throw 'flags';}};
                try {RegExp.prototype[method].call(receiver,'a');print(false);} catch (error) {print(error instanceof TypeError);}
            }
            print(reads);
            var matcher={};Object.defineProperty(matcher,'lastIndex',{value:0,writable:false});
            var receiver={constructor:{[Symbol.species]:function(){return matcher;}},flags:'g',lastIndex:0};
            try {RegExp.prototype[Symbol.matchAll].call(receiver,'a');print(false);} catch (error) {print(error instanceof TypeError);}
            for (var flags of ['g','gu','gv','']) {
                var writes=[],matchReads=0;
                function Species() {return {exec() {return {get 0() {matchReads++;return '';}};},get lastIndex() {return 0;},set lastIndex(value) {$262.gc();writes.push(value);}};}
                var receiver={constructor:{[Symbol.species]:Species},flags:flags,lastIndex:0};
                RegExp.prototype[Symbol.matchAll].call(receiver,String.fromCodePoint(128512)).next();
                print(writes.join(',')+':'+matchReads);
            }
            "#,
            &[
                "true", "1", "true", "2", "true", "true", "0", "true", "0,1:1", "0,2:1", "0,2:1",
                "0:0",
            ],
        );
    }

    #[test]
    fn regression_array_iterator_advances_before_get_and_reentrant_next() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['values','entries']) {
                var reads=0,iterator;
                var source={length:2,get 0() {$262.gc();reads++;if (reads===1) throw 'item';return 10;},1:20};
                iterator=Array.prototype[method].call(source);
                try {iterator.next();} catch (error) {print(error);}
                var value=iterator.next().value;
                print(method==='entries'?value[0]+':'+value[1]:value);
                print(reads);
                var nested,entered=false;
                source={length:2,get 0() {$262.gc();if (!entered) {entered=true;nested=iterator.next().value;}return 10;},1:20};
                iterator=Array.prototype[method].call(source);
                value=iterator.next().value;
                print(method==='entries'?value[0]+':'+value[1]:value);
                print(method==='entries'?nested[0]+':'+nested[1]:nested);
                print(iterator.next().done);
            }
            "#,
            &[
                "item", "20", "1", "10", "20", "true", "item", "1:20", "1", "0:10", "1:20", "true",
            ],
        );
    }

    #[test]
    fn regression_iterator_step_rejects_primitive_before_done_access() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            Object.defineProperty(Number.prototype,'done',{get:function() {reads++;return true;}});
            for (var Consumer of [Map, Set, WeakMap, WeakSet]) {
                var iterator = Iterator.from({next:function() {$262.gc();return 42;}});
                try {new Consumer(iterator); print(false);} catch (error) {print(error instanceof TypeError);}
            }
            print(reads);
            "#,
            &["true", "true", "true", "true", "0"],
        );
    }

    #[test]
    fn regression_bound_has_instance_dispatches_the_target_method() {
        assert_output_in_execution_modes(
            r#"
            function C() {}
            var calls = 0, value = {tag:42};
            Object.defineProperty(C, Symbol.hasInstance, {configurable:true, value:function(v) {
                $262.gc(); calls++; return this===C && v===value;
            }});
            var bound = C.bind(null).bind(null);
            print(value instanceof bound);
            print(Function.prototype[Symbol.hasInstance].call(bound,value));
            print(calls);
            Object.defineProperty(C, Symbol.hasInstance, {get:function() {$262.gc(); throw value;}});
            try {value instanceof bound; print(false);} catch (error) {print(error===value);}
            "#,
            &["true", "true", "2", "true"],
        );
    }

    #[test]
    fn regression_binary_operand_coercion_preserves_order_and_hints() {
        assert_output_in_execution_modes(
            r#"
            var events=[];
            var left={[Symbol.toPrimitive](hint) {events.push('left:'+hint);return 41;}};
            var right={[Symbol.toPrimitive](hint) {events.push('right:'+hint);$262.gc();return 1;}};
            print(left+right);print(events.join(','));events=[];
            print(left-right);print(events.join(','));events=[];
            print(left<right);print(events.join(','));
            "#,
            &[
                "42",
                "left:default,right:default",
                "40",
                "left:number,right:number",
                "false",
                "left:number,right:number",
            ],
        );
    }

    #[test]
    fn regression_binary_coercion_keeps_fresh_first_primitive() {
        assert_output_in_execution_modes(
            r#"
            function pair(bigint) {return [
                {[Symbol.toPrimitive](hint) {return bigint?BigInt(42):String.fromCharCode(97,98);}},
                {[Symbol.toPrimitive](hint) {$262.gc();return bigint?BigInt(7):String.fromCharCode(99);}}
            ];}
            for (var bigint of [false,true]) {
                var values=pair(bigint);print(values[0]<values[1]);
                values=pair(bigint);print(values[0]<=values[1]);
                values=pair(bigint);print(values[0]>values[1]);
                values=pair(bigint);print(values[0]>=values[1]);
                values=pair(bigint);print(values[0]+values[1]);
            }
            function left() {return {[Symbol.toPrimitive]() {return BigInt(42);}};}
            function right() {return {[Symbol.toPrimitive]() {$262.gc();return BigInt(7);}};}
            print(left()-right());print(left()*right());print(left()/right());print(left()%right());
            print(left()|right());print(left()^right());print(left()&right());print(left()<<right());print(left()>>right());
            print(String.fromCharCode(97,98)=={[Symbol.toPrimitive]() {$262.gc();return String.fromCharCode(97,99);}});
            print({[Symbol.toPrimitive]() {$262.gc();return BigInt(43);}}==BigInt(42));
            function symbol() {return {[Symbol.toPrimitive]() {return Symbol('fresh');}};}
            try {print(symbol()<right());}catch(error) {print(error instanceof TypeError);}
            try {print(symbol()+right());}catch(error) {print(error instanceof TypeError);}
            print(left()+{[Symbol.toPrimitive]() {$262.gc();return String.fromCharCode(55);}});
            print(left()**right());
            "#,
            &[
                "true",
                "true",
                "false",
                "false",
                "abc",
                "false",
                "false",
                "true",
                "true",
                "49",
                "35",
                "294",
                "6",
                "0",
                "47",
                "45",
                "2",
                "5376",
                "0",
                "false",
                "false",
                "true",
                "true",
                "427",
                "230539333248",
            ],
        );
    }

    #[test]
    fn regression_revoked_proxy_keeps_installed_call_and_construct_methods() {
        assert_output_in_execution_modes(
            r#"
            for (var target of [{},()=>42,function(){},class C{},async()=>42,BigInt,Symbol,Function.prototype]) {
                var r=Proxy.revocable(target,{});r.revoke();$262.gc();
                print(typeof r.proxy);
                var outer=new Proxy(r.proxy,{apply() {return 43;},construct() {return {rank:44};}});
                try {print(Reflect.apply(outer,null,[]));}catch(error) {print(error instanceof TypeError);}
                try {print(Reflect.construct(outer,[]).rank);}catch(error) {print(error instanceof TypeError);}
            }
            var events=[],r=Proxy.revocable(function(){},{});r.revoke();
            var list={get length() {events.push('length');$262.gc();return 0;}};
            var ctor=new Proxy(function(){},{construct(target,args,newTarget) {events.push('trap');print(newTarget===r.proxy);return {rank:45};}});
            print(Reflect.construct(ctor,list,r.proxy).rank);print(events.join(','));
            events=[];try {Reflect.apply(r.proxy,null,list);}catch(error) {print(error instanceof TypeError);}print(events.join(','));
            "#,
            &[
                "object",
                "true",
                "true",
                "function",
                "43",
                "true",
                "function",
                "43",
                "44",
                "function",
                "43",
                "44",
                "function",
                "43",
                "true",
                "function",
                "43",
                "44",
                "function",
                "43",
                "44",
                "function",
                "43",
                "true",
                "true",
                "45",
                "length,trap",
                "true",
                "length",
            ],
        );
    }

    #[test]
    fn regression_revoker_consumes_one_owner_and_ignores_receiver() {
        assert_output_in_execution_modes(
            r#"
            var r=Proxy.revocable({},{}),revoke=r.revoke,proxy=r.proxy;
            print(Reflect.ownKeys(r).join(','));print(Reflect.getOwnPropertyDescriptor(r,'\0quench:proxy-revoke-target')===undefined);
            for (var key of ['proxy','revoke']) {var d=Object.getOwnPropertyDescriptor(r,key);print(d.writable&&d.enumerable&&d.configurable);}
            print(revoke.name);print(revoke.length);
            var poison=new Proxy({}, {get() {throw new Error('receiver read');}});
            print(Reflect.apply(revoke,poison,[poison]));print(revoke.call(poison));print(revoke.apply(poison,[]));
            try {Reflect.getPrototypeOf(proxy);}catch(error) {print(error instanceof TypeError);}
            print(revoke.name);print(revoke.length);
            "#,
            &[
                "proxy,revoke",
                "true",
                "true",
                "true",
                "",
                "0",
                "undefined",
                "undefined",
                "undefined",
                "true",
                "",
                "0",
            ],
        );
    }

    #[test]
    fn regression_proxy_own_keys_fallback_retains_original_target_after_revocation() {
        assert_output_in_execution_modes(
            r#"
            for (var trapped of [false,true]) {
                var symbol=Symbol('entry'),events=[];
                var target=new Proxy({first:42,[symbol]:43},{ownKeys(object) {events.push('target');$262.gc();return Reflect.ownKeys(object);}});
                var r=Proxy.revocable(target,{get ownKeys() {events.push('lookup');r.revoke();$262.gc();
                    if (!trapped) return null;
                    return function(object) {events.push('trap');$262.gc();return Reflect.ownKeys(object);};
                }});
                var keys=Reflect.ownKeys(r.proxy);print(keys[0]);print(keys[1]===symbol);print(events.join(','));
                try {Reflect.ownKeys(r.proxy);}catch(error) {print(error instanceof TypeError);}
            }
            var r=Proxy.revocable(function(){},{get construct() {r.revoke();$262.gc();return function(target,args,newTarget) {print(newTarget===r.proxy);return {rank:46};};}});
            print(Reflect.construct(r.proxy,[]).rank);
            "#,
            &[
                "first",
                "true",
                "lookup,target",
                "true",
                "first",
                "true",
                "lookup,trap,target,target",
                "true",
                "true",
                "46",
            ],
        );
    }

    #[test]
    fn regression_proxy_presence_and_delete_keep_captured_targets_after_revocation() {
        assert_output_in_execution_modes(
            r#"
            for (var remove of [false,true]) {
                for (var fallback of [false,true]) {
                    var symbol=Symbol('entry'),raw={[symbol]:42},events=[];
                    var target=new Proxy(raw,{getOwnPropertyDescriptor(object,key) {events.push('descriptor');$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},isExtensible(object) {events.push('extensible');$262.gc();return Reflect.isExtensible(object);}});
                    var handler={};Object.defineProperty(handler,remove?'deleteProperty':'has',{get() {events.push('lookup');revocable.revoke();$262.gc();
                        if (fallback) return null;
                        return function(object,key) {events.push('trap');$262.gc();print(this===handler);print(object===target);print(key===symbol);return remove;};
                    }});
                    var revocable=Proxy.revocable(target,handler);
                    print(remove?Reflect.deleteProperty(revocable.proxy,symbol):Reflect.has(revocable.proxy,symbol));print(events.join(','));
                    try {if (remove) Reflect.deleteProperty(revocable.proxy,symbol);else Reflect.has(revocable.proxy,symbol);}catch(error) {print(error instanceof TypeError);}
                }
                events=[];target=new Proxy({}, {getOwnPropertyDescriptor() {events.push('descriptor');$262.gc();return undefined;},isExtensible() {events.push('extensible');throw new Error('unexpected');}});
                var source=new Proxy(target,{has() {return false;},deleteProperty() {return true;}});
                print(remove?Reflect.deleteProperty(source,'missing'):Reflect.has(source,'missing'));print(events.join(','));
            }
            "#,
            &[
                "true",
                "true",
                "true",
                "false",
                "lookup,trap,descriptor,extensible",
                "true",
                "true",
                "lookup",
                "true",
                "false",
                "descriptor",
                "true",
                "true",
                "true",
                "true",
                "lookup,trap,descriptor,extensible",
                "true",
                "true",
                "lookup",
                "true",
                "true",
                "descriptor",
            ],
        );
    }

    #[test]
    fn regression_proxy_presence_and_delete_coerce_keys_before_dispatch() {
        assert_output_in_execution_modes(
            r#"
            for (var remove of [false,true]) {
                var events=[],handler={};
                Object.defineProperty(handler,remove?'deleteProperty':'has',{get() {events.push('lookup');$262.gc();return function(object,key) {events.push('trap:'+key);return true;};}});
                var source=new Proxy({},handler),key={toString() {events.push('key');$262.gc();return 'entry';}};
                print(remove?Reflect.deleteProperty(source,key):key in source);print(events.join(','));
                events=[];var revoked=Proxy.revocable({},handler);revoked.revoke();
                key={toString() {events.push('key');throw {kind:'coercion'};}};
                try {if (remove) Reflect.deleteProperty(revoked.proxy,key);else print(key in revoked.proxy);}catch(error) {print(error.kind);}
                print(events.join(','));
                events=[];revoked=Proxy.revocable({},handler);
                key={toString() {events.push('key');revoked.revoke();$262.gc();return 'entry';}};
                try {if (remove) Reflect.deleteProperty(revoked.proxy,key);else print(key in revoked.proxy);}catch(error) {print(error instanceof TypeError);}
                print(events.join(','));
            }
            "#,
            &[
                "true",
                "key,lookup,trap:entry",
                "coercion",
                "key",
                "true",
                "key",
                "true",
                "key,lookup,trap:entry",
                "coercion",
                "key",
                "true",
                "key",
            ],
        );
    }

    #[test]
    fn regression_proxy_presence_invariants_skip_extensibility_for_frozen_properties() {
        assert_output_in_execution_modes(
            r#"
            for (var remove of [false,true]) {
                var events=[],raw={};Object.defineProperty(raw,'entry',{value:42,configurable:false});
                var target=new Proxy(raw,{getOwnPropertyDescriptor(object,key) {events.push('descriptor');$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},isExtensible() {events.push('extensible');throw {kind:'extensible'};}});
                var source=new Proxy(target,{has() {events.push('has');return false;},deleteProperty() {events.push('delete');return true;}});
                try {if (remove) Reflect.deleteProperty(source,'entry');else Reflect.has(source,'entry');}catch(error) {print(error instanceof TypeError);}
                print(events.join(','));
                events=[];raw={entry:42};target=new Proxy(raw,{getOwnPropertyDescriptor(object,key) {events.push('descriptor');$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},isExtensible(object) {events.push('extensible');$262.gc();return Reflect.isExtensible(object);}});
                source=new Proxy(target,{has() {events.push('has');return false;},deleteProperty() {events.push('delete');return true;}});
                print(remove?Reflect.deleteProperty(source,'entry'):Reflect.has(source,'entry'));print(events.join(','));
                Object.preventExtensions(raw);events=[];
                try {if (remove) Reflect.deleteProperty(source,'entry');else Reflect.has(source,'entry');}catch(error) {print(error instanceof TypeError);}
                print(events.join(','));
            }
            "#,
            &[
                "true",
                "has,descriptor",
                "false",
                "has,descriptor,extensible",
                "true",
                "has,descriptor,extensible",
                "true",
                "delete,descriptor",
                "true",
                "delete,descriptor,extensible",
                "true",
                "delete,descriptor,extensible",
            ],
        );
    }

    #[test]
    fn regression_proxy_invocations_keep_captured_operands_after_revocation() {
        assert_output_in_execution_modes(
            r#"
            for (var construct of [false,true]) {
                for (var fallback of [false,true]) {
                    var events=[],arg={rank:47},receiver={rank:46};
                    function Target(value) {$262.gc();events.push('target');return {rank:42,arg:value,receiver:this};}
                    function Destination() {}
                    var handler={};
                    Object.defineProperty(handler,construct?'construct':'apply',{get() {
                        events.push('lookup');revocable.revoke();$262.gc();
                        if (fallback) return null;
                        return function(target,second,third) {
                            $262.gc();events.push('trap');
                            print(this===handler);print(target===Target);
                            print(Array.isArray(construct?second:third));
                            print((construct?third:second)===(construct?Destination:receiver));
                            return {rank:42,arg:(construct?second:third)[0]};
                        };
                    }});
                    var revocable=Proxy.revocable(Target,handler);
                    var result=construct?Reflect.construct(revocable.proxy,[arg],Destination):Reflect.apply(revocable.proxy,receiver,[arg]);
                    print(result.rank);print(result.arg===arg);print(result.arg.rank);print(events.join(','));
                    try {Reflect.apply(revocable.proxy,receiver,[]);}catch(error) {print(error instanceof TypeError);}
                }
            }
            "#,
            &[
                "true",
                "true",
                "true",
                "true",
                "42",
                "true",
                "47",
                "lookup,trap",
                "true",
                "42",
                "true",
                "47",
                "lookup,target",
                "true",
                "true",
                "true",
                "true",
                "true",
                "42",
                "true",
                "47",
                "lookup,trap",
                "true",
                "42",
                "true",
                "47",
                "lookup,target",
                "true",
            ],
        );
    }

    #[test]
    fn regression_proxy_mutation_fallback_keeps_captured_targets_alive() {
        assert_output_in_execution_modes(
            r#"
            var revocable=Proxy.revocable({}, {get setPrototypeOf() {revocable.revoke();$262.gc();return null;}});
            print(Reflect.setPrototypeOf(revocable.proxy,{rank:42}));
            revocable=Proxy.revocable({}, {get preventExtensions() {revocable.revoke();$262.gc();return undefined;}});
            print(Reflect.preventExtensions(revocable.proxy));
            try {Object.preventExtensions(revocable.proxy);}catch(error) {print(error instanceof TypeError);}
            revocable=Proxy.revocable({},{});revocable.revoke();
            var object={};print(Object.setPrototypeOf(object,revocable.proxy)===object);print(Object.getPrototypeOf(object)===revocable.proxy);
            print(Object.setPrototypeOf(1,null));print(Object.preventExtensions(null));
            "#,
            &["true", "true", "true", "true", "true", "1", "null"],
        );
    }

    #[test]
    fn regression_object_and_reflect_share_boolean_mutation_semantics() {
        assert_output_in_execution_modes(
            r#"
            var target={},prototype={rank:43},events=[];
            var source=new Proxy(target,{setPrototypeOf(object,proto) {events.push('set');$262.gc();return false;},preventExtensions() {events.push('prevent');$262.gc();return false;}});
            print(Reflect.setPrototypeOf(source,prototype));try {Object.setPrototypeOf(source,prototype);}catch(error) {print(error instanceof TypeError);}
            print(Reflect.preventExtensions(source));try {Object.preventExtensions(source);}catch(error) {print(error instanceof TypeError);}
            print(events.join(','));
            Object.preventExtensions(target);print(Reflect.setPrototypeOf(target,Object.getPrototypeOf(target)));print(Reflect.setPrototypeOf(target,prototype));
            try {Object.setPrototypeOf(target,prototype);}catch(error) {print(error instanceof TypeError);}
            print(Object.setPrototypeOf(Object.prototype,null)===Object.prototype);print(Reflect.setPrototypeOf(Object.prototype,prototype));
            var plain={},other=Object.create(plain);print(Reflect.setPrototypeOf(plain,other));
            var reads=0;var exotic=new Proxy({}, {getPrototypeOf() {reads++;throw new Error('cycle walk');}});
            print(Reflect.setPrototypeOf(plain,exotic));print(Object.getPrototypeOf(plain)===exotic);print(reads);
            "#,
            &[
                "false",
                "true",
                "false",
                "true",
                "set,set,prevent,prevent",
                "true",
                "false",
                "true",
                "true",
                "false",
                "false",
                "true",
                "true",
                "0",
            ],
        );
    }

    #[test]
    fn regression_proxy_reads_keep_fresh_results_through_descriptor_validation() {
        assert_output_in_execution_modes(
            r#"
            var symbol=Symbol('entry'),events=[];
            var target=new Proxy({}, {getOwnPropertyDescriptor(object,key) {events.push('descriptor');$262.gc();return undefined;}});
            var source=new Proxy(target, {get get() {events.push('lookup');$262.gc();return function(object,key,receiver) {
                events.push('trap');return {rank:42,key:key,receiver:receiver};
            };}});
            print(source.entry.rank);print(events.join(','));events=[];
            var receiver={rank:43};var result=Reflect.get(source,symbol,receiver);
            print(result.rank);print(result.key===symbol);print(result.receiver===receiver);print(events.join(','));
            var revocable=Proxy.revocable({get entry() {$262.gc();return this.rank;}}, {get get() {revocable.revoke();$262.gc();return null;}});
            print(Reflect.get(revocable.proxy,'entry',{rank:44}));
            try {revocable.proxy.entry;}catch(error) {print(error instanceof TypeError);}
            revocable=Proxy.revocable({[symbol]:45},{get get() {revocable.revoke();$262.gc();return undefined;}});
            print(revocable.proxy[symbol]);
            try {revocable.proxy[symbol];}catch(error) {print(error instanceof TypeError);}
            "#,
            &[
                "42",
                "lookup,trap,descriptor",
                "42",
                "true",
                "true",
                "lookup,trap,descriptor",
                "44",
                "true",
                "45",
                "true",
            ],
        );
    }

    #[test]
    fn regression_proxy_prototype_results_survive_validation_and_revocation() {
        assert_output_in_execution_modes(
            r#"
            var target=new Proxy({}, {isExtensible(object) {$262.gc();return Reflect.isExtensible(object);}});
            var source=new Proxy(target, {getPrototypeOf() {return {rank:42};}});
            print(Object.getPrototypeOf(source).rank);print(Reflect.getPrototypeOf(source).rank);
            var revocable=Proxy.revocable(Object.create({rank:43}), {get getPrototypeOf() {revocable.revoke();$262.gc();return null;}});
            print(Object.getPrototypeOf(revocable.proxy).rank);
            try {Object.getPrototypeOf(revocable.proxy);}catch(error) {print(error instanceof TypeError);}
            print(Object.getPrototypeOf('ab')===String.prototype);
            "#,
            &["42", "42", "43", "true", "true"],
        );
    }

    #[test]
    fn regression_proxy_descriptor_coercion_and_validation_order() {
        assert_output_in_execution_modes(
            r#"
            var events=[];
            var target=new Proxy({}, {
                getOwnPropertyDescriptor() {events.push('target-desc');$262.gc();return undefined;},
                isExtensible() {events.push('extensible');$262.gc();return true;}
            });
            var source=new Proxy(target, {get getOwnPropertyDescriptor() {events.push('get-trap');$262.gc();return function() {
                events.push('trap');return {get enumerable() {events.push('enumerable');$262.gc();return true;},configurable:true,
                    get value() {events.push('value');$262.gc();return {rank:44};},writable:true};
            };}});
            var key={toString() {events.push('key');$262.gc();return 'entry';}};
            var result=Object.getOwnPropertyDescriptor(source,key);print(result.value.rank);print(events.join(','));
            events=[];result=Reflect.getOwnPropertyDescriptor(source,key);print(result.value.rank);print(events.join(','));
            events=[];
            source=new Proxy(target,{getOwnPropertyDescriptor() {events.push('trap');return undefined;}});
            print(Object.getOwnPropertyDescriptor(source,'missing'));print(events.join(','));
            "#,
            &[
                "44",
                "key,get-trap,trap,target-desc,extensible,enumerable,value",
                "44",
                "key,get-trap,trap,target-desc,extensible,enumerable,value",
                "undefined",
                "trap,target-desc",
            ],
        );
    }

    #[test]
    fn regression_proxy_descriptor_compatibility_uses_shared_records() {
        assert_output_in_execution_modes(
            r#"
            var target={};Object.defineProperty(target,'entry',{value:1,writable:true,configurable:false});
            var descriptor={value:2,writable:true,configurable:false};
            var source=new Proxy(target,{getOwnPropertyDescriptor() {return descriptor;}});
            print(Object.getOwnPropertyDescriptor(source,'entry').value);
            descriptor.writable=false;try {Object.getOwnPropertyDescriptor(source,'entry');}catch(error) {print(error instanceof TypeError);}
            descriptor={get:undefined,configurable:false};try {Object.getOwnPropertyDescriptor(source,'entry');}catch(error) {print(error instanceof TypeError);}
            var revocable=Proxy.revocable({entry:45},{get getOwnPropertyDescriptor() {revocable.revoke();$262.gc();return undefined;}});
            print(Object.getOwnPropertyDescriptor(revocable.proxy,'entry').value);
            try {Object.getOwnPropertyDescriptor(revocable.proxy,'entry');}catch(error) {print(error instanceof TypeError);}
            var badKey={toString() {throw {kind:'key'};}};
            try {Object.getOwnPropertyDescriptor(null,badKey);}catch(error) {print(error instanceof TypeError);}
            "#,
            &["2", "true", "true", "45", "true", "true"],
        );
    }

    #[test]
    fn regression_for_in_preserves_collecting_key_snapshots() {
        assert_output_in_execution_modes(
            r#"
            function source() {return new Proxy({first:1,later:2}, {
                getOwnPropertyDescriptor(object,key) {$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},
                getPrototypeOf() {$262.gc();return new Proxy({inherited:3}, {
                    getOwnPropertyDescriptor(object,key) {$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},
                    getPrototypeOf() {$262.gc();return null;}
                });}
            });}
            var keys=[];for(var key in source()) {$262.gc();keys.push(key);}print(keys.join(','));
            var object=Object.create({hidden:1,inherited:2});
            Object.defineProperty(object,'hidden',{value:3,enumerable:false});object.first=4;
            keys=[];for(var key in object) {keys.push(key);}print(keys.join(','));
            keys=[];for(var key in 'ab') {keys.push(key);}print(keys.join(','));
            keys=[];for(var key in null) {keys.push(key);}for(var key in undefined) {keys.push(key);}print(keys.length);
            "#,
            &["first,later,inherited", "first,inherited", "0,1", "0"],
        );
    }

    #[test]
    fn regression_property_copy_snapshots_survive_collecting_getters() {
        assert_output_in_execution_modes(
            r#"
            var symbol=Symbol('entry');
            function source() {return new Proxy({first:42, later:43, [symbol]:44}, {
                getOwnPropertyDescriptor(object,key) {$262.gc(); return Reflect.getOwnPropertyDescriptor(object,key);},
                get(object,key) {$262.gc(); return Reflect.get(object,key);}
            });}
            var target=Object.assign(1,source());
            print(target.valueOf()); print(target.first);print(target.later);print(target[symbol]);
            var spread={...source()};print(spread.first);print(spread.later);print(spread[symbol]);
            var {first,...rest}=source(); print(first);print(Object.hasOwn(rest,'first'));print(rest.later);print(rest[symbol]);
            var getterReads=0;
            var overwritten={get first() {getterReads++;return 99;},...{first:42}};
            print(overwritten.first);print(Object.getOwnPropertyDescriptor(overwritten,'first').writable);print(getterReads);
            var setterCalls=0;
            Object.assign({set first(value) {$262.gc();setterCalls++;print(value.rank);}}, {get first() {$262.gc();return {rank:45};}});
            print(setterCalls);
            Object.assign=function() {throw new Error('overridden assign');};
            print(({...{first:46}}).first);print(Object.keys({...null,...undefined}).length);
            "#,
            &[
                "1", "42", "43", "44", "42", "43", "44", "42", "false", "43", "44", "42", "true",
                "0", "45", "1", "46", "0",
            ],
        );
    }

    #[test]
    fn regression_descriptor_snapshot_preserves_fresh_destination() {
        assert_output_in_execution_modes(
            r#"
            var symbol=Symbol('entry'), events=[];
            var source=new Proxy({first:42,later:43,[symbol]:44}, {
                getOwnPropertyDescriptor(object,key) {events.push(typeof key==='symbol' ? 'symbol' : key);$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);}
            });
            var result=Object.getOwnPropertyDescriptors(source);
            print(result.first.value); print(result.later.value); print(result[symbol].value);print(events.join(','));
            print(Object.keys(result).join(','));
            var attributes=Object.getOwnPropertyDescriptor(result,'first');print(attributes.writable && attributes.enumerable && attributes.configurable);
            print(Object.getOwnPropertyDescriptors('ab')[0].value);
            "#,
            &[
                "42",
                "43",
                "44",
                "first,later,symbol",
                "first,later",
                "true",
                "a",
            ],
        );
    }

    #[test]
    fn regression_array_like_arguments_keep_earlier_collecting_values() {
        assert_output_in_execution_modes(
            r#"
            function list() {return {length:{valueOf() {$262.gc(); return 2;}},
                get 0() {return {rank:42};}, get 1() {$262.gc(); return {rank:43};}};}
            function called(first, second) {print(first.rank); print(second.rank);}
            Reflect.apply(called, null, list()); called.apply(null, list());
            var result = Reflect.construct(function(first,second) {this.first=first; this.second=second;}, list());
            print(result.first.rank); print(result.second.rank);
            var events=[];
            var proxy = new Proxy({}, {ownKeys() {return {length:2,
                get 0() {events.push('first'); return 'fresh-' + events.length;},
                get 1() {$262.gc(); events.push('second'); return Symbol('later');}};}});
            var keys=Reflect.ownKeys(proxy);
            print(keys[0]); print(keys[1].description); print(events.join(','));
            "#,
            &[
                "42",
                "43",
                "42",
                "43",
                "42",
                "43",
                "fresh-1",
                "later",
                "first,second",
            ],
        );
    }

    #[test]
    fn regression_proxy_own_keys_checks_types_and_observable_target_invariants() {
        assert_output_in_execution_modes(
            r#"
            function outcome(action) {try {return String(action());} catch(error) {return error instanceof TypeError ? 'TypeError' : error;}}
            var events=[];
            print(outcome(function() {return Reflect.ownKeys(new Proxy({}, {ownKeys() {return {
                length:2, get 0() {events.push('invalid'); return undefined;},
                get 1() {events.push('later'); throw 'later error';}
            };}}));})); print(events.join(','));
            for (var sealed of [false,true]) {
                var events=[], target={first:42, later:43};
                Object.defineProperty(target,'first',{configurable:false});
                if (sealed) Object.preventExtensions(target);
                var inner=new Proxy(target, {isExtensible(object) {events.push('extensible'); $262.gc(); return Reflect.isExtensible(object);},
                    ownKeys(object) {events.push('target-keys'); $262.gc(); return Reflect.ownKeys(object);},
                    getOwnPropertyDescriptor(object,key) {events.push('descriptor:' + key); $262.gc(); return Reflect.getOwnPropertyDescriptor(object,key);}});
                var outer=new Proxy(inner,{ownKeys() {events.push('trap'); return ['later','first'];}});
                print(Reflect.ownKeys(outer).join(',')); print(events.join(','));
                events=[];
                print(outcome(function() {return Reflect.ownKeys(new Proxy(inner,{ownKeys() {events.push('trap'); return ['later'];}}));}));
                print(events.join(','));
            }
            "#,
            &[
                "TypeError",
                "invalid",
                "later,first",
                "trap,extensible,target-keys,descriptor:first,descriptor:later",
                "TypeError",
                "trap,extensible,target-keys,descriptor:first,descriptor:later",
                "later,first",
                "trap,extensible,target-keys,descriptor:first,descriptor:later",
                "TypeError",
                "trap,extensible,target-keys,descriptor:first,descriptor:later",
            ],
        );
    }

    #[test]
    fn regression_own_enumeration_survives_collecting_callbacks() {
        assert_output_in_execution_modes(
            r#"
            for (var kind of ['values', 'entries']) {
                var events = [];
                var object = {get first() {events.push('first'); return {rank:42};},
                    get second() {$262.gc(); events.push('second'); return 43;}};
                var result = Object[kind](object);
                print(kind === 'values' ? result[0].rank : result[0][1].rank);
                print(kind === 'values' ? result[1] : result[1][1]);
                print(events.join(','));
            }
            for (var kind of ['keys', 'values', 'entries']) {
                var events = [], reads = 0;
                var object = new Proxy({}, {
                    ownKeys() {return ['first','later',Symbol('ignored')];},
                    getOwnPropertyDescriptor(object,key) {
                        events.push('descriptor:' + key); $262.gc();
                        return {value:undefined, enumerable:true, configurable:true, writable:true};
                    },
                    get(object,key) {reads++; events.push('get:' + key); $262.gc(); return {rank:42};}
                });
                var result = Object[kind](object);
                print(result.length);
                print(kind === 'keys' ? result.join(',') : kind === 'values' ? result[0].rank + ',' + result[1].rank : result[0][0] + ':' + result[0][1].rank + ',' + result[1][0] + ':' + result[1][1].rank);
                print(reads); print(events.join(','));
            }
            "#,
            &[
                "42",
                "43",
                "first,second",
                "42",
                "43",
                "first,second",
                "2",
                "first,later",
                "0",
                "descriptor:first,descriptor:later",
                "2",
                "42,42",
                "2",
                "descriptor:first,get:first,descriptor:later,get:later",
                "2",
                "first:42,later:42",
                "2",
                "descriptor:first,get:first,descriptor:later,get:later",
            ],
        );
    }

    #[test]
    fn regression_object_create_preserves_collected_descriptor_records() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            var result = Object.create(null, {
                first: {get value() {reads++; return {rank:42};}},
                second: {get get() {reads++; $262.gc(); return function() {return 43;};}},
                third: {get value() {$262.gc(); return 44;}}
            });
            print(Object.getPrototypeOf(result) === null); print(result.first.rank);
            print(result.second); print(result.third); print(reads);
            var target = {};
            var marker = {rank:45};
            try {Object.defineProperties(target, {first:{value:42}, get second() {$262.gc(); throw marker;}});}
            catch (error) {print(error === marker);}
            print(Object.hasOwn(target,'first'));
            "#,
            &["true", "42", "43", "44", "2", "true", "false"],
        );
    }

    #[test]
    fn regression_property_definition_parses_once_before_proxy_effects() {
        assert_output_in_execution_modes(
            r#"
            for (var define of [Object.defineProperty, Reflect.defineProperty]) {
                var events = [];
                var input = {enumerable:true, configurable:true,
                    get value() {events.push('value'); $262.gc(); return {rank:42};},
                    get writable() {events.push('writable'); $262.gc(); return true;}};
                var target = {};
                var proxy = new Proxy(target, {get defineProperty() {events.push('trap-getter'); $262.gc();
                    return function(target, key, descriptor) {
                        events.push('trap'); $262.gc(); print(descriptor === input);
                        print(Reflect.ownKeys(descriptor).join(','));
                        Object.defineProperty(target, key, descriptor); return true;
                    };
                }});
                define(proxy, {[Symbol.toPrimitive]() {events.push('key'); $262.gc(); return 'answer';}}, input);
                print(events.join(','));
                print(target.answer.rank);
            }
            "#,
            &[
                "false",
                "value,writable,enumerable,configurable",
                "key,value,writable,trap-getter,trap",
                "42",
                "false",
                "value,writable,enumerable,configurable",
                "key,value,writable,trap-getter,trap",
                "42",
            ],
        );
    }

    #[test]
    fn regression_reflect_definition_preserves_thrown_completion() {
        assert_output_in_execution_modes(
            r#"
            var marker = {rank:42};
            try {Reflect.defineProperty([], 'length', {value:{valueOf() {$262.gc(); throw marker;}}}); print(false);}
            catch (error) {print(error === marker);}
            try {Reflect.defineProperty([], 'length', {value:-1}); print(false);}
            catch (error) {print(error instanceof RangeError);}
            var frozen = Object.freeze({answer:1});
            print(Reflect.defineProperty(frozen, 'answer', {value:2}));
            print(Reflect.defineProperty(frozen, 'other', {value:2}));
            print(Reflect.defineProperty(new Uint8Array(0), '0', {value:2}));
            "#,
            &["true", "true", "false", "false", "false"],
        );
    }

    #[test]
    fn regression_internal_definitions_ignore_descriptor_prototype_fields() {
        assert_output_in_execution_modes(
            r#"
            Object.defineProperty(Object.prototype, 'get', {configurable:true, get() {$262.gc(); throw 'inherited-get';}});
            var reflected, mapped, flattened, from;
            try {
                var descriptor = Object.create(null); descriptor.value = {rank:42}; descriptor.configurable = true;
                reflected = {}; print(Reflect.defineProperty(reflected, 'answer', descriptor));
                mapped = [1,2].map(value => value + 1);
                flattened = [[1],[2]].flat();
                from = Array.from([1,2], value => value + 2);
            } finally {delete Object.prototype.get;}
            print(reflected.answer.rank); print(mapped.join(',')); print(flattened.join(',')); print(from.join(','));
            "#,
            &["true", "42", "2,3", "1,2", "3,4"],
        );
    }

    #[test]
    fn regression_proxy_definition_invariants_use_original_record() {
        assert_output_in_execution_modes(
            r#"
            for (var define of [Object.defineProperty, Reflect.defineProperty]) {
                var proxy = new Proxy({}, {defineProperty(target,key,descriptor) {
                    descriptor.configurable = true; $262.gc(); return true;
                }});
                try {define(proxy, 'answer', {value:42, configurable:false}); print(false);}
                catch (error) {print(error instanceof TypeError);}
            }
            "#,
            &["true", "true"],
        );
    }

    #[test]
    fn regression_constructor_operands_survive_collecting_prototype_lookup() {
        assert_output_in_execution_modes(
            r#"
            var result = new (new Proxy(function C(value) {
                $262.gc(); this.answer = value.answer;
            }, {get(target, key, receiver) {
                if (key === 'prototype') {$262.gc(); return {};}
                return Reflect.get(target, key, receiver);
            }}))({answer: 42});
            print(result.answer);
            "#,
            &["42"],
        );
    }

    #[test]
    fn regression_collecting_getters_keep_current_instruction_registers() {
        assert_output_in_execution_modes(
            r#"
            var source = {get first() {return {answer: 42};}, get second() {$262.gc(); return 7;}};
            print([source.first, source.second][0].answer);
            print(({first: source.first, second: source.second}).first.answer);
            function first(left, right) {return left.answer;}
            print(first(source.first, source.second));
            "#,
            &["42", "42", "42"],
        );
    }

    #[test]
    fn regression_spread_and_aggregate_lists_survive_collection() {
        assert_output_in_execution_modes(
            r#"
            for (var mode of ['spread', 'aggregate']) {
                var nextReads = 0;
                var source = {[Symbol.iterator]() {
                    var index = 0;
                    return {get next() {
                        nextReads++;
                        $262.gc();
                        return function() {
                            $262.gc();
                            if (index === 3) return {done: true};
                            return {get done() {$262.gc(); return false;}, value: {rank: ++index}};
                        };
                    }};
                }};
                var result = mode === 'spread' ? [...source] : new AggregateError(source).errors;
                print(result.length);
                print(result.map(value => value.rank).join(','));
                print(nextReads);
            }
            var cause = {marker: 7};
            var error = new AggregateError([{rank: 1}], {toString() {$262.gc(); return 'message';}}, {
                get cause() {$262.gc(); return cause;}
            });
            print(error.message);
            print(error.cause === cause);
            print(error.errors[0].rank);
            print(Object.getOwnPropertyDescriptor(error, 'errors').enumerable);
            "#,
            &[
                "3", "1,2,3", "1", "3", "1,2,3", "1", "message", "true", "1", "false",
            ],
        );
    }

    #[test]
    fn regression_aggregate_error_preserves_effect_order() {
        assert_output_in_execution_modes(
            r#"
            var log = [];
            var prototype = {};
            var target = new Proxy(function Other() {}, {get(object, key, receiver) {
                if (key === 'prototype') {log.push('prototype'); $262.gc(); return prototype;}
                return Reflect.get(object, key, receiver);
            }});
            var cause = {marker: 7};
            var options = new Proxy({}, {
                has(object, key) {log.push('has'); $262.gc(); return true;},
                get(object, key) {log.push('cause'); $262.gc(); return cause;}
            });
            var source = {get [Symbol.iterator]() {log.push('iterator'); $262.gc(); return function() {
                var index = 0;
                return {next() {log.push('next'); $262.gc(); return index++ ? {done: true} : {value: {rank: 1}, done: false};}};
            };}};
            var message = {toString() {log.push('message'); $262.gc(); return 'message';}};
            var error = Reflect.construct(AggregateError, [source, message, options], target);
            print(log.join(','));
            print(Object.getPrototypeOf(error) === prototype);
            print(error.cause === cause);
            print(error.errors[0].rank);
            for (var key of ['message', 'cause', 'errors']) {
                var descriptor = Object.getOwnPropertyDescriptor(error, key);
                print(descriptor.writable && descriptor.configurable && !descriptor.enumerable);
            }
            var SavedAggregateError = AggregateError;
            AggregateError = function Poison() {};
            function InvalidPrototypeTarget() {}
            InvalidPrototypeTarget.prototype = null;
            var fallback = Reflect.construct(SavedAggregateError, [[]], InvalidPrototypeTarget);
            print(Object.getPrototypeOf(fallback) === SavedAggregateError.prototype);
            AggregateError = SavedAggregateError;
            for (var mode of ['spread', 'aggregate']) {
                for (var phase of ['next', 'done', 'value']) {
                    var marker = {};
                    var closed = 0;
                    var source = {[Symbol.iterator]() {return {
                        next() {
                            if (phase === 'next') throw marker;
                            return {get done() {if (phase === 'done') throw marker; return false;}, get value() {throw marker;}};
                        },
                        return() {closed++; return {};}
                    };}};
                    try {if (mode === 'spread') [...source]; else new AggregateError(source);}
                    catch (error) {print(error === marker);}
                    print(closed);
                }
            }
            "#,
            &[
                "prototype,message,has,cause,iterator,next,next",
                "true",
                "true",
                "1",
                "true",
                "true",
                "true",
                "true",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
            ],
        );
    }

    #[test]
    fn regression_typed_iterable_roots_survive_collection() {
        assert_output_in_execution_modes(
            r#"
            for (var mode of ['construct', 'from']) {
                for (var Constructor of [Uint8Array, BigInt64Array, BigUint64Array]) {
                    var bigint = Constructor !== Uint8Array;
                    var source = {[Symbol.iterator]() {
                        var index = 0;
                        return {next() {
                            $262.gc();
                            if (index === 3) return {done: true};
                            var rank = ++index;
                            return {get done() {$262.gc(); return false;}, get value() {
                                return {valueOf() {$262.gc(); return bigint ? BigInt(rank) : rank;}};
                            }};
                        }};
                    }};
                    var result = mode === 'construct' ? new Constructor(source) : Constructor.from(source);
                    print(result.join(','));
                }
            }
            for (var mode of ['construct', 'from']) {
                var log = [];
                var marker = {};
                var source = {[Symbol.iterator]() {return {
                    next() {throw marker;},
                    return() {log.push('close'); return {};}
                };}};
                try {if (mode === 'construct') new Uint8Array(source); else Uint8Array.from(source);}
                catch (error) {print(error === marker);}
                print(log.length);
            }
            "#,
            &[
                "1,2,3", "1,2,3", "1,2,3", "1,2,3", "1,2,3", "1,2,3", "true", "0", "true", "0",
            ],
        );
    }

    #[test]
    fn regression_typed_array_like_conversion_is_incremental() {
        assert_output_in_execution_modes(
            r#"
            for (var Constructor of [Uint8Array, BigInt64Array, BigUint64Array]) {
                var log = [];
                var converted = false;
                var source = {
                    get [Symbol.iterator]() {$262.gc(); return undefined;},
                    get length() {$262.gc(); return 2;},
                    get 0() {log.push('get0'); return {valueOf() {$262.gc(); log.push('convert0'); converted = true; return '7';}};},
                    get 1() {log.push('get1'); return {valueOf() {$262.gc(); log.push('convert1'); return converted ? '8' : '0';}};}
                };
                print(new Constructor(source).join(','));
                print(log.join(','));
                print(new Constructor({length: -1}).length);
                try {new Constructor({length: Number.MAX_SAFE_INTEGER + 1});}
                catch (error) {print(error instanceof RangeError);}
            }
            "#,
            &[
                "7,8",
                "get0,convert0,get1,convert1",
                "0",
                "true",
                "7,8",
                "get0,convert0,get1,convert1",
                "0",
                "true",
                "7,8",
                "get0,convert0,get1,convert1",
                "0",
                "true",
            ],
        );
    }

    #[test]
    fn regression_typed_fill_value_survives_collecting_bounds() {
        assert_output_in_execution_modes(
            r#"
            for (var Constructor of [BigInt64Array, BigUint64Array]) {
                for (var bound of ['start', 'end']) {
                    var log = [];
                    var source = new Constructor([1n, 2n, 3n]);
                    var value = {valueOf() {log.push('value'); return 7n;}};
                    var index = {valueOf() {log.push(bound); $262.gc(); return bound === 'start' ? 1 : 2;}};
                    var result = bound === 'start' ? source.fill(value, index) : source.fill(value, 0, index);
                    print(source.join(','));
                    print(result === source);
                    print(log.join(','));
                }
            }
            for (var phase of ['value', 'start', 'end']) {
                var source = new BigInt64Array(3);
                var log = [];
                function step(name, result) {
                    return {valueOf() {
                        log.push(name);
                        if (phase === name) { $262.detachArrayBuffer(source.buffer); $262.gc(); }
                        return result;
                    }};
                }
                try { source.fill(step('value', 7n), step('start', 0), step('end', 2)); }
                catch (error) {print(error instanceof TypeError);}
                print(log.join(','));
            }
            "#,
            &[
                "1,7,7",
                "true",
                "value,start",
                "7,7,3",
                "true",
                "value,end",
                "1,7,7",
                "true",
                "value,start",
                "7,7,3",
                "true",
                "value,end",
                "true",
                "value,start,end",
                "true",
                "value,start,end",
                "true",
                "value,start,end",
            ],
        );
    }

    #[test]
    fn regression_typed_sort_snapshots_survive_collecting_comparators() {
        assert_output_in_execution_modes(
            r#"
            for (var Constructor of [BigInt64Array, BigUint64Array]) {
                for (var method of ['sort', 'toSorted']) {
                    var source = new Constructor([3n, 1n, 2n, 4n]);
                    var result = source[method]((left, right) => {
                        $262.gc();
                        return {valueOf() {
                            $262.gc();
                            return left < right ? -1 : left > right ? 1 : 0;
                        }};
                    });
                    print(result.join(','));
                    print(result === source);
                }
            }
            var numeric = new Float64Array([NaN, 0, -0, 2, -1]).toSorted();
            print(numeric[0]);
            print(Object.is(numeric[1], -0));
            print(Object.is(numeric[2], 0));
            print(numeric[3]);
            print(Number.isNaN(numeric[4]));
            "#,
            &[
                "1,2,3,4", "true", "1,2,3,4", "false", "1,2,3,4", "true", "1,2,3,4", "false", "-1",
                "true", "true", "2", "true",
            ],
        );
    }

    #[test]
    fn regression_array_sort_snapshots_survive_collecting_guest_effects() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['sort', 'toSorted']) {
                for (var custom of [true, false]) {
                    var source = [0, 1, 2];
                    var written = [];
                    for (let index of [0, 1, 2]) {
                        Object.defineProperty(source, index, {
                            get() {
                                if (written[index]) return written[index];
                                $262.gc();
                                return {rank: 3 - index, toString() {$262.gc(); return String(this.rank);}};
                            },
                            set(value) {$262.gc(); written[index] = value;}
                        });
                    }
                    var result = custom ? source[method]((left, right) => {
                        $262.gc(); return {valueOf() {$262.gc(); return left.rank - right.rank;}};
                    }) : source[method]();
                    print(result.map(value => value.rank).join(','));
                    print(result === source);
                }
            }
            "#,
            &[
                "1,2,3", "true", "1,2,3", "true", "1,2,3", "false", "1,2,3", "false",
            ],
        );
    }

    #[test]
    fn regression_array_copies_root_values_across_collecting_getters() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['with', 'toReversed', 'toSpliced']) {
                var trace = [];
                var source = [0, 1, 2];
                for (let index of [0, 1, 2]) {
                    Object.defineProperty(source, index, {get() {
                        trace.push(index);
                        $262.gc();
                        return {answer: 40 + index};
                    }});
                }
                var replacement = {answer: 99};
                var result = method === 'with' ? source.with(1, replacement)
                    : method === 'toSpliced' ? source.toSpliced(1, 1, replacement) : source.toReversed();
                print(result.map(value => value.answer).join(','));
                print(trace.join(','));
                print(Object.getPrototypeOf(result) === Array.prototype);
            }
            print(Array.prototype.with.call('abc', {valueOf() {$262.gc(); return 1;}}, 'z').join(','));
            print(Array.prototype.toSpliced.call('abc', {valueOf() {$262.gc(); return 1;}}, 1, 'z').join(','));
            "#,
            &[
                "40,99,42", "0,2", "true", "42,41,40", "2,1,0", "true", "40,99,42", "0,2", "true",
                "a,z,c", "a,z,c",
            ],
        );
    }

    #[test]
    fn regression_reduction_accumulators_survive_guest_collection() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['reduce', 'reduceRight']) {
                var source = [0, 1];
                Object.defineProperty(source, method === 'reduce' ? '1' : '0', {
                    get() { $262.gc(); return 1; }
                });
                print(source[method]((accumulator, value) => ({sum: accumulator.sum + 1}), {sum: 40}).sum);
                var seed = [0, 1];
                Object.defineProperty(seed, method === 'reduce' ? '0' : '1', {
                    get() { return {sum: 40}; }
                });
                Object.defineProperty(seed, method === 'reduce' ? '1' : '0', {
                    get() { $262.gc(); return {sum: 2}; }
                });
                print(seed[method]((accumulator, value) => ({sum: accumulator.sum + value.sum})).sum);
                var proxy = new Proxy([0, 1], {
                    has(target, key) { $262.gc(); return Reflect.has(target, key); }
                });
                print(proxy[method]((accumulator, value) => ({sum: accumulator.sum + 1}), {sum: 40}).sum);
                print(new Uint8Array([0, 1])[method]((accumulator, value) => {
                    $262.gc();
                    return {sum: accumulator.sum + 1};
                }, {sum: 40}).sum);
            }
            "#,
            &["42", "42", "42", "42", "42", "42", "42", "42"],
        );
    }

    #[test]
    fn regression_flattened_values_survive_collection() {
        assert_output_in_execution_modes(
            r#"
            var source = [0, 1];
            Object.defineProperty(source, '0', {get() { return [{answer: 42}]; }});
            Object.defineProperty(source, '1', {get() { $262.gc(); return [{answer: 43}]; }});
            print(source.flat().map(value => value.answer).join(','));
            print([0, 1].flatMap(value => {
                $262.gc();
                return [{answer: 44 + value}];
            }).map(value => value.answer).join(','));
            "#,
            &["42,43", "44,45"],
        );
    }

    #[test]
    fn regression_flattening_species_writes_follow_each_element() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['flat', 'flatMap']) {
                var trace = [];
                var source = [1, 2];
                source.constructor = {[Symbol.species]: function() {
                    return new Proxy({}, {defineProperty(target, key, descriptor) {
                        trace.push('write' + key);
                        Object.defineProperty(target, key, descriptor);
                        return true;
                    }});
                }};
                Object.defineProperty(source, '0', {get() { trace.push('get0'); return 1; }});
                Object.defineProperty(source, '1', {get() { trace.push('get1'); return 2; }});
                var result = method === 'flat' ? source.flat() : source.flatMap(value => {
                    trace.push('map' + value);
                    return [value];
                });
                print(trace.join(','));
                print(result[0] + ',' + result[1]);
            }
            "#,
            &[
                "get0,write0,get1,write1",
                "1,2",
                "get0,map1,write0,get1,map2,write1",
                "1,2",
            ],
        );
    }

    #[test]
    fn regression_flattening_stops_when_species_write_fails() {
        assert_output_in_execution_modes(
            r#"
            var trace = [];
            var source = [1, 2];
            source.constructor = {[Symbol.species]: function() {
                return new Proxy({}, {defineProperty() {trace.push('write'); return false;}});
            }};
            try {
                source.flatMap(value => {trace.push('map' + value); return [value];});
            } catch (error) { print(error instanceof TypeError); }
            print(trace.join(','));
            "#,
            &["true", "map1,write"],
        );
    }

    #[test]
    fn regression_bound_function_survives_collecting_metadata_getters() {
        assert_output_in_execution_modes(
            r#"
            function target(first, second) { return this.base + first.value + second; }
            Object.defineProperty(target, 'length', {
                configurable: true,
                get() { print('length'); $262.gc(); return 2; }
            });
            Object.defineProperty(target, 'name', {
                configurable: true,
                get() { print('name'); $262.gc(); return 'target'; }
            });
            var bound = target.bind({base: 40}, {value: 1});
            $262.gc();
            print(bound.name);
            print(bound.length);
            print(bound(1));
            var name = Object.getOwnPropertyDescriptor(bound, 'name');
            var length = Object.getOwnPropertyDescriptor(bound, 'length');
            print(name.writable + ',' + name.enumerable + ',' + name.configurable);
            print(length.writable + ',' + length.enumerable + ',' + length.configurable);
            for (var property of ['length', 'name']) {
                var marker = {};
                var victim = function() {};
                Object.defineProperty(victim, property, {
                    get() { $262.gc(); throw marker; }
                });
                try { victim.bind(null); }
                catch (error) { print(error === marker); }
            }
            "#,
            &[
                "length",
                "name",
                "bound target",
                "1",
                "42",
                "false,false,true",
                "false,false,true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_closure_identity_survives_collection() {
        assert_output_in_execution_modes(
            r#"
            function factory(value) {
                return function self() {
                    print(self === arguments.callee);
                    $262.gc();
                    return value;
                };
            }
            var first = factory(42);
            var second = factory(43);
            $262.gc();
            print(first());
            print(second());
            $262.gc();
            print(first());
            "#,
            &["true", "42", "true", "43", "true", "42"],
        );
    }

    #[test]
    fn regression_direct_eval_private_expressions_use_the_class_environment() {
        assert_output_in_execution_modes(
            r#"
            var Box = class {
                #value = 42;
                read() {
                    print(eval('this.#value'));
                    print(eval("'use strict'; this.#value"));
                    print(eval('this.#value += 1'));
                    var read = eval('() => this.#value');
                    $262.gc();
                    print(read());
                    print(eval('#value in this'));
                    try { eval('this.#missing'); }
                    catch (error) { print(error instanceof SyntaxError); }
                    try { (0, eval)('this.#value'); }
                    catch (error) { print(error instanceof SyntaxError); }
                    try { eval('({}).#value'); }
                    catch (error) { print(error instanceof TypeError); }
                }
            };
            new Box().read();
            "#,
            &["42", "42", "43", "43", "true", "true", "true", "true"],
        );
    }

    #[test]
    fn regression_strict_eval_retains_method_and_private_syntax_context() {
        assert_output_in_execution_modes(
            r#"
            var Derived = class {
                #value = 43;
                field = eval('() => super.value');
                read() {
                    print(this.field());
                    print(eval('super.value'));
                    try { eval('function inner() { return super.value; }'); }
                    catch (error) { print(error instanceof SyntaxError); }
                    try { eval('this.#missing'); }
                    catch (error) { print(error instanceof SyntaxError); }
                    try { eval('(() => function eval() {})'); }
                    catch (error) { print(error instanceof SyntaxError); }
                }
            };
            Object.setPrototypeOf(Derived.prototype, { get value() { return 42; } });
            new Derived().read();
            "#,
            &["42", "42", "true", "true", "true"],
        );
    }

    #[test]
    fn regression_parser_exhaustion_is_catchable_across_dynamic_entry_points() {
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    var intrinsicRangeError = RangeError;
                    RangeError = function() { throw 'replaced'; };
                    var parserStressDepth = 20000;
                    var source = '('.repeat(parserStressDepth) + '1' + ')'.repeat(parserStressDepth);
                    for (var mode = 0; mode < 3; mode++) {
                        try {
                            if (mode === 0) eval(source);
                            else if (mode === 1) (0, eval)(source);
                            else Function('return ' + source);
                            print('missing error');
                        } catch (error) {
                            print(error instanceof intrinsicRangeError);
                            print(error.message);
                        }
                        print(eval('1 + 2'));
                    }
                    "#,
                    &["true", crate::stack::STACK_EXHAUSTED_MESSAGE, "3",
                      "true", crate::stack::STACK_EXHAUSTED_MESSAGE, "3",
                      "true", crate::stack::STACK_EXHAUSTED_MESSAGE, "3"],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_regexp_parser_exhaustion_preserves_syntax_error_and_recovers() {
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    var intrinsicSyntaxError = SyntaxError;
                    var intrinsicRangeError = RangeError;
                    SyntaxError = function() { throw 'replaced'; };
                    RangeError = function() { throw 'replaced'; };
                    var regexpStressDepth = 20000;
                    var source = '['.repeat(regexpStressDepth) + 'a' + ']'.repeat(regexpStressDepth);
                    var receiver = /a/;
                    receiver.lastIndex = 7;
                    for (var mode = 0; mode < 4; mode++) {
                        try {
                            if (mode === 0) new RegExp(source, 'v');
                            else if (mode === 1) eval('/' + source + '/v');
                            else if (mode === 2) Function('return /' + source + '/v');
                            else receiver.compile(source, 'v');
                            print('missing error');
                        } catch (error) {
                            print(error instanceof (mode === 1 || mode === 2 ? intrinsicRangeError : intrinsicSyntaxError));
                            print(error.message.endsWith('Maximum call stack size exceeded'));
                        }
                        print(receiver.source);
                        print(receiver.lastIndex);
                        print(/a/.test('a'));
                    }
                    "#,
                    &[
                        "true", "true", "a", "7", "true",
                        "true", "true", "a", "7", "true",
                        "true", "true", "a", "7", "true",
                        "true", "true", "a", "7", "true",
                    ],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_dynamic_compiler_exhaustion_is_catchable_and_recovers() {
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    var intrinsicRangeError = RangeError;
                    RangeError = function() { throw 'replaced'; };
                    var source = Array(20000).fill('1').join('+');
                    for (var mode = 0; mode < 3; mode++) {
                        try {
                            if (mode === 0) eval(source);
                            else if (mode === 1) (0, eval)(source);
                            else Function('return ' + source);
                            print('missing error');
                        } catch (error) {
                            print(error instanceof intrinsicRangeError);
                            print(error.message);
                        }
                        print(eval('1 + 2'));
                    }
                    "#,
                    &[
                        "true",
                        crate::stack::STACK_EXHAUSTED_MESSAGE,
                        "3",
                        "true",
                        crate::stack::STACK_EXHAUSTED_MESSAGE,
                        "3",
                        "true",
                        crate::stack::STACK_EXHAUSTED_MESSAGE,
                        "3",
                    ],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_error_bootstrap_preserves_global_binding_order() {
        assert_output_in_execution_modes(
            "print(Object.getOwnPropertyNames(globalThis).filter(name => ['Object', 'Date', 'RangeError', 'RegExp'].includes(name)).join(','));",
            &["Object,Date,RangeError,RegExp"],
        );
    }

    #[test]
    fn regression_runtime_owners_share_recursion_budget() {
        let program = Engine::specialize("print(42);", "reentry.js").unwrap();
        let mut runtimes = [
            Runtime::new(Capture::default()),
            Runtime::new(Capture::default()),
        ];
        let mut guards = Vec::new();
        while let Ok(guard) = quench_stack::StackGuard::enter() {
            guards.push(guard);
        }
        let errors = runtimes
            .iter_mut()
            .map(|runtime| runtime.execute(&program).unwrap_err())
            .collect::<Vec<_>>();
        drop(guards);
        for (runtime, error) in runtimes.iter_mut().zip(errors) {
            assert!(error.thrown_value().is_some());
            assert_eq!(
                runtime.format_error(&program, &error),
                format!("RangeError: {}", quench_stack::STACK_EXHAUSTED_MESSAGE)
            );
            runtime.execute(&program).unwrap();
        }
    }

    #[test]
    fn regression_recursive_guest_transitions_throw_and_release_stack_budget() {
        std::thread::Builder::new()
            .name("recursion-regression".into())
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    function check(operation) {
                      try { operation(); print('returned'); }
                      catch (error) {
                        print(error instanceof RangeError);
                        print(error.message);
                      }
                      print((function () { return 42; })());
                    }
                    check(function () { function f() { f(); } f(); });
                    check(function () {
                      var object = { get value() { return this.value; } };
                      object.value;
                    });
                    check(function () {
                      var object = { set value(value) { this.value = value; } };
                      object.value = 1;
                    });
                    check(function () {
                      function C() { new C(); } new C();
                    });
                    check(function () {
                      var proxy = new Proxy({}, {
                        get: function (target, key, receiver) { return receiver[key]; }
                      });
                      proxy.value;
                    });
                    check(function () {
                      var object = { toString: function () { return String(this); } };
                      String(object);
                    });
                    check(function () {
                      var nesting = 10000;
                      JSON.parse('['.repeat(nesting) + '0' + ']'.repeat(nesting));
                    });
                    "#,
                    &[
                        "true",
                        "Maximum call stack size exceeded",
                        "42",
                        "true",
                        "Maximum call stack size exceeded",
                        "42",
                        "true",
                        "Maximum call stack size exceeded",
                        "42",
                        "true",
                        "Maximum call stack size exceeded",
                        "42",
                        "true",
                        "Maximum call stack size exceeded",
                        "42",
                        "true",
                        "Maximum call stack size exceeded",
                        "42",
                        "true",
                        "Maximum call stack size exceeded",
                        "42",
                    ],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_stack_errors_preserve_realm_intrinsics_through_collection() {
        std::thread::Builder::new()
            .name("recursion-realm-regression".into())
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    print($262.gc.name);
                    print($262.gc.length);
                    var retained = [{ answer: 42 }, /x/];
                    $262.gc();
                    print(retained[0].answer);
                    print(retained[1].test('x'));
                    var intrinsic = RangeError;
                    Object.defineProperty(globalThis, 'RangeError', {
                      get: function () { throw 'guest getter ran'; }, configurable: true
                    });
                    function f() { f(); }
                    try { f(); } catch (error) {
                      print(Object.getPrototypeOf(error) === intrinsic.prototype);
                      var descriptor = Object.getOwnPropertyDescriptor(error, 'message');
                      print(descriptor.value);
                      print(descriptor.writable && !descriptor.enumerable && descriptor.configurable);
                    }
                    var realm = $262.createRealm();
                    var foreignRangeError = realm.global.RangeError;
                    var foreign = realm.evalScript('(function f() { f(); })');
                    realm.evalScript('RangeError = undefined');
                    realm.gc();
                    try { foreign(); } catch (error) {
                      print(Object.getPrototypeOf(error) === foreignRangeError.prototype);
                      print(error.message);
                    }
                    "#,
                    &[
                        "gc", "0", "42", "true", "true",
                        "Maximum call stack size exceeded", "true", "true",
                        "Maximum call stack size exceeded",
                    ],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_class_and_eval_retain_original_source() {
        assert_output_in_execution_modes(
            r#"
print((class {}).toString());print(((class {})).toString());
var Named=class /* before name */ Named /* before body */ { /* inside */ };
print(Named.toString());
var Derived=class /* derived */ extends /* base */ Named { /* empty */ };
print(Function.prototype.toString.call(Derived));
class Declared { /* no constructor */ method(){return 1;} }
print(Declared.toString());print(Declared.prototype.method.toString());
print(eval('(class /* eval */ {})').toString());
print((class { constructor /* explicit */ () {} }).toString());
var Holder=class {static text=this.toString();};print(Holder.text);
var First=(class {});var text=First.toString();var Second=(class {x=1;});print(First.toString()===text);print(Second.toString());
var ctor=(class {});ctor.name='changed';print(ctor.toString());
"#,
            &[
                "class {}",
                "class {}",
                "class /* before name */ Named /* before body */ { /* inside */ }",
                "class /* derived */ extends /* base */ Named { /* empty */ }",
                "class Declared { /* no constructor */ method(){return 1;} }",
                "method(){return 1;}",
                "class /* eval */ {}",
                "class { constructor /* explicit */ () {} }",
                "class {static text=this.toString();}",
                "true",
                "class {x=1;}",
                "class {}",
            ],
        );
        assert_output_in_execution_modes(
            r#"
print(eval('1 /* between */ + 2'));
print((0,eval)('1 /* between */ + 2'));
print(eval('/* leading */ (function f(){ /* body */ return 1;})').toString());
print(eval('(() => /* arrow */ 2)').toString());
function local(){let x=9;return eval('/* scope */ x');} print(local());
try{eval("/* directive */ 'use strict'; with({}){}");}catch(e){print(e instanceof SyntaxError);}
print(eval("'/* text */'"));print(eval("/[/][*]/.test('/*')"));
print(eval('/* super( # arguments */ 1'));
print(eval('/* line\n++ comment */ 1'));
class C {x=eval('/* arguments super(\n++ */ 1');} print(new C().x);
try{eval('return 1;');}catch(e){print(e instanceof SyntaxError);}
try{eval('/* comment */ return 1;');}catch(e){print(e instanceof SyntaxError);}
"#,
            &[
                "3",
                "3",
                "function f(){ /* body */ return 1;}",
                "() => /* arrow */ 2",
                "9",
                "true",
                "/* text */",
                "true",
                "1",
                "1",
                "1",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_call_target_ast_preserves_source_and_abrupt_reference_order() {
        assert_output_in_execution_modes(
            r#"var index=0;var method={["m"+ ++index](){return 1;}};print(method.m1());print(index);
var effects=[];function target(){effects.push('call');return {valueOf(){effects.push('numeric');return 1;}};}
function rhs(){effects.push('rhs');return 1;}
function rejected(code){effects=[];try{eval(code);print('accepted');}catch(e){print(e instanceof ReferenceError);}print(effects.join(','));}
for(var code of ['target() = rhs()','target() += rhs()','++target()','--target()','target()++','target()--','(target()) = rhs()','target(target() = rhs()) = rhs()','`${target() = rhs()}`','for(target() in {a:1}){}','[method.result = (target() = rhs())] = [undefined]','({[target() = rhs()]: method.result} = {})']){rejected(code);}
var iterable={[Symbol.iterator](){effects.push('iterator');return {next(){$262.gc();effects.push('next');return {value:1,done:false};},return(){effects.push('close');return {done:true};}};}};
rejected('for(target() of iterable){}');
var getter={get fn(){effects.push('get');return function(){effects.push('call');return 1;};}};
rejected('++getter.fn()');
for(var code of ['target() ||= rhs()','target?.() = rhs()','[target()] = [1]','[target() = 1] = [undefined]','[...target()] = []','({a:target()} = {a:1})','({...target()} = {})']){
 try{Function(code);print('accepted');}catch(e){print(e instanceof SyntaxError);}
}
function original(){target()=rhs();}print(original.toString());
var key='\0oxc:call-assignment-target';var ordinary={};ordinary[key]=17;print(ordinary[key]);
try{new (class C{field=eval('target(arguments)=rhs()');})();}catch(e){print(e instanceof SyntaxError);}
"#,
            &[
                "1",
                "1",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "call",
                "true",
                "iterator,next,call,close",
                "true",
                "get,call",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "function original(){target()=rhs();}",
                "17",
                "true",
            ],
        );
    }

    #[test]
    fn regression_loop_targets_share_super_and_dynamic_name_references() {
        assert_output_in_execution_modes(
            r#"var seen=[];class Base{set field(value){seen.push(value);this.saved=value;}}
class Derived extends Base{run(){for(super.field of [11,13]){};for(super.field in {a:0}){};}}
var instance=new Derived();instance.run();print(seen.join(','));print(instance.saved==='a');print(Base.prototype.saved===undefined);
var outer='outer';var holder={outer:'holder'};with(holder){for(outer of ['changed']){}}print(outer);print(holder.outer);
var effects=[];var destination={};var key={toString(){effects.push('key');return 'saved';}};
var values={[Symbol.iterator](){effects.push('iterator');var done=false;return {next(){effects.push('next');if(done)return {done:true};done=true;return {done:false,value:17};}};}};
for(destination[key] of values){}print(effects.join(','));print(destination.saved);
"#,
            &[
                "11,13,a",
                "true",
                "true",
                "outer",
                "changed",
                "iterator,next,key,next",
                "17",
            ],
        );
    }

    #[test]
    fn regression_computed_method_updates_preserve_syntax_and_annex_b_targets() {
        assert_output_in_execution_modes(
            r#"var i=0;var first={["m"+ ++i](){return 1;}};print(first.m1());print(i);
var j=0;var second={["m"+j++](){return 2;}};print(second.m0());print(j);
var k=0;var third={[++k](){return 3;}};print(third[1]());
var n=0;class C{["m"+ ++n](){return 4;}}print(new C().m1());
var g=0;var getter={get ["m"+ ++g](){return 5;}};print(getter.m1);
var p=0;var setter={set ["m"+ ++p](value){this.saved=value;}};setter.m1=6;print(setter.saved);
var c=0;var closure={[++c](value=7){return ()=>value;}};print(closure[1]()());
print(first.m1.toString());
var calls=0;function target(){calls++;return 7;}
for(var code of ['target()=3','target()+=3','++target()','target()++']){
 try{eval(code);print('accepted');}catch(e){print(e instanceof ReferenceError);}
 print(calls);
}
for(var code of ['target() ||= 3','target?.()=3']){
 try{Function(code);print('accepted');}catch(e){print(e instanceof SyntaxError);}
}
"#,
            &[
                "1",
                "1",
                "2",
                "1",
                "3",
                "4",
                "5",
                "6",
                "7",
                "[\"m\"+ ++i](){return 1;}",
                "true",
                "1",
                "true",
                "2",
                "true",
                "3",
                "true",
                "4",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_compiled_field_eval_retains_binding_site_scope() {
        assert_output_in_execution_modes(
            r#"{
 class C {
  static field=eval("C");
  static read=eval("()=>C");
  static write=eval("()=>{try{C=1;}catch(e){return e instanceof TypeError;}}");
 }
 print(C.field===C);var Saved=C;C=null;$262.gc();print(Saved.read()===Saved);print(Saved.write());
}
{
 let C=class Inner {
  static field=eval("Inner");
  static read=eval("()=>Inner");
 };
 print(C.field===C);var Named=C;C=null;$262.gc();print(Named.read()===Named);
}
try{let C=class {static field=eval("C");};}catch(e){print(e instanceof ReferenceError);}
function make(value){let outer=value;class C {static read=eval("()=>[C,outer]");}return C;}
var first=make(7),second=make(9);$262.gc();print(first.read()[0]===first);print(first.read()[1]);print(second.read()[0]===second);print(second.read()[1]);
{
 class C {field=eval("C");read=eval("()=>C");}
 var instance=new C();var InstanceClass=C;C=null;$262.gc();print(instance.field===InstanceClass);print(instance.read()===InstanceClass);
}
"#,
            &[
                "true", "true", "true", "true", "true", "true", "true", "7", "true", "9", "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_field_context_survives_eval_arrows_and_stops_at_ordinary_functions() {
        assert_output_in_execution_modes(
            r#"class Base {get value(){return 5;}}
class Static {
 static direct=eval('()=>eval("arguments")');
 static nested=eval('()=>eval(`eval("arguments")`)');
 static normal=eval('(function(){return eval("arguments.length");})');
 static fromNormal=eval('(function(v){return ()=>eval("arguments[0]");})(13)');
 static receiver=eval('()=>eval("this")');
 static target=eval('()=>eval("new.target")');
}
class Instance extends Base {
 #hidden=21;
 direct=eval('()=>eval("arguments")');
 nested=eval('()=>eval(`eval("arguments")`)');
 normal=eval('(function(){return eval("arguments.length");})');
 fromNormal=eval('(function(v){return ()=>eval("arguments[0]");})(17)');
 privateRead=eval('()=>eval("this.#hidden")');
 superRead=eval('()=>eval("super.value")');
 receiver=eval('()=>eval("this")');
 target=eval('()=>eval("new.target")');
 invalidSuper=eval('()=>eval("super()")');
}
var instance=new Instance();$262.gc();
for(var read of [Static.direct,Static.nested,instance.direct,instance.nested]){
 try{read();print('accepted');}catch(e){print(e instanceof SyntaxError);}
}
print(Static.normal(1,2));print(Static.fromNormal());print(Static.receiver()===Static);print(Static.target()===undefined);
print(instance.normal(1,2,3));print(instance.fromNormal());print(instance.privateRead());print(instance.superRead());print(instance.receiver()===instance);print(instance.target()===undefined);
try{instance.invalidSuper();}catch(e){print(e instanceof SyntaxError);}
"#,
            &[
                "true", "true", "true", "true", "2", "13", "true", "true", "3", "17", "21", "5",
                "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_field_eval_retains_its_receiver_without_constructor_permissions() {
        assert_output_in_execution_modes(
            r#"class Holder {
 static f='test';static g=this.f+'262';static h=eval('this.g')+'test';
 static read=eval('()=>this.h');static target=eval('new.target');
 static property=eval('({arguments:1}).arguments');
 static ownArguments=eval('(function(){return arguments.length;})(1,2)');
 static comment=eval('/* arguments */ this.f');
 static strictThis=eval('(function(){return this===undefined;})()');
 field=eval('this');
}
print(Holder.property);print(Holder.ownArguments);print(Holder.comment);print(Holder.strictThis);
print(Holder.h);print(Holder.target===undefined);var item=new Holder();print(item.field===item);
$262.gc();Holder.h='changed';print(Holder.read());
var outer={label:9};function factory(){class Inner {static value=eval('this');}print(Inner.value===Inner);print(this===outer);}factory.call(outer);
class Base {} class Derived extends Base {constructor(){
 try{class Invalid {static value=eval('super()');}}catch(e){print(e instanceof SyntaxError);}
 try{class Escaped {static value=eval("\\u0061rguments");}}catch(e){print(e instanceof SyntaxError);}
 super();
}}new Derived();
"#,
            &[
                "1",
                "2",
                "test",
                "true",
                "test262test",
                "true",
                "true",
                "changed",
                "true",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_constructor_eval_uses_the_callers_super_and_this_binding() {
        assert_output_in_execution_modes(
            r#"var fields=0,bases=0;
class Base {constructor(v){bases++;this.v=v;}}
class Derived extends Base {
 field=++fields; #private=7;
 constructor(mode,v){
  print(eval('new.target')===Derived);
  try{eval('this');}catch(e){print(e instanceof ReferenceError);}
  if(mode===0){print(eval('super(v); this')===this);}
  else if(mode===1){(()=>eval('super(v)'))();}
  else if(mode===2){eval("eval('super(v)')");}
  else if(mode===3){eval('(()=>super(v))()');}
  else{return {init:eval('()=> {super(v); return this;}')};}
  print(eval('this.v'));print(eval('this.#private'));print(this.field);
 }
 test(){try{eval('super()');}catch(e){print(e instanceof SyntaxError);}}
}
var a=new Derived(0,4);new Derived(1,5);new Derived(2,6);new Derived(3,7);
var deferred=new Derived(4,8);$262.gc();var late=deferred.init();print(late.v);print(late.field);
try{deferred.init();}catch(e){print(e instanceof ReferenceError);}print(fields);print(bases);
a.test();
class Plain {constructor(){try{eval('super()');}catch(e){print(e instanceof SyntaxError);}}}new Plain();
class Invalid extends Base {constructor(){
 try{eval('function nested(){super();}');}catch(e){print(e instanceof SyntaxError);}
 try{(0,eval)('super()');}catch(e){print(e instanceof SyntaxError);}
 try{eval('return 1;');}catch(e){print(e instanceof SyntaxError);}
 super(9);
}}new Invalid();
"#,
            &[
                "true", "true", "true", "4", "7", "1", "true", "true", "5", "7", "2", "true",
                "true", "6", "7", "3", "true", "true", "7", "7", "4", "true", "true", "8", "5",
                "true", "5", "6", "true", "true", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_super_initializes_instance_elements_from_lexical_this_owner() {
        assert_output_in_execution_modes(
            r#"var count=0,events=[];
class Base {constructor(v){events.push('base');this.base=v;}}
class Derived extends Base {
 field=(events.push('field'),++count); #private=(events.push('private'),7);
 read=()=>this.#private;
 constructor(path){if(path===0){super(4);}else if(path===1){(()=>super(4))();}else{(()=>()=>super(4))()();}
 print(this.base);print(this.field);print(this.#private);print(events.join(','));events=[];}
}
var first=new Derived(0),second=new Derived(1),third=new Derived(2);
$262.gc();print(first.read());print(second.read());print(third.read());
class Deferred extends Base {field=++count; constructor(){var init=()=>super(5);return {init};}}
var deferred=new Deferred();$262.gc();var value=deferred.init();print(value.base);print(value.field);
try{deferred.init();}catch(e){print(e instanceof ReferenceError);}print(count);
var attempts=0;
class Throwing extends Base {field=(()=>{attempts++;throw 9;})();constructor(){
 try{(()=>super(8))();}catch(e){print(e);}print(this.base);
 try{super(9);}catch(e){print(e instanceof ReferenceError);}print(this.base);print(attempts);}}
new Throwing();
"#,
            &[
                "4",
                "1",
                "7",
                "base,field,private",
                "4",
                "2",
                "7",
                "base,field,private",
                "4",
                "3",
                "7",
                "base,field,private",
                "7",
                "7",
                "7",
                "5",
                "4",
                "true",
                "4",
                "9",
                "8",
                "true",
                "8",
                "1",
            ],
        );
    }

    #[test]
    fn regression_compiled_eval_retains_caller_method_context() {
        assert_output_in_execution_modes(
            r#"var events=[];
    class Base {
     method(v){events.push('method:'+this.tag);return this.count+v;}
     get value(){events.push('get:'+this.tag);return this.count;}
     set value(v){events.push('set:'+this.tag);this.count=v;}
    }
    class Derived extends Base {
     #hidden=11;
     constructor(){super();this.count=3;this.tag='D';}
     original(){return this.#hidden;}
     test(v){let local=2;
      print(eval('super.method(v+local)'));
      print(eval('super.value++; super.value'));
      print(eval('var retained=7; super.value + retained'));
      try{eval('retained');}catch(e){print(e instanceof ReferenceError);}
      print(eval('var n=this.#hidden; super.value+n'));
      print(eval('eval("super.value")'));
      print(eval('eval("this.#hidden")'));
      var Inner=eval('(class extends Derived {#hidden=22;read(){return this.#hidden;} })');
      var inner=new Inner();print(inner.read());print(inner.original());
      print(eval('(function f(){ /* exact */ return 1; })').toString());
      try{eval('return 1;');}catch(e){print(e instanceof SyntaxError);}
      return eval('(()=> /* retained */ super.value)');
     }
    }
    var instance=new Derived();var read=instance.test(4);print(read());print(read.toString());
    Object.setPrototypeOf(Derived.prototype,{get value(){return this.count+20;}});print(read());
    var home={get value(){return this.count;}};
    var object={count:2,__proto__:home,test(){
     print(eval('var leaked=5; super.value+leaked'));print(eval('leaked'));
     print(eval('with({x:3}){super.value+x}'));
     return eval('() => super.value');
    }};
    var objectRead=object.test();object.count=9;print(objectRead());
    try{(0,eval)('super.value');}catch(e){print(e instanceof SyntaxError);}
    "#,
            &[
                "9",
                "4",
                "11",
                "true",
                "15",
                "4",
                "11",
                "22",
                "11",
                "function f(){ /* exact */ return 1; }",
                "true",
                "4",
                "()=> /* retained */ super.value",
                "24",
                "7",
                "5",
                "5",
                "9",
                "true",
            ],
        );
    }

    #[test]
    fn regression_global_property_updates_preserve_binding_identity() {
        assert_output_in_execution_modes(
            r#"var visible=3;var alias=globalThis;alias.visible++;print(visible);globalThis.visible--;print(visible);
    let shadow=7;Object.defineProperty(globalThis,'shadow',{value:2,writable:true,configurable:true});print(globalThis.shadow++);print(shadow);print(globalThis.shadow);
    globalThis.shadow=6;print(shadow);print(globalThis.shadow);
    const fixed=9;Object.defineProperty(globalThis,'fixed',{value:4,writable:true,configurable:true});print(globalThis.fixed++);print(fixed);print(globalThis.fixed);
    function local(){let globalThis={field:1};let field=8;print(globalThis.field++);print(globalThis.field);print(field);}local();
    "#,
            &[
                "4", "3", "2", "7", "3", "7", "6", "4", "9", "5", "1", "2", "8",
            ],
        );
    }

    #[test]
    fn regression_super_calls_and_updates_share_receiver_references() {
        assert_output_in_execution_modes(
            r#"var events=[];
    class Base {
     get value(){events.push('get:'+this.tag);return this.count;}
     set value(v){events.push('set:'+this.tag);this.count=v;}
     get method(){events.push('method:'+this.tag);return function(v){events.push('call:'+this.tag);return this.count+v;};}
    }
    class Derived extends Base {
     constructor(){super();this.tag='D';this.count=3;}
     test(){
      print(super.value);print(super.method(4));print(super['method'](5));
      print(super.value++);print(++super['value']);print(this.count);
      print(events.join('|'));
     }
    }
    new Derived().test();
    var key=Symbol('key');
    class SymbolBase {get [key](){return this.count;}set [key](v){this.count=v;}}
    class SymbolDerived extends SymbolBase {constructor(){super();this.count=8n;}test(){print(super[key]++ === 8n);print(++super[key] === 10n);print(this.count === 10n);}}
    new SymbolDerived().test();
    class StaticBase {static get value(){return this.count;}}
    class StaticDerived extends StaticBase {static count=12;static test(){print(super.value);}}
    StaticDerived.test();
    var home={get value(){return this.count;},set value(v){this.count=v;}};
    var object={count:20,__proto__:home,test(){print(super.value++);print(this.count);}};object.test();
    "#,
            &[
                "3",
                "7",
                "8",
                "3",
                "5",
                "5",
                "get:D|method:D|call:D|method:D|call:D|get:D|set:D|get:D|set:D",
                "true",
                "true",
                "true",
                "12",
                "20",
                "21",
            ],
        );
    }

    #[test]
    fn regression_commented_eval_retains_super_home_and_arrow_source() {
        assert_output_in_execution_modes(
            r#"
    class B {get value(){return this.marker;}}
    class D extends B {
     constructor(){super();this.marker=42;}
     method(){
      print(eval('/* retained */ super.value'));
      print(eval('super /* between */ .value'));
      print(eval('/* binary */ super.value /* between */ + 1'));
      print(eval('super.value + 2'));
      print(eval('super[/* key */ "v\\u0061lue"]'));
      var arrow=eval('/* leading */ (() => /* retained */ super.value)');
      print(arrow());print(arrow.toString());return arrow;
     }
    }
    class Field extends B {
     x=(() => {try{return eval('/* context */ super.value + arguments');}catch(e){return e instanceof SyntaxError;}})();
    }
    print(new Field().x);
    var arrow=new D().method();
    Object.setPrototypeOf(D.prototype,{get value(){return this.marker+1;}});
    print(arrow());
    try{(0,eval)('/* context */ super.value');}catch(e){print(e instanceof SyntaxError);}
    try{eval('/* context */ super.value');}catch(e){print(e instanceof SyntaxError);}
    "#,
            &[
                "true",
                "42",
                "42",
                "43",
                "44",
                "42",
                "42",
                "() => /* retained */ super.value",
                "43",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_typed_source_copies_preserve_bits_and_content_type() {
        assert_output_in_execution_modes(
            r#"
var g=$262.createRealm().global;
for (var C of [Float32Array,g.Float32Array]) {
 var bits=new Uint32Array([0x7f800001,0x7fffffff,0xff800001,0xffffffff]);
 var src=new Float32Array(bits.buffer);var dst=new C(src);
 print(new Uint32Array(dst.buffer).join(','));print(dst.buffer !== src.buffer);
 var out=new C(4);out.set(src);print(new Uint32Array(out.buffer).join(','));
}
var bits=new Uint32Array([1,0x7ff00000,0xffffffff,0xfff7ffff]);var src=new Float64Array(bits.buffer);
print(new Uint32Array(new Float64Array(src).buffer).join(','));
var dst=new Float64Array(2);dst.set(src);print(new Uint32Array(dst.buffer).join(','));
var words=new Uint16Array([0x7c01,0x7fff,0xfc01,0xffff]);var src=new Float16Array(words.buffer);
print(new Uint16Array(new Float16Array(src).buffer).join(','));
var dst=new Float16Array(4);dst.set(src);print(new Uint16Array(dst.buffer).join(','));
var bits=new Uint32Array([0x7f800001,0x7fffffff,0xff800001,0xffffffff]);
var view=new Float32Array(bits.buffer);view.set(view.subarray(0,3),1);print(bits.join(','));
var src=new Uint8Array([1,2,3,4]);src.set(src.subarray(1));print(src.join(','));
var buf=new ArrayBuffer(16);new Uint8Array(buf).set([1,2,3,4]);var src=new Uint8Array(buf,0,4);
var dst=new Uint16Array(buf,0,4);dst.set(src);print(dst.join(','));
var src=new Uint16Array([17,18,19,20]);var dst=new Uint8Array(src.buffer,3,4);dst.set(src);print(dst.join(','));
var src=new BigInt64Array([-1n,2n]);print(new BigUint64Array(src).join(','));
var dst=new BigUint64Array(2);dst.set(src);print(dst.join(','));
for(var length of [0,1]){
 try{new Uint8Array(new BigInt64Array(length));}catch(e){print(e instanceof TypeError);}
 try{new BigInt64Array(new Uint8Array(length));}catch(e){print(e instanceof TypeError);}
 try{new Uint8Array(length).set(new BigInt64Array(length));}catch(e){print(e instanceof TypeError);}
 try{new BigInt64Array(length).set(new Uint8Array(length));}catch(e){print(e instanceof TypeError);}
}
try{new Uint8Array(0).set(new BigInt64Array(1));}catch(e){print(e instanceof RangeError);}
var shared=new SharedArrayBuffer(16);new Uint32Array(shared).set([0x7f800001,0x7fffffff,0xff800001,0xffffffff]);
var src=new Float32Array(shared);print(new Uint32Array(new Float32Array(src).buffer).join(','));
var dst=new Float32Array(4);dst.set(src);print(new Uint32Array(dst.buffer).join(','));
var src=new Float32Array(new Uint32Array([0x7f800001,0x7fffffff]).buffer), dst=new Float32Array(2);
dst.set(src,{valueOf(){$262.gc();new Uint32Array(src.buffer)[0]=0xff800001;return 0;}});print(new Uint32Array(dst.buffer).join(','));
"#,
            &[
                "2139095041,2147483647,4286578689,4294967295",
                "true",
                "2139095041,2147483647,4286578689,4294967295",
                "2139095041,2147483647,4286578689,4294967295",
                "true",
                "2139095041,2147483647,4286578689,4294967295",
                "1,2146435072,4294967295,4294443007",
                "1,2146435072,4294967295,4294443007",
                "31745,32767,64513,65535",
                "31745,32767,64513,65535",
                "2139095041,2139095041,2147483647,4286578689",
                "2,3,4,4",
                "1,2,3,4",
                "17,18,19,20",
                "18446744073709551615,2",
                "18446744073709551615,2",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "2139095041,2147483647,4286578689,4294967295",
                "2139095041,2147483647,4286578689,4294967295",
                "4286578689,2147483647",
            ],
        );
        assert_output_in_execution_modes(
            r#"
var src=new Float32Array(new Uint32Array([0x7f800001,0x7fffffff]).buffer.transferToImmutable());
print(new Uint32Array(new Float32Array(src).buffer).join(','));
var dst=new Float32Array(2);dst.set(src);print(new Uint32Array(dst.buffer).join(','));
"#,
            &["2139095041,2147483647", "2139095041,2147483647"],
        );
    }

    #[test]
    fn regression_typed_integrity_uses_indexed_and_named_descriptors() {
        assert_output_in_execution_modes(
            r#"
for(var op of ['seal','freeze']) for(var length of [0,1]) {
 var t=new Int32Array(length), symbol=Symbol();t.extra=1;t[symbol]=2;
 var result='ok';try{Object[op](t);}catch(e){result=e.name;}
 var d=Object.getOwnPropertyDescriptor(t,'extra'), s=Object.getOwnPropertyDescriptor(t,symbol);
 print([op,length,result,Object.isExtensible(t),Object.isSealed(t),Object.isFrozen(t),d.configurable,d.writable,s.configurable].join(','));
 if(length) print(Object.getOwnPropertyDescriptor(t,'0').configurable);
}
for(var op of ['seal','freeze']){
 var t=new Int32Array(new ArrayBuffer(0,{maxByteLength:8}));
 try{Object[op](t);}catch(e){print(e instanceof TypeError);}
 print(Object.isExtensible(t));print(Reflect.preventExtensions(t));
}
var t=new Int32Array(1);t.extra=3;$262.detachArrayBuffer(t.buffer);Object.freeze(t);
print(Object.isFrozen(t));print(Object.isSealed(t));print(Object.getOwnPropertyDescriptor(t,'extra').writable);
var t=new Int32Array(0), symbol=Symbol(); t[symbol]=4;Object.seal(t);
print(Object.getOwnPropertyDescriptor(t,symbol).configurable);print(Object.isFrozen(t));
Object.freeze(t);print(Object.getOwnPropertyDescriptor(t,symbol).writable);print(Object.isFrozen(t));
"#,
            &[
                "seal,0,ok,false,true,false,false,true,false",
                "seal,1,TypeError,false,false,false,true,true,true",
                "true",
                "freeze,0,ok,false,true,true,false,false,false",
                "freeze,1,TypeError,false,false,false,true,true,true",
                "true",
                "true",
                "true",
                "false",
                "true",
                "true",
                "false",
                "true",
                "true",
                "false",
                "false",
                "false",
                "false",
                "true",
            ],
        );
    }

    #[test]
    fn regression_typed_set_validates_after_offset_and_roots_sources() {
        assert_output_in_execution_modes(
            r#"
var target = new Int32Array(1); $262.detachArrayBuffer(target.buffer);
try { target.set(null, {valueOf(){throw 'offset';}}); } catch(e){print(e);}
try { target.set([], -1); } catch(e){print(e instanceof RangeError);}
try { target.set([], Infinity); } catch(e){print(e instanceof TypeError);}
var target = new Int32Array(1), order=[];
try { target.set({get length(){order.push('length');throw 'source';}}, Infinity); } catch(e){print(e);}
print(order.join(','));
var source = new Int32Array(0); $262.detachArrayBuffer(source.buffer);
try { new Int32Array(1).set(source, Infinity); } catch(e){print(e instanceof TypeError);}
var target = new Int32Array(2), order=[];
target.set({get length(){$262.detachArrayBuffer(target.buffer);return 2;}, get 0(){order.push('get0');return {valueOf(){order.push('convert0');return 1;}};}, get 1(){order.push('get1');return {valueOf(){order.push('convert1');return 2;}};}});
print(order.join(','));
var target = new Int32Array(1);
try {target.set(null, {valueOf(){$262.gc();return 0;}});} catch(e){print(e instanceof TypeError);}
var target = new Uint8Array(3);
target.set(Object.create({get length(){$262.gc();return 2;}, get 0(){$262.gc();return 7;}, get 1(){$262.gc();return 8;}}), {valueOf(){$262.gc();return 1;}});
print(target.join(','));
try {Uint8Array.prototype.set.call({}, [], {valueOf(){throw 'offset';}});} catch(e){print(e instanceof TypeError);}
var source = new Int32Array(1), target=new Int32Array(1);
try {target.set(source, {valueOf(){$262.detachArrayBuffer(source.buffer);return -1;}});} catch(e){print(e instanceof RangeError);}
var target = new Int32Array(1);
try {target.set({get length(){throw 'length';}}, {valueOf(){$262.detachArrayBuffer(target.buffer);return 0;}});} catch(e){print(e instanceof TypeError);}
var target = new Uint8Array(new ArrayBuffer(2).transferToImmutable()), order=[];
try {target.set({get length(){order.push('length');return 0;}}, {valueOf(){order.push('offset');return 0;}});} catch(e){print(e instanceof TypeError);}
print(order.length);
"#,
            &[
                "offset",
                "true",
                "true",
                "source",
                "length",
                "true",
                "get0,convert0,get1,convert1",
                "true",
                "0,7,8",
                "true",
                "true",
                "true",
                "true",
                "0",
            ],
        );
    }

    #[test]
    fn regression_typed_constructor_resolves_prototype_before_object_effects() {
        assert_output_in_execution_modes(
            r#"
for (var C of [Uint8Array, Int32Array, Float32Array, BigInt64Array]) {
    var order=[];
    var Target=(function(){}).bind(null);
    Object.defineProperty(Target,'prototype',{get(){order.push('prototype'); $262.gc(); return {tag:C.name};}});
    var arraylike={get length(){order.push('length'); $262.gc(); return 0;}};
    var view=Reflect.construct(C,[arraylike],Target);
    print(order.join(',')); print(Object.getPrototypeOf(view).tag === C.name);
}
var order=[];
var Target=(function(){}).bind(null);
Object.defineProperty(Target,'prototype',{get(){order.push('prototype'); throw 'prototype error';}});
var offset={valueOf(){order.push('offset');throw 'offset error';}};
try {Reflect.construct(Int32Array,[new ArrayBuffer(8),offset],Target);} catch(e) {print(e);}
print(order.join(','));
order=[];
try {Reflect.construct(Int32Array,[-1],Target);} catch(e) {print(e instanceof RangeError);}
print(order.join(','));
var buffer=new ArrayBuffer(8);
$262.detachArrayBuffer(buffer);
var length={valueOf(){throw 'length error';}};
try {new Int32Array(buffer,1,length);} catch(e) {print(e instanceof RangeError);}
try {new Int32Array(buffer,0,length);} catch(e) {print(e);}
try {new Int32Array(buffer,0,0);} catch(e) {print(e instanceof TypeError);}
var order=[];
var buffer=new ArrayBuffer(8);
var Target=(function(){}).bind(null);
Object.defineProperty(Target,'prototype',{get(){order.push('prototype');$262.detachArrayBuffer(buffer);return {};}});
var offset={valueOf(){order.push('offset');return 1;}};
var length={valueOf(){order.push('length');return 0;}};
try {Reflect.construct(Int32Array,[buffer,offset,length],Target);} catch(e) {print(e instanceof RangeError);}
print(order.join(','));
var g=$262.createRealm().global;
print(g.$262.global === g);
var detached = new g.ArrayBuffer(8); g.$262.detachArrayBuffer(detached); print(detached.byteLength);
g.eval('var Target = function Target() {}; Target.prototype=null;');
var home = g.Int32Array.prototype;
g.Int32Array = function(){throw 'mutable intrinsic';};
var result=Reflect.construct(Int32Array,[2],g.Target);
print(Object.getPrototypeOf(result) === home);
print(Object.getPrototypeOf(result.buffer) === ArrayBuffer.prototype);
class Derived extends Uint16Array {}
var derived=new Derived([1,2]);print(derived instanceof Derived);print(derived.join(','));
"#,
            &[
                "prototype,length",
                "true",
                "prototype,length",
                "true",
                "prototype,length",
                "true",
                "prototype,length",
                "true",
                "prototype error",
                "prototype",
                "true",
                "",
                "true",
                "length error",
                "true",
                "true",
                "prototype,offset",
                "true",
                "0",
                "true",
                "true",
                "true",
                "1,2",
            ],
        );
    }

    #[test]
    fn regression_typed_slice_preserves_bytes_and_live_overlap_order() {
        assert_output_in_execution_modes(
            r#"
var g = $262.createRealm().global;
for (var ctor of [Float32Array, g.Float32Array]) {
    var bits = new Uint32Array([0x7f800001, 0x7fffffff, 0xff800001, 0xffffffff]);
    var source = new ctor(bits.buffer);
    source.constructor = {[Symbol.species]: g.Float32Array};
    print(new Uint32Array(source.slice().buffer).join(','));
}
var bits = new Uint32Array([0x00000001, 0x7ff00000, 0xffffffff, 0xfff7ffff]);
var source = new Float64Array(bits.buffer);
print(new Uint32Array(source.slice().buffer).join(','));
var source = new Uint8Array([1,2,3,4]);
source.constructor = {[Symbol.species]: function(n) {return new Uint8Array(source.buffer, 1, n);}};
print(source.slice(0,3).join(',')); print(source.join(','));
var source = new Uint8Array([1,2,3,4]);
source.constructor = {[Symbol.species]: function(n) {return new Uint8Array(source.buffer, 0, n);}};
print(source.slice(1).join(',')); print(source.join(','));
var source = new Uint16Array([0,10,20,30]);
var destination = new Uint16Array([91,92,93,94,95]);
source = source.subarray(1);
source.constructor = {[Symbol.species]: function() {return new Uint16Array(destination.buffer,2,4);}};
print(source.slice(1).join(',')); print(destination.join(','));
var source = new Float32Array([NaN,1.5]);
source.constructor = {[Symbol.species]: Float64Array};
var result = source.slice(); print(result instanceof Float64Array); print(Number.isNaN(result[0])); print(result[1]);
var source = new Uint8Array([1,2,3,4]);
source.constructor = {[Symbol.species]: function(n) {$262.gc(); source[1]=9; return new Uint8Array(n);}};
print(source.slice(1).join(','));
var buffer = new ArrayBuffer(8, {maxByteLength:16});
var source = new Uint16Array(buffer); source.set([1,2,3,4]);
source.constructor = {[Symbol.species]: function(n) {buffer.resize(4); return new Uint16Array(n);}};
print(source.slice(1).join(','));
var shared = new SharedArrayBuffer(16); new Uint32Array(shared).set([0x7f800001,0x7fffffff,0xff800001,0xffffffff]);
print(new Uint32Array(new Float32Array(shared).slice().buffer).join(','));
"#,
            &[
                "2139095041,2147483647,4286578689,4294967295",
                "2139095041,2147483647,4286578689,4294967295",
                "1,2146435072,4294967295,4294443007",
                "1,1,1",
                "1,1,1,1",
                "2,3,4",
                "2,3,4,4",
                "20,30,94,95",
                "91,20,30,94,95",
                "true",
                "true",
                "1.5",
                "9,3,4",
                "2,0,0",
                "2139095041,2147483647,4286578689,4294967295",
            ],
        );
        assert_output_in_execution_modes(
            r#"
            var buffer = new Uint8Array([1, 2]).buffer.transferToImmutable();
            var source = new Uint8Array(buffer);
            source.constructor = {[Symbol.species]: function () {return source;}};
            for (var end of [0, 1]) {
                try {source.slice(0, end); print('returned');}
                catch (error) {print(error instanceof TypeError);}
            }
            print(source.join(','));
            "#,
            &["true", "true", "1,2"],
        );
    }

    #[test]
    fn regression_array_buffer_intrinsics_and_backing_stores_are_realm_owned() {
        assert_output_in_execution_modes(
            r#"
var g = $262.createRealm().global;
var A = g.ArrayBuffer;
var P = A.prototype;
print(P !== ArrayBuffer.prototype);
print(Object.getPrototypeOf(P) === g.Object.prototype);
print(Object.prototype.hasOwnProperty.call(P, 'slice'));
print(P.slice !== ArrayBuffer.prototype.slice);
print(Object.getPrototypeOf(P.slice) === g.Function.prototype);
print(A[Symbol.species] === A);
print(Object.getOwnPropertyDescriptor(A, 'prototype').writable);
print(Object.getOwnPropertyDescriptor(A, Symbol.species).get.name);
print(Object.getOwnPropertyDescriptor(P, 'byteLength').get !== Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get);
var a = new A(4);
new Uint8Array(a).set([1,2,3,4]);
print(a.slice(1).constructor === A);
print(new Uint8Array(a.slice(1)).join(','));
class ForeignSubclass extends A {}
print(new ForeignSubclass(3).slice().constructor === ForeignSubclass);
print(g.eval('new Uint8Array([5,6]).buffer.constructor === ArrayBuffer'));
print(new g.Uint8Array([5,6]).buffer.constructor === A);
print(new Uint8Array(new g.Uint8Array([5,6])).buffer.constructor === ArrayBuffer);
a.constructor = undefined;
print(Object.getPrototypeOf(ArrayBuffer.prototype.slice.call(a)) === ArrayBuffer.prototype);
print(Object.getPrototypeOf(P.slice.call(new ArrayBuffer(2))) === ArrayBuffer.prototype);
var local = new ArrayBuffer(2); local.constructor = undefined;
print(Object.getPrototypeOf(P.slice.call(local)) === P);
var home = ArrayBuffer.prototype;
var saved = ArrayBuffer;
g.ArrayBuffer = function () {throw new Error('mutable global observed');};
P.constructor = function () {throw new Error('mutable prototype constructor observed');};
print(Object.getPrototypeOf(P.slice.call(a)) === P);
a.constructor = {[Symbol.species]: null};
print(Object.getPrototypeOf(P.slice.call(a)) === P);
print(Object.getPrototypeOf(new g.Uint8Array(1).buffer) === P);
var speciesCalls=0;
a.constructor = {[Symbol.species]: function(n) {speciesCalls++; $262.gc(); return new A(n);}};
print(new Uint8Array(a.slice(1)).join(','));
print(speciesCalls);

print(Object.getPrototypeOf(new A(2).transfer()) === P);
print(Object.getPrototypeOf(ArrayBuffer.prototype.transfer.call(new A(2))) === ArrayBuffer.prototype);
var resizable = new A(2, {maxByteLength: 4}).transfer(3);
print(Object.getPrototypeOf(resizable) === P);
print(resizable.resizable);
print(resizable.maxByteLength);
print(Object.getPrototypeOf(new A(2).transferToFixedLength(3)) === P);
"#,
            &[
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "false",
                "get [Symbol.species]",
                "true",
                "true",
                "2,3,4",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "2,3,4",
                "1",
                "true",
                "true",
                "true",
                "true",
                "4",
                "true",
            ],
        );
    }

    #[test]
    fn regression_array_buffer_slice_rechecks_source_after_guest_effects() {
        assert_output_in_execution_modes(
            r#"
            for (var position of [0, 1]) {
              var buffer = new ArrayBuffer(8);
              var order = [];
              var start = { valueOf: function () {
                order.push('start');
                if (position === 0) { $262.detachArrayBuffer(buffer); $262.gc(); }
                return 1;
              } };
              var end = { valueOf: function () {
                order.push('end');
                if (position === 1) { $262.detachArrayBuffer(buffer); $262.gc(); }
                return 4;
              } };
              buffer.constructor = { [Symbol.species]: function (length) {
                order.push('species'); return new ArrayBuffer(length);
              } };
              try { buffer.slice(start, end); print('returned'); }
              catch (error) { print(error instanceof TypeError); }
              print(order.join(','));
              print(buffer.byteLength);
            }
            var buffer = new ArrayBuffer(3);
            var view = new Uint8Array(buffer);
            view.set([1, 2, 3]);
            buffer.constructor = { [Symbol.species]: function (length) {
              view[0] = 9; $262.gc(); return new ArrayBuffer(length);
            } };
            var start = { valueOf: function () { view[1] = 8; return 0; } };
            print(new Uint8Array(buffer.slice(start)).join(','));
            var buffer = new ArrayBuffer(4, { maxByteLength: 8 });
            new Uint8Array(buffer).set([1, 2, 3, 4]);
            var start = { valueOf: function () { buffer.resize(2); return 0; } };
            print(new Uint8Array(buffer.slice(start)).join(','));
            "#,
            &[
                "true",
                "start,end,species",
                "0",
                "true",
                "start,end,species",
                "0",
                "9,8,3",
                "1,2,0,0",
            ],
        );
    }

    #[test]
    fn script_requests_use_the_validated_runtime_boundary() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script("print(40 + 2);", "boundary.js"))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["42"]);
    }

    #[test]
    fn module_evaluation_query_follows_execution_gc_and_reinitialization() {
        let mut runtime = Runtime::new(Capture::default());
        let pending =
            Engine::specialize_module("await new Promise(() => {});", "pending.mjs").unwrap();
        assert!(runtime.module_evaluation_pending(&pending).is_err());
        runtime.execute(&pending).unwrap();
        assert!(runtime.module_evaluation_pending(&pending).unwrap());
        runtime.collect(&pending).unwrap();
        runtime.run_jobs(&pending).unwrap();
        assert!(runtime.module_evaluation_pending(&pending).unwrap());

        let settled = Engine::specialize_module("await Promise.resolve();", "settled.mjs").unwrap();
        runtime.execute(&settled).unwrap();
        assert!(!runtime.module_evaluation_pending(&settled).unwrap());
        assert!(runtime.module_evaluation_pending(&pending).is_err());

        let script = Engine::specialize("0;", "script.js").unwrap();
        runtime.execute(&script).unwrap();
        assert!(!runtime.module_evaluation_pending(&script).unwrap());
        assert!(runtime.module_evaluation_pending(&settled).is_err());
    }

    #[test]
    fn generic_reference_matches_specialized_output() {
        let source = "var proto = { answer: 41 }; var other = Object.create({ answer: 1 }); var object = Object.create(proto); object.add = function(value) { return value + 1; }; function read(value) { return value.answer; } print(read(object) + read(other) + object.add(1));";
        let optimized_host = Capture::default();
        let optimized_view = optimized_host.clone();
        let mut optimized = Runtime::new(optimized_host);
        let optimized_program = Engine::specialize(source, "optimized.js").unwrap();
        assert!(optimized_program.specialized);
        assert!(optimized_program.cache_sites > 0);
        assert!(optimized_program.functions.iter().any(|function| {
            function
                .code
                .iter()
                .any(|instruction| instruction.op() == crate::bytecode::Op::GetField)
        }));
        optimized.execute(&optimized_program).unwrap();

        let generic_host = Capture::default();
        let generic_view = generic_host.clone();
        let mut generic = Runtime::new(generic_host);
        let generic_program = Engine::specialize_unspecialized(source, "generic.js").unwrap();
        assert!(!generic_program.specialized);
        generic.execute(&generic_program).unwrap();

        assert_eq!(
            optimized_view.0.borrow().as_slice(),
            generic_view.0.borrow().as_slice()
        );
    }

    #[test]
    fn wide_instruction_side_table_executes_through_the_general_core() {
        let source = format!("print([{}].length);", "0,".repeat(4097));
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        let program = Engine::specialize(&source, "wide.js").unwrap();
        assert!(
            program
                .functions
                .iter()
                .any(|function| !function.wide.is_empty())
        );
        let path = std::env::temp_dir().join(format!("quench-wide-{}", std::process::id()));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(
            decoded
                .functions
                .iter()
                .any(|function| !function.wide.is_empty())
        );
        runtime.execute(&decoded).unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["4097"]);
    }

    #[test]
    fn oxc_lone_surrogate_escapes_retain_utf16_units() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        let program = Engine::compile(ExecutionRequest::script(
            r#"print("\uD800".length); print("\uD800".charCodeAt(0));"#,
            "surrogate.js",
        ))
        .unwrap();
        let path = std::env::temp_dir().join(format!("quench-surrogate-{}", std::process::id()));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        runtime.execute(&decoded).unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["1", "55296"]);
    }

    #[test]
    fn finalization_registry_drains_collected_targets_after_their_root_is_cleared() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var registry = new FinalizationRegistry(function(held) { print(held); }); var target = {}; registry.register(target, 7); target = undefined; for (var i = 0; i < 10000; i = i + 1) { var temporary = {}; } print(\"done\");",
                "finalization-drain.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["done", "7"]);
    }

    #[test]
    fn stale_root_cannot_enqueue_a_job() {
        let mut runtime = Runtime::new(Capture::default());
        let root = runtime.root(Value::UNDEFINED);
        assert!(runtime.release_root(root));
        assert!(!runtime.enqueue_rooted_job(root, &[]));
    }

    #[test]
    fn root_handles_cannot_cross_runtime_boundaries() {
        let mut owner = Runtime::new(Capture::default());
        let mut other = Runtime::new(Capture::default());
        let owned_root = owner.root(Value::number(1.0));
        let other_root = other.root(Value::number(2.0));

        assert!(!other.root_is_live(owned_root));
        assert!(!other.update_root(owned_root, Value::number(3.0)));
        assert!(!other.release_root(owned_root));
        assert!(!other.enqueue_rooted_job(owned_root, &[]));
        assert!(other.root_is_live(other_root));
    }

    #[test]
    fn explicit_collection_preserves_runtime_roots() {
        let mut runtime = Runtime::new(Capture::default());
        let program = Engine::specialize("print(0);", "collect.js").unwrap();
        runtime.execute(&program).unwrap();
        let root = runtime.root(Value::number(42.0));
        runtime.collect(&program).unwrap();
        assert!(runtime.root_is_live(root));
        assert!(runtime.release_root(root));
    }

    #[test]
    fn rooted_execution_completion_survives_collection() {
        let mut runtime = Runtime::new(Capture::default());
        let program = Engine::specialize("print('ran');", "rooted-result.js").unwrap();
        let completion = runtime.execute_rooted(&program).unwrap();

        assert!(runtime.root_is_live(completion));
        runtime.collect(&program).unwrap();
        assert!(runtime.root_is_live(completion));
        assert!(runtime.release_root(completion));
        assert!(!runtime.root_is_live(completion));
    }

    #[test]
    fn regression_eval_defaults_capture_caller_and_infer_function_names() {
        assert_output_in_execution_modes(
            r#"
            function caller() {
                var offset = 39;
                function value() { return offset + 3; }
                eval('var f = function named([a = value(), ...rest]) { return a; };');
                print(f([]));
                print(f.name);
                eval('var inferred = function() {};');
                print(inferred.name);
            }
            caller();
            "#,
            &["42", "named", "inferred"],
        );
    }

    #[test]
    fn regression_eval_global_bindings_follow_property_descriptors() {
        assert_output_in_execution_modes(
            r#"
            var initial;
            var x = 23;
            eval('initial = x; var x = 45;');
            print(initial);
            print(x);
            var descriptor = Object.getOwnPropertyDescriptor(this, 'x');
            print(descriptor.value);
            print(descriptor.writable);
            print(descriptor.enumerable);
            print(descriptor.configurable);
            var reads = 0;
            var writes = 0;
            eval('var visible = 1; Object.defineProperty(this, "visible", {get: function() { reads++; return 71; }, set: function(v) { writes = v; }}); print(visible); visible = 12; print(writes); print(visible);');
            print(reads);
            var frozen = 2;
            Object.defineProperty(this, 'frozen', {writable: false});
            frozen = 5;
            print(frozen);
            print(eval('frozen'));
            try { eval('"use strict"; frozen = 7;'); }
            catch (error) { print(error.name); }
            "#,
            &[
                "23",
                "45",
                "45",
                "true",
                "true",
                "false",
                "71",
                "12",
                "71",
                "2",
                "2",
                "2",
                "TypeError",
            ],
        );
    }

    #[test]
    fn regression_eval_property_sites_use_the_executing_program() {
        assert_output_in_execution_modes(
            r#"
            eval('var object = {a: 2, b: 3, c: 4}; object.a = object.b + object.c; print(object.a); print(object.b); print(object.c);');
            "#,
            &["7", "3", "4"],
        );
    }

    #[test]
    fn regression_regexp_literals_and_species_use_realm_intrinsics() {
        assert_output_in_execution_modes(
            r#"
            var original = RegExp;
            RegExp = null;
            var expression = /a/g;
            print(Object.getPrototypeOf(expression) === original.prototype);
            print(original(expression) === expression);
            expression.constructor = undefined;
            print(expression[Symbol.matchAll]('a').next().value[0]);
            print(expression[Symbol.split]('ba')[0]);
            function shadowed(RegExp) { return /b/.test('b'); }
            print(shadowed(null));
            "#,
            &["true", "true", "a", "b", "true"],
        );
    }

    #[test]
    fn regression_foreign_regexp_realm_has_intrinsic_methods_and_descriptors() {
        assert_output_in_execution_modes(
            r#"
            var other = $262.createRealm().global;
            other.eval('var intrinsic = RegExp; RegExp = null; var expression = /a/g;');
            print(Object.getPrototypeOf(other.expression) === other.intrinsic.prototype);
            print(other.expression.toString());
            print(other.expression.test('a'));
            var descriptor = Object.getOwnPropertyDescriptor(other.intrinsic, 'prototype');
            print(descriptor.writable);
            print(descriptor.enumerable);
            print(descriptor.configurable);
            "#,
            &["true", "/a/g", "true", "false", "false", "false"],
        );
    }

    #[test]
    fn regression_regexp_constructor_uses_internal_source_and_flags() {
        assert_output_in_execution_modes(
            r#"
            var expression = /a/g;
            var gets = 0;
            Object.defineProperty(expression, 'source', {get: function() { gets++; return 'wrong'; }});
            Object.defineProperty(expression, 'flags', {get: function() { gets++; return 'i'; }});
            var cloned = new RegExp(expression);
            print(cloned.source);
            print(cloned.flags);
            print(gets);
            "#,
            &["a", "g", "0"],
        );
    }

    #[test]
    fn regression_regexp_constructor_orders_getters_prototype_and_coercions() {
        assert_output_in_execution_modes(
            r#"
            var events = [];
            var like = {};
            Object.defineProperty(like, Symbol.match, {get: function() { events.push('match'); return true; }});
            Object.defineProperty(like, 'source', {get: function() { events.push('source'); return {toString: function() { events.push('source coercion'); return 'a'; }}; }});
            Object.defineProperty(like, 'flags', {get: function() { events.push('flags'); return {toString: function() { events.push('flags coercion'); return 'g'; }}; }});
            var target = new Proxy(function() {}, {get: function(t, key) { if (key === 'prototype') events.push('prototype'); return Reflect.get(t, key); }});
            var result = Reflect.construct(RegExp, [like], target);
            print(events.join(','));
            print(result.source);
            print(result.flags);
            "#,
            &[
                "match,source,flags,prototype,source coercion,flags coercion",
                "undefined",
                "undefined",
            ],
        );
    }

    #[test]
    fn regression_legacy_braced_unicode_uses_identity_escape_and_quantifier() {
        assert_output_in_execution_modes(
            r#"
            print(/\u{3}/.exec('Auuu')[0]);
            print(/\u{41}/u.exec('Auuu')[0]);
            print(/\u{3}/.test('uuu'));
            print(/\u{3}/.test('A'));
            print(/\u{4A}/.exec('u{4A}')[0]);
            "#,
            &["uuu", "A", "true", "false", "u{4A}"],
        );
    }

    #[test]
    fn regression_regexp_source_escapes_line_terminators_and_preserves_classes() {
        assert_output_in_execution_modes(
            r#"
            var rows = [
                ['\n', '\\n'], ['\\\n', '\\n'], ['\\\\\n', '\\\\\\n'],
                ['\r', '\\r'], ['\\\r', '\\r'],
                ['\u2028', '\\u2028'], ['\\\u2028', '\\u2028'],
                ['\u2029', '\\u2029'], ['\\\u2029', '\\u2029'],
                ['/', '\\/'], ['[/]', '[/]'], ['[\\/]', '[\\/]'],
                ['\\[/\\]', '\\[\\/\\]']
            ];
            var passed = 0;
            for (var row of rows) {
                var expression = new RegExp(row[0]);
                if (expression.source === row[1]) passed++;
                if (eval('/' + expression.source + '/').source === row[1]) passed++;
                if (expression.toString() === '/' + row[1] + '/') passed++;
            }
            print(passed);
            "#,
            &["39"],
        );
    }

    #[test]
    fn regression_regexp_preserves_raw_utf16_patterns_across_entry_points() {
        assert_output_in_execution_modes(
            r#"
            var lone = '\uD83D';
            var pair = '\uD83D\uDC38';
            print(new RegExp(lone, 'u').exec(lone)[0].charCodeAt(0));
            print(new RegExp(lone, 'u').source.charCodeAt(0));
            print(eval('/' + lone + '/u').exec(lone)[0].charCodeAt(0));
            print(eval('/[' + pair + ']/').exec(pair)[0].charCodeAt(0));
            print(eval('/[' + pair + ']/u').exec(pair)[0].length);
            print(eval('/' + pair + '?/').exec('') === null);
            print(new RegExp(pair + '?').exec('') === null);
            print(new RegExp('\\uD83D\uDC38', 'u').test(pair));
            print(new RegExp('\uD83D\\uDC38', 'u').test(pair));
            print(new RegExp('\\uD83D\uDC38').test(pair));
            print(new RegExp('\uD83D\\uDC38').test(pair));
            print(new RegExp('\\' + lone).test(lone));
            try { new RegExp('\\' + lone, 'u'); }
            catch (error) { print(error.name); }
            var expression = /a/;
            expression.compile('[' + lone + ']', 'u');
            print(expression.exec(lone)[0].charCodeAt(0));
            print(new RegExp(expression).exec(lone)[0].charCodeAt(0));
            "#,
            &[
                "55357",
                "55357",
                "55357",
                "55357",
                "2",
                "true",
                "true",
                "false",
                "false",
                "true",
                "true",
                "true",
                "SyntaxError",
                "55357",
                "55357",
            ],
        );
    }

    #[test]
    fn regression_regexp_control_letters_work_in_classes_and_ranges() {
        assert_output_in_execution_modes(
            r#"
            print(/[\cA]/u.test('\u0001'));
            print(/[\cz]/u.exec('\u001A')[0].charCodeAt(0));
            print(/[\cA-\cZ]/u.test('\u0007'));
            print(/[\cA-\cZ]/u.test('A'));
            print(/[\cA]/v.test('\u0001'));
            print(/[\cA]/.test('\u0001'));
            print(/[\\cA]/u.test('c'));
            "#,
            &["true", "26", "true", "false", "true", "true", "true"],
        );
    }

    #[test]
    fn regression_eval_regexp_class_delimiters_share_literal_scanning() {
        assert_output_in_execution_modes(
            r#"
            print(eval(' /[/]/ ').source);
            print(eval(' /[/]/ ').test('/'));
            print(eval('(/[/]/)').source);
            "#,
            &["[/]", "true", "[/]"],
        );
    }

    #[test]
    fn regression_eval_name_reads_do_not_borrow_bytecode_cache_sites() {
        assert_output_in_execution_modes(
            r#"
            print(eval('(42)'));
            print(eval('({answer: 42})').answer);
            var expression = eval('(/a/)');
            print(expression.source);
            print(expression.test('a'));
            var visible = 7;
            print(eval('visible'));
            "#,
            &["42", "42", "a", "true", "7"],
        );
    }

    #[test]
    fn regression_intl_german_date_format_ignores_string_split_override() {
        assert_output_in_execution_modes(
            r#"
            var possibleAnswers = ["1.1.1970", "2.1.1970", "3.1.1970"];
            var replacements = ["", "x-foo", "de-u-co", "en-US"];
            for (var index = 0; index < replacements.length; index++) {
                String.prototype[Symbol.split] = function() { return [replacements[index]]; };
                var formatted = Intl.DateTimeFormat("de", {}).format(86400000);
                print(formatted);
                print(possibleAnswers.includes(formatted));
            }
            "#,
            &[
                "1.1.1970", "true", "1.1.1970", "true", "1.1.1970", "true", "1.1.1970", "true",
            ],
        );
    }

    #[test]
    fn regression_arguments_binding_survives_closure_and_direct_eval() {
        assert_output_in_execution_modes(
            r#"
            function countArguments(a, b, c, d) {
                return (() => arguments.length)();
            }
            function evalArgumentsLength() {
                eval("arguments.length = 42");
                return arguments.length;
            }
            print(countArguments());
            print(countArguments(1, 2, 3));
            print(countArguments(1, 2, 3, 4));
            print(evalArgumentsLength());
            "#,
            &["0", "3", "4", "42"],
        );
    }
}

#[cfg(test)]
#[path = "api_promise_tests.rs"]
mod promise_tests;
