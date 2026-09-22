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
            SourceKind::Module => Err(vec![Diagnostic::unsupported(
                request.name,
                "module compilation is not available in the staged v2 subset",
            )]),
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

        fn clock_millis(&mut self) -> f64 {
            0.0
        }
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
}
