//! Per-domain handler trampolines. Each trampoline adapts a
//! module-level function into the canonical `CallHandler`.
//! The handlers table is the single canonical place where the
//! capability id resolves to a Rust function.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;
use quench_runtime::{execute, host_api};

use crate::host::HostState;

include!("dispatch_handlers/misc.rs");
include!("dispatch_handlers/events.rs");
include!("dispatch_handlers/util.rs");
include!("dispatch_handlers/buffer.rs");
include!("dispatch_handlers/internal.rs");
include!("dispatch_handlers/os.rs");
include!("dispatch_handlers/timers.rs");
include!("dispatch_handlers/url.rs");
include!("dispatch_handlers/net.rs");
include!("dispatch_handlers/process.rs");
include!("dispatch_handlers/string_decoder.rs");
include!("dispatch_handlers/child_process.rs");
include!("dispatch_handlers/test_runner.rs");
