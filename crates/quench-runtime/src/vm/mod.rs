use crate::intl::tolocale::value::{is_finite, to_string};
use crate::ops::{
    Builtin, FunctionKind, FunctionStrictness, HostCapabilityKind, HostCapabilityRef, Op, RealmId,
};
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
mod realm;
mod scope;
pub(crate) mod vm_arithmetic;
pub(crate) mod vm_ops;
mod vm_typed_bigint;
pub use crate::intl::tolocale::value::is_truthy;

thread_local! {
    static ACTIVE_SOURCE_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
    static COMPILE_SOURCE_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
    static CURRENT_SOURCE_OFFSET: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

pub struct SourceNameGuard(Option<String>);
pub struct CompileSourceNameGuard(Option<String>);
pub(crate) struct SourceOffsetGuard(Option<u32>);

pub fn active_source_name(name: Option<&str>) -> SourceNameGuard {
    SourceNameGuard(ACTIVE_SOURCE_NAME.with(|current| current.replace(name.map(str::to_owned))))
}

pub fn compile_source_name(name: Option<&str>) -> CompileSourceNameGuard {
    CompileSourceNameGuard(
        COMPILE_SOURCE_NAME.with(|current| current.replace(name.map(str::to_owned))),
    )
}

pub fn current_active_source_name() -> Option<String> {
    ACTIVE_SOURCE_NAME.with(|current| current.borrow().clone())
}

pub fn current_compile_source_name() -> Option<String> {
    COMPILE_SOURCE_NAME.with(|current| current.borrow().clone())
}

pub fn current_source_offset() -> Option<u32> {
    CURRENT_SOURCE_OFFSET.with(std::cell::Cell::get)
}

pub(crate) fn source_offset(offset: Option<u32>) -> SourceOffsetGuard {
    SourceOffsetGuard(CURRENT_SOURCE_OFFSET.with(|current| current.replace(offset)))
}

impl Drop for SourceNameGuard {
    fn drop(&mut self) {
        ACTIVE_SOURCE_NAME.with(|current| current.replace(self.0.take()));
    }
}

impl Drop for CompileSourceNameGuard {
    fn drop(&mut self) {
        COMPILE_SOURCE_NAME.with(|current| current.replace(self.0.take()));
    }
}

impl Drop for SourceOffsetGuard {
    fn drop(&mut self) {
        CURRENT_SOURCE_OFFSET.with(|current| current.set(self.0.take()));
    }
}

pub fn reset_host_agent_state() {
    reset_agent_state();
    reset_agent_object();
}

/// Snapshot the active JavaScript function names for host-created errors.
/// The packed VM owns this single call-stack representation; Node adapters
/// only consume the observable names and never maintain a parallel stack.
pub fn current_call_stack_frames() -> Vec<String> {
    vm_ops::call_stack_frames()
}

pub fn current_call_stack_source_names() -> Vec<Option<String>> {
    vm_ops::current_call_stack_source_names()
}

/// Release realms created by the current fixture and reuse compact ids.
pub fn reset_fixture_realms() {
    realm::reset_fixture_state();
    reset_script_contexts();
}

include!("vm_context.rs");
include!("vm_execution.rs");
include!("vm_runtime.rs");
