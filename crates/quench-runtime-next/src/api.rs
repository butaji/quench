use crate::{Diagnostic, Engine, Host, JsError, ResidualProgram, Value, Vm};

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
