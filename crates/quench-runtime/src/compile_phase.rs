//! The phases a compilation passes, and the hook a profiler sets to observe them. The phases are
//! a property of compiling a text anywhere (an image's build, a host's cache), so the runtime
//! names them without naming the compiler.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CompilePhase {
    /// A compilation has parsed its source; its semantic analysis starts.
    Analysis,
    /// A compilation has parsed and analysed its source; its code generation starts.
    CodeGeneration,
    /// A compiled program is complete and its encoding for the host's compile cache starts.
    Encoding,
}

static COMPILE_PHASE_BOUNDARY: OnceLock<fn(CompilePhase)> = OnceLock::new();

/// Sets the hook a compilation calls at each phase boundary (a profile lecture).
pub fn set_compile_phase_boundary(action: fn(CompilePhase)) {
    let _ = COMPILE_PHASE_BOUNDARY.set(action);
}

pub(crate) fn compile_phase_boundary(phase: CompilePhase) {
    if let Some(action) = COMPILE_PHASE_BOUNDARY.get() {
        action(phase);
    }
}
