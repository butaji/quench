//! Operation semantics written once, over a binding-time domain (`Stage`).
//!
//! This is Futamura's first projection made literal through Ershov's mixed computation. Each
//! operation below is one Rust function, generic over the stage that runs it:
//!
//! - The interpreter runs it on machine words (`Concrete`). Every condition is known there, so
//!   only the arm taken runs.
//! - A specializer runs it on IR values, which emits the residual code (`Emit` in
//!   `vm/compiled/object_slots.rs`). A condition it cannot know becomes a guard, or both arms.
//!
//! A plain Rust argument (a slot, a storage form a site proved) is static, so a branch on it
//! leaves no code. A rule written here is the rule of every tier, which is what makes the
//! native tiers the interpreter specialized, not a second implementation of it
//! (`docs/v2/specialization.md`). `staged_coverage` in this module's tests keeps hand-written
//! decodes of the same layouts from reappearing elsewhere.

use crate::bytecode::{Decoder, FieldBase, FieldLookup, Op, Operand, Register, WideInstruction};
use crate::typed_ir::Type;

mod concrete;
pub(crate) mod numeric;
/// The stencil layout numbers, one source for the stencils (`#[path]`) and the runtime's const checks.
pub(crate) mod layout_offsets;
/// The head of the dispatch payload, which the runtime embeds and the stencils take offsets from.
pub(crate) mod js_step_head;
pub(crate) mod record_heads;
pub(crate) mod encodings;
/// The value encoding (tags, handle bits, narrow layout), shared with the stencils.
pub(crate) mod value_encoding;
/// The field cache entry's stored words and complement encoding, shared with the stencils.
pub(crate) mod field_cache_layout;
/// The object, array and record words (storage forms, tags, prototype ranges, record blocks), shared with the stencils.
pub(crate) mod object_encoding;
/// The head of an object's extras record (inline values, arena start, element backing).
pub(crate) mod object_extras;
/// The head of a property buffer (base pointer and length), embedded first by `PropertyBuffer`.
pub(crate) mod property_buffer;
/// The flat string forms' heads and offsets, shared with the stencils (`vm::wtf16::native_string`).
pub(crate) mod string_form;
/// The typed-array kinds, one file for the runtime and the stencils (`heap/cell/typed_array_catalog.rs`).
#[path = "heap/cell/typed_array_catalog.rs"]
pub(crate) mod typed_array_catalog;
/// The b-jit's shared words, as the stencils take them.
pub(crate) use quench_bjit::shared_words;
pub(crate) mod flow;
pub(crate) mod machine;
#[cfg_attr(not(feature = "jit"), allow(dead_code))] // a specializer's composition
pub(crate) mod barrier;
#[cfg_attr(not(feature = "jit"), allow(dead_code))] // a specializer's operations; the interpreter has native Numbers
pub(crate) mod number_machine;
mod heap;
mod global;
#[cfg_attr(not(test), allow(dead_code))] // the division is data the specializers will ask; today only its tests do
pub(crate) mod division;
pub(crate) mod math;
mod names;
mod callout;
mod spread;
mod admission;
mod delete;
mod decode;
mod captures;
pub(crate) mod compare;
pub(crate) use global::{lexical_read, GlobalBinding, GlobalFrame};
pub(crate) use names::Protocol;
pub(crate) use callout::{CallOut, OperandValues};
pub(crate) use spread::Passing;
pub(crate) use decode::{DecodeContext, Fields};
pub(crate) use compare::{nullish_of, Comparison};
pub(crate) use concrete::Concrete;
pub(crate) use numeric::{Domain, NegativeZero, NumberOperation, NumericFrame, NumericStage, Overflow};
pub(crate) mod builtins;
pub(crate) mod add;
pub(crate) mod add_plan;
pub(crate) mod elements;
pub(crate) mod element_rows;
pub(crate) mod typed_elements;
pub(crate) mod typed_view;
mod element_frame;
pub(crate) use element_frame::ElementFrame;
pub(crate) mod entry;
pub(crate) mod environment;
pub(crate) mod ic;
pub(crate) mod ic_flat;
pub(crate) mod ic_read;
pub(crate) mod object;
mod narrow;
pub(crate) mod store;
pub(crate) mod slot_store;
pub(crate) mod strings;
pub(crate) mod string_length;
pub(crate) mod code_units;
pub(crate) mod parse_int;
pub(crate) mod holder;
pub(crate) mod iteration;
pub(crate) mod naming;
pub(crate) mod call; pub(crate) mod frame;
pub(crate) mod forms;
pub(crate) mod protocol;
pub(crate) use forms::{Arguments, CallFacts, Callee, FunctionId, Invocation, Receiver, RegisterList};
pub(crate) use protocol::CallOperation;
pub(crate) mod invoke;
pub(crate) mod plan;
mod native_call;
pub(crate) use plan::{plan_entry, CalleeKind, CallEntry, DirectPlan, Identity, Resolved};
#[cfg(test)]
pub(crate) use plan::Bound;
pub(crate) use invoke::{CallFrame, Entered, Room};
mod allocation;
pub(crate) use allocation::Allocation;
pub(crate) mod plan_fields;
pub(crate) mod property;
mod property_write;
pub(crate) use property::{NoPlans, PropertyFrame};
pub(crate) mod value;
mod guard;
mod steps;
mod control;
mod generic;
pub(crate) use steps::step;
pub(crate) mod effects;
#[allow(dead_code)] // the K-PLAN builder; its first consumer is the u-jit lowering (P4, TOD-1700)
pub(crate) mod function_plan;
pub(crate) mod region;
pub(crate) mod wasm;
pub(crate) mod wasm_float;
mod wasm_concrete;
pub(crate) mod wasm_call;
pub(crate) mod wasm_flow;
pub(crate) mod wasm_instance;
pub(crate) mod wasm_machine;
pub(crate) mod wasm_memory;
pub(crate) mod wasm_operation;
pub(crate) mod wasm_simd;
pub(crate) mod wasm_simd_concrete;
pub(crate) mod wasm_step;
pub(crate) use wasm_concrete::ConcreteWasm;
#[cfg(test)]
pub(crate) mod trace;
#[allow(unused_imports)] // the Stage v2 vocabulary: stencil and UltraJIT stages use the rest
pub(crate) use effects::{Effects, Region, RegionSet};
#[allow(unused_imports)]
pub(crate) use region::Access;
pub(crate) use ic::{HINTED_FORM, TAGGED_SLOTS, concrete_hinted_own_field_hit, concrete_own_field_hit, concrete_site_own_chain, concrete_site_own_read, concrete_site_store};
pub(crate) use object::{Form, Layout, Slots, concrete_own_read, arena_extras_layout};

mod stage;
pub(crate) use stage::{fill_by_rows, Passed, RecordSpace, Stage, Table, word_bytes, word_offset};

mod register_frame;
pub(crate) use register_frame::{Constant, Frame, Step, bound_this, jump_if, load_this, logical_not, read_initialized_local, read_local, write_initialized_local, write_local};

mod operation;
pub(crate) use operation::Operation;

mod binary_class;
pub(crate) use binary_class::BinaryClass;

mod edge;
pub(crate) use edge::Edge;

/// Where a row goes when its frame cannot decide a step: the VM's general step, or, for a
/// capture access whose scopes the frame does not hold, the walk over the scopes from the
/// activation's environment (a read) or the store into the scope it finds (a write).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Miss { General, WalkScopes, StoreThroughScopes }

/// The row of `op`'s miss route (`Miss`).
pub(crate) const fn miss(op: Op) -> Miss {
    match op {
        Op::LoadCapture => Miss::WalkScopes,
        Op::StoreCapture => Miss::StoreThroughScopes,
        _ => Miss::General,
    }
}


#[cfg(test)]
mod test_words;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod stage_coverage_tests;
#[cfg(test)]
mod capture_return_tests;
#[cfg(test)]
mod generic_tests;
#[cfg(test)]
mod element_tests;
#[cfg(test)]
mod primitive_tests;
#[cfg(test)]
mod typed_element_tests;
#[cfg(test)]
mod wasm_tests;
#[cfg(test)]
mod wasm_float_tests;
#[cfg(test)]
mod wasm_instance_tests;
#[cfg(test)]
mod wasm_memory_tests;
#[cfg(test)]
mod wasm_flow_tests;
#[cfg(test)]
mod wasm_call_tests;
#[cfg(test)]
mod wasm_simd_tests;
#[cfg(test)]
mod instanceof_tests;
