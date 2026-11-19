//! The stencils' layout numbers. The values live in `quench-runtime/src/staged/layout_offsets.rs`,
//! which both builds include (`#[path]`), so a stencil and the runtime read one set of literals; the
//! runtime checks them against its own `offset_of!` values in `vm/compiled/base_rows.rs`.
//! The dispatch payload's head (`JS_STEP_*`) is `offset_of!` of one `#[repr(C)]` struct,
//! `staged/js_step_head.rs`, which the runtime embeds in `JsStepDispatch`. The two words shared
//! with the host (`call_record`, `shared_words`) are mounted here.

#[path = "../../quench-runtime/src/staged/js_step_head.rs"]
pub(crate) mod js_step_head;

#[path = "../../quench-runtime/src/staged/record_heads.rs"]
pub(crate) mod record_heads;

#[path = "../../quench-runtime/src/staged/encodings.rs"]
pub(crate) mod encodings;

#[path = "../../quench-runtime/src/staged/layout_offsets.rs"]
mod offsets;
pub(crate) use offsets::*;

/// The lane-call protocol, shared with the host (`quench_bjit::call_record`).
#[path = "../src/call_record.rs"]
pub(crate) mod call_record;
/// The words shared with the host (`quench_bjit::shared_words`).
#[path = "../src/shared_words.rs"]
pub(crate) mod shared_words;
