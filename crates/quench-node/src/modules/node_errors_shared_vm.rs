//! Node's coded error prototypes and shared private brand.

use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

const INVALID_ARGUMENT_TYPE: &str = "ERR_INVALID_ARG_TYPE";
pub(crate) struct NodeErrorApi {
    brand: RootId,
    prototypes: std::collections::HashMap<(&'static str, ErrorClass, ErrorPresentation), RootId>,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ErrorClass {
    Error,
    TypeError,
    RangeError,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ErrorPresentation {
    Direct,
    HiddenStackFrames,
}
