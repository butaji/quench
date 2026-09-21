mod bytecode;
mod compile;
mod heap;
mod host;
#[cfg(feature = "profile-memory")]
mod memory_edge;
mod profile;
mod value;
mod value_vec;
mod vm;

pub use bytecode::ResidualProgram;
pub use compile::{Diagnostic, Engine};
pub use heap::RootId;
pub use host::{CapabilityId, Host, HostContext, SystemHost};
#[cfg(feature = "profile-memory")]
pub use memory_edge::report_allocator_memory;
pub use value::Value;
pub use vm::{JsError, Vm};

mod api;
pub use api::{ExecutionRequest, Runtime, RuntimeError, SourceKind};
