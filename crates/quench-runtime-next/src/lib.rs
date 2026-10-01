#![deny(warnings)]

mod bigint;
mod bytecode;
mod compile;
mod heap;
mod host;
#[cfg(feature = "profile-memory")]
mod memory_edge;
mod module_identity;
mod number_to_string;
mod profile;
pub(crate) use quench_stack as stack;
mod unicode;
mod value;
mod value_vec;
mod vm;
mod wasm;
pub use wasm::WasmI32Function;

pub use bytecode::ResidualProgram;
pub use compile::{Diagnostic, Engine};
pub use heap::RootId;
pub use host::{CapabilityId, Host, HostContext, HostGlobal, ModuleSource, SystemHost};
#[cfg(feature = "profile-memory")]
pub use memory_edge::report_allocator_memory;
pub use stack::{STACK_BUDGET_BYTES, STACK_HEADROOM_BYTES, WORKER_STACK_SIZE};
pub use value::Value;
pub use vm::JsError;

mod api;
pub use api::{ExecutionRequest, Runtime, RuntimeError, SourceKind};
