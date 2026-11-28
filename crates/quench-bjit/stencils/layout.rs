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

/// The value encoding: tags, handle bits and the narrow layout (`words`, `ValueLayout`, `NarrowLayout`).
#[path = "../../quench-runtime/src/staged/value_encoding.rs"]
pub(crate) mod value_encoding;

/// The object, array and record words (storage forms, tags, prototype ranges, record blocks).
#[path = "../../quench-runtime/src/staged/object_encoding.rs"]
pub(crate) mod object_encoding;

/// The head of an object's extras record, instantiated with words (`ObjectExtrasWords`).
#[path = "../../quench-runtime/src/staged/object_extras.rs"]
pub(crate) mod object_extras;

/// The head of a property buffer, instantiated with a word base (`PropertyBufferHead`).
#[path = "../../quench-runtime/src/staged/property_buffer.rs"]
pub(crate) mod property_buffer;

/// The flat string forms' heads and offsets (`string_form`), the runtime's `native_string` reads the same.
#[path = "../../quench-runtime/src/staged/string_form.rs"]
pub mod string_form;

/// The field cache entry's stored words and their empty values.
#[path = "../../quench-runtime/src/staged/field_cache_layout.rs"]
pub(crate) mod field_cache_layout;

/// The typed-array kinds, the runtime's own enum (`TypedArrayKind`): mounted by `codes.rs` in the stencil
/// crate, and the runtime's `staged::typed_array_catalog` where the runtime's layout test mounts this file.
#[cfg(stencil_build)]
pub(crate) use crate::codes::typed_array_catalog;
#[cfg(not(stencil_build))]
pub(crate) use crate::staged::typed_array_catalog;

#[path = "../../quench-runtime/src/staged/layout_offsets.rs"]
mod offsets;
pub(crate) use offsets::*;

/// The lane-call protocol, shared with the host (`quench_bjit::call_record`).
#[path = "../src/call_record.rs"]
pub mod call_record;
/// The words shared with the host (`quench_bjit::shared_words`).
#[path = "../src/shared_words.rs"]
pub(crate) mod shared_words;
