//! Functions of `dispatch_fast` in a codegen unit of their own (B1: a staged edit recompiles only the units that inline it).
#![allow(unused_imports)]
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

#[path = "part_4/window_envs.rs"]
mod window_envs;
#[allow(unused_imports)]
pub(crate) use window_envs::*;
#[path = "part_4/construct_fields.rs"]
mod construct_fields;
#[allow(unused_imports)]
pub(crate) use construct_fields::*;
#[path = "part_4/activation.rs"]
mod activation;
#[allow(unused_imports)]
pub(crate) use activation::*;
#[path = "part_4/returns.rs"]
mod returns;
#[allow(unused_imports)]
pub(crate) use returns::*;
#[path = "part_4/reads.rs"]
mod reads;
#[allow(unused_imports)]
pub(crate) use reads::*;
#[path = "part_4/async_calls.rs"]
mod async_calls;
#[allow(unused_imports)]
pub(crate) use async_calls::*;
#[path = "part_4/frames.rs"]
mod frames;
#[allow(unused_imports)]
pub(crate) use frames::*;
#[path = "part_4/handlers.rs"]
mod handlers;
#[allow(unused_imports)]
pub(crate) use handlers::*;
#[path = "part_4/fits.rs"]
mod fits;
#[allow(unused_imports)]
pub(crate) use fits::*;
#[path = "part_4/awaits.rs"]
mod awaits;
#[allow(unused_imports)]
pub(crate) use awaits::*;
