#![no_std]
#![allow(dead_code, unused_imports, incomplete_features)]
extern crate core as std;
mod layout;
mod stage;
mod facade;
pub(crate) use facade::{Value, heap, typed_ir, value, value_vec, vm};
mod staged;
