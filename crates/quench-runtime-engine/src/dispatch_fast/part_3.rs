//! Functions of `dispatch_fast` in a codegen unit of their own (B1: a staged edit recompiles only the units that inline it).
#![allow(unused_imports)]
use super::*;

#[path = "part_3/activation.rs"]
mod activation;
#[allow(unused_imports)]
pub(crate) use activation::*;
#[path = "part_3/construction.rs"]
mod construction;
#[allow(unused_imports)]
pub(crate) use construction::*;
#[path = "part_3/arguments.rs"]
mod arguments;
#[allow(unused_imports)]
pub(crate) use arguments::*;
#[path = "part_3/tiers.rs"]
mod tiers;
#[allow(unused_imports)]
pub(crate) use tiers::*;
#[path = "part_3/body_exits.rs"]
mod body_exits;
#[allow(unused_imports)]
pub(crate) use body_exits::*;
#[path = "part_3/pushes.rs"]
mod pushes;
#[allow(unused_imports)]
pub(crate) use pushes::*;
#[path = "part_3/windows.rs"]
mod windows;
#[allow(unused_imports)]
pub(crate) use windows::*;
#[path = "part_3/environment.rs"]
mod environment;
#[allow(unused_imports)]
pub(crate) use environment::*;
