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
mod unicode;
mod value;
mod value_vec;
mod vm;

pub use bytecode::ResidualProgram;
pub use compile::{Diagnostic, Engine};
pub use heap::RootId;
pub use host::{CapabilityId, Host, HostContext, HostGlobal, ModuleSource, SystemHost};
#[cfg(feature = "profile-memory")]
pub use memory_edge::report_allocator_memory;
pub use value::Value;
pub use vm::{JsError, Vm};

mod api;
pub use api::{ExecutionRequest, Runtime, RuntimeError, SourceKind};
