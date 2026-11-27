//! The exit of a run: how `Vm::run_main`, `Vm::call_root` and `Vm::execute_restored` end when
//! they return no value. A run returns `Ok(value)` (its `Returned` exit); its `Err` side is this
//! type, so a caller cannot read a captured run as a throw.
//!
//! - `Threw`: the run threw. An execution termination is one too, uncatchable, so it stays a
//!   `JsError` description (`JsError::is_execution_terminated`).
//! - `Captured`: a native edge captured the frames of the run and stopped there
//!   (`Vm::capture_chain_parked`). The frames are the chain, which `Vm::resume_chain` resumes. No
//!   handler ran and no promise settled, because the frames the handler would unwind are gone.
//!
//! A native returns `Result<Value, JsError>`, so the edge's `Err` is a `JsError` built by
//! `JsError::captured`. `Exit::of` reads that error at the run boundary, the one place a capture
//! turns into this exit.

use super::JsError;
use super::activation::FrameRecord;

#[derive(Debug)]
pub enum Exit {
    /// The run threw, or was terminated, and no handler outside it took the error.
    Threw(JsError),
    /// The run captured its frames at a native edge and stopped there (the chain, bottom first
    /// by its `caller` links).
    Captured(Box<FrameRecord>),
}

impl Exit {
    /// The exit an error ends a run with: a captured error keeps its chain, any other is a throw.
    pub fn of(error: JsError) -> Self {
        match error.into_captured() {
            Ok(chain) => Self::Captured(chain),
            Err(error) => Self::Threw(error),
        }
    }
}

impl From<JsError> for Exit {
    fn from(error: JsError) -> Self {
        Self::of(error)
    }
}

impl From<Exit> for JsError {
    /// The error an exit is for callers that still return `JsError`: a captured exit keeps its
    /// chain in the error (`JsError::captured`), so `into_captured` still finds it.
    fn from(exit: Exit) -> Self {
        match exit {
            Exit::Threw(error) => error,
            Exit::Captured(chain) => JsError::captured(chain),
        }
    }
}
