//! Current-JS-call-stack tracker.
//!
//! Records the active JS-function chain (function name + originating module
//! file) so `Error.captureStackTrace` can build real, non-empty `CallSite`
//! objects for stack-inspection tools (e.g. the `depd` package under
//! Express/body-parser). Frames reflect actual called closures — no
//! fabricated call sites.
//!
//! Performance: each JS call pushes/pops one `FrameInfo` into a persistent
//! `thread_local` `Vec`; capacity is retained across calls (truncate, not
//! clear), so steady-state adds no allocation.

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::FunctionValue;

#[derive(Debug, Clone)]
pub struct FrameInfo {
    /// The function's inferred name (`<anonymous>` when unnamed).
    pub function: String,
    /// The module file path being executed, when known.
    pub filename: String,
}

thread_local! {
    static CURRENT_FRAMES: RefCell<Vec<FrameInfo>> = const { RefCell::new(Vec::new()) };
    static CURRENT_FILE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Set the module file the runner is currently executing (host calls this on
/// module load so captured frames cite the right file).
pub fn set_current_file(path: String) {
    CURRENT_FILE.with(|cell| *cell.borrow_mut() = Some(path));
}

/// Clear per-program frame state (host calls between independent programs).
pub fn reset() {
    CURRENT_FRAMES.with(|cell| cell.borrow_mut().clear());
    CURRENT_FILE.with(|cell| *cell.borrow_mut() = None);
}

#[allow(clippy::collapsible_if, clippy::collapsible_match)]
fn inferred_name(function: &Rc<FunctionValue>) -> String {
    if let Some((_, value)) = function
        .properties
        .borrow()
        .iter()
        .rev()
        .find(|(key, _)| key == "name")
    {
        if let crate::value::Value::String(name) = value {
            if !name.is_empty() {
                return name.clone();
            }
        }
    }
    "<anonymous>".to_string()
}

/// RAII guard: pushes the frame on construction, pops on drop — including
/// error/panic unwinding — so the stack never leaks across calls.
pub struct FrameGuard {
    popped: bool,
}

impl FrameGuard {
    pub fn enter(function: &Rc<FunctionValue>) -> Self {
        let filename = CURRENT_FILE
            .with(|cell| cell.borrow().clone())
            .unwrap_or_default();
        let frame = FrameInfo {
            function: inferred_name(function),
            filename,
        };
        CURRENT_FRAMES.with(|cell| cell.borrow_mut().push(frame));
        Self { popped: false }
    }
}

impl Drop for FrameGuard {
    fn drop(&mut self) {
        if !self.popped {
            CURRENT_FRAMES.with(|cell| {
                cell.borrow_mut().pop();
            });
            self.popped = true;
        }
    }
}

/// Snapshot the active frames, oldest first.
pub fn snapshot() -> Vec<FrameInfo> {
    CURRENT_FRAMES.with(|cell| cell.borrow().clone())
}
