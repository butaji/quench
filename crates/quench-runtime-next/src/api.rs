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
        let source = "var object = { answer: 41 }; print(object['answer'] + 1);";
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
}
