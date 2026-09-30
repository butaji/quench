use crate::{Diagnostic, Engine, Host, JsError, ResidualProgram, RootId, Value, Vm};

/// The syntax context used when compiling source.  The v2 compiler currently
/// accepts the Script subset; the other contexts are explicit so callers do
/// not accidentally treat module/eval source as an ordinary script.
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

    pub fn execute(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        program.validate().map_err(JsError::validation)?;
        self.vm.execute(program)
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

    pub fn root_value(&self, root: RootId) -> Option<Value> {
        self.vm.root_value(root)
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

    pub fn run_jobs(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        program.validate().map_err(JsError::validation)?;
        self.vm.drain_jobs(program)
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
    ) -> Result<Value, RuntimeError> {
        let program = Engine::compile(request).map_err(RuntimeError::Diagnostics)?;
        self.execute(&program).map_err(RuntimeError::Execution)
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
                panic!("{mode}: {}", runtime.format_error(&program, &error));
            }
            assert_eq!(view.0.borrow().as_slice(), expected, "{mode}");
        }
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
            assert_eq!(runtime.format_error(&program, &error),
                format!("RangeError: {}", quench_stack::STACK_EXHAUSTED_MESSAGE));
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
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
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
            &["true", "start,end,species", "0", "true", "start,end,species", "0", "9,8,3", "1,2,0,0"],
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
    fn non_script_requests_are_rejected_before_execution() {
        let mut runtime = Runtime::new(Capture::default());
        let error = runtime
            .compile_and_execute(ExecutionRequest {
                source: "export default 1;",
                name: "module.mjs",
                kind: SourceKind::Module,
            })
            .unwrap_err();
        assert!(format!("{error:?}").contains("module compilation"));
    }

    #[test]
    fn arrow_functions_compile_as_residual_closures() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var add = (x) => x + 1; print(add(41));",
                "arrow.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["42"]);
    }

    #[test]
    fn generic_reference_matches_specialized_output() {
        let source = "var proto = { answer: 41 }; var other = Object.create({ answer: 1 }); var object = Object.create(proto); object.add = function(value) { return value + 1; }; function read(value) { return value.answer; } print(read(object) + read(other) + object.add(1));";
        let optimized_host = Capture::default();
        let optimized_view = optimized_host.clone();
        let mut optimized = Runtime::new(optimized_host);
        optimized
            .execute(&Engine::specialize(source, "optimized.js").unwrap())
            .unwrap();

        let generic_host = Capture::default();
        let generic_view = generic_host.clone();
        let mut generic = Runtime::new(generic_host);
        generic
            .execute(&Engine::specialize_unspecialized(source, "generic.js").unwrap())
            .unwrap();

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
        let path = std::env::temp_dir().join(format!("rqj-wide-{}", std::process::id()));
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
        let path = std::env::temp_dir().join(format!("rqj-surrogate-{}", std::process::id()));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        runtime.execute(&decoded).unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["1", "55296"]);
    }

    #[test]
    fn oxc_surrogate_property_keys_use_exact_index_semantics() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                r#"var object = {"\uD800": 1}; print(Object.keys(object)[0].charCodeAt(0)); print(object["\uD800"]);"#,
                "surrogate-key.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["55296", "1"]);
    }

    #[test]
    fn oxc_surrogate_keys_cover_class_fields_and_destructuring() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                r#"class Box { "\uD801" = 7; } print(new Box()["\uD801"]); var {"\uD800": value} = {"\uD800": 3}; print(value);"#,
                "surrogate-class-destructure.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["7", "3"]);
    }

    #[test]
    fn finalization_registry_registers_and_unregisters_generation_checked_tokens() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var registry = new FinalizationRegistry(function() {}); var target = {}; var token = {}; registry.register(target, 1, token); print(registry.unregister(token)); print(registry.unregister(token));",
                "finalization.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["true", "false"]);
    }

    #[test]
    fn finalization_registry_uses_generic_property_traversal() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        let program = Engine::specialize_unspecialized(
            "var registry = new FinalizationRegistry(function() {}); var token = {}; registry.register({}, 1, token); print(typeof registry.unregister); print(registry.unregister(token));",
            "finalization-generic.js",
        )
        .unwrap();
        runtime.execute(&program).unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["function", "true"]);
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
    fn prototype_mutation_rejects_cycles_and_invalidates_property_caches() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var first = { value: 1 }; var second = Object.create(first); print(second.value); var third = { value: 3 }; Object.setPrototypeOf(second, third); print(second.value); try { Object.setPrototypeOf(third, second); print(\"not-rejected\"); } catch (error) { print(\"cycle\"); }",
                "prototype-cycle.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["1", "3", "cycle"]);
    }

    #[test]
    fn stale_root_cannot_enqueue_a_job() {
        let mut runtime = Runtime::new(Capture::default());
        let root = runtime.root(Value::UNDEFINED);
        assert!(runtime.release_root(root));
        assert!(!runtime.enqueue_rooted_job(root, &[]));
    }

    #[test]
    fn descriptor_transitions_reject_mixed_fields_and_allow_configurable_kind_changes() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var object = {}; try { Object.defineProperty(object, 'mixed', { value: 1, get: function() { return 2; } }); print('bad'); } catch (error) { print('mixed'); } Object.defineProperty(object, 'value', { get: function() { return 3; }, configurable: true }); Object.defineProperty(object, 'value', { value: 4 }); print(object.value); var array = []; Object.defineProperty(array, '0', { get: function() { return 5; }, configurable: true }); Object.defineProperty(array, '0', { value: 6 }); print(array[0]);",
                "descriptor-transition.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["mixed", "4", "6"]);
    }

    #[test]
    fn define_properties_uses_one_snapshot_of_descriptor_keys() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var target = {}; var descriptors = { a: { value: 1 }, b: { value: 2, enumerable: true } }; Object.defineProperties(target, descriptors); print(target.a); print(target.b); print(Object.keys(target).join(',')); var created = Object.create(null, { x: { value: 9, enumerable: true } }); print(created.x); print(Object.keys(created).join(','));",
                "define-properties.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["1", "2", "b", "9", "x"]);
    }

    #[test]
    fn explicit_collection_preserves_runtime_roots() {
        let mut runtime = Runtime::new(Capture::default());
        let program = Engine::specialize("print(0);", "collect.js").unwrap();
        runtime.execute(&program).unwrap();
        let root = runtime.root(Value::number(42.0));
        runtime.collect(&program).unwrap();
        assert_eq!(runtime.root_value(root), Some(Value::number(42.0)));
        assert!(runtime.release_root(root));
    }

    #[test]
    fn array_length_descriptor_is_an_own_non_enumerable_property() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var descriptor = Object.getOwnPropertyDescriptor([1, 2], 'length'); print(descriptor.value); print(descriptor.enumerable); print(descriptor.configurable);",
                "array-length-descriptor.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["2", "false", "false"]);
    }

    #[test]
    fn array_length_is_visible_only_to_non_enumerable_own_key_views() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var array = [1, 2]; print(Object.keys(array).join(',')); print(Object.getOwnPropertyNames(array).join(',')); print(Reflect.ownKeys(array).join(','));",
                "array-own-keys.js",
            ))
            .unwrap();
        assert_eq!(
            view.0.borrow().as_slice(),
            ["0,1", "0,1,length", "0,1,length"]
        );
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
                "1.1.1970",
                "true",
                "1.1.1970",
                "true",
                "1.1.1970",
                "true",
                "1.1.1970",
                "true",
            ],
        );
    }

}

#[cfg(test)]
#[path = "api_promise_tests.rs"]
mod promise_tests;

#[cfg(test)]
#[path = "api_iterator_tests.rs"]
mod iterator_tests;
