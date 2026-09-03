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
pub(crate) use vm_arithmetic::numeric_to_int32;

pub fn reset_host_agent_state() {
    reset_agent_state();
    reset_agent_object();
}

include!("vm_context.rs");
include!("vm_execution.rs");
include!("vm_runtime.rs");
