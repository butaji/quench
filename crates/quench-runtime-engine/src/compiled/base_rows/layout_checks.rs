//! The stencil layout numbers (`staged/layout_offsets.rs`) checked against the engine's own types; the runtime's
//! layouts are checked beside the numbers (`staged/layout_offset_checks.rs`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::JsStepDispatch;
use quench_runtime::staged::layout_offsets as L;
use std::mem::offset_of;

// The call context's raised slot has the stencils' stand-in size and alignment (`layout_offsets::CallContextWords`).
const _: () = assert!(std::mem::size_of::<Option<super::super::Raised>>() == std::mem::size_of::<[usize; 3]>() && std::mem::align_of::<Option<super::super::Raised>>() == std::mem::align_of::<[usize; 3]>());
const _: () = assert!(L::CAPTURE_ADDRESS_PAD == u64::BITS - quench_bjit::shared_words::CAPTURE_ADDRESS_BITS);

// The dispatch record's head is its first field, so the stencil's `JS_STEP_*` offsets (`js_step_head`) are its own.
const _: () = assert!(offset_of!(JsStepDispatch, head) == 0);
