#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/stage.rs"] mod stage;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use stage::{StagedRecordSpace, Stage, Table, word_bytes, word_offset, Passed};
#[cfg(not(stencil_family = "wasm"))] pub(crate) type Concrete = crate::stage::StencilStage;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/region.rs"] pub(crate) mod region;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/register_frame.rs"] mod register_frame;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use register_frame::{Constant, Frame, Step, bound_this, jump_if, load_this, logical_not, read_arguments_binding, read_initialized_local, read_local, write_initialized_local, write_local};
pub(crate) use crate::bytecode::Register;
// The names the runtime's `staged.rs` root gives its leaves' `use super::*`.
pub(crate) use crate::bytecode::{FieldBase, FieldLookup, Operand};
pub(crate) use crate::typed_ir::Type;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use object::Form;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/global.rs"] mod global;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use global::{lexical_read, GlobalBinding, GlobalFrame};
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/math.rs"] pub(crate) mod math;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/numeric.rs"] pub(crate) mod numeric;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/builtins.rs"] pub(crate) mod builtins;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/strings.rs"] pub(crate) mod strings;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use numeric::{Domain, NegativeZero, NumberOperation, NumericFrame, NumericStage, Overflow};
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/value.rs"] pub(crate) mod value;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/guard.rs"] mod guard;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/object.rs"] pub(crate) mod object;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/narrow.rs"] mod narrow;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/elements.rs"] pub(crate) mod elements;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/double_view.rs"] pub(crate) mod double_view;
// The value encoding, mounted once by `layout.rs`.
pub(crate) use crate::layout::value_encoding;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/environment.rs"] pub(crate) mod environment;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/string_length.rs"] pub(crate) mod string_length;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/code_units.rs"] pub(crate) mod code_units;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/parse_int.rs"] pub(crate) mod parse_int;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/typed_elements.rs"] pub(crate) mod typed_elements;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/entry.rs"] pub(crate) mod entry;
#[path = "../../quench-runtime/src/staged/frame.rs"] pub(crate) mod frame;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/call.rs"] pub(crate) mod call;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/ic.rs"] pub(crate) mod ic;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/ic_flat.rs"] pub(crate) mod ic_flat;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/ic_read.rs"] pub(crate) mod ic_read;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/slot_store.rs"] pub(crate) mod slot_store;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/iteration.rs"] pub(crate) mod iteration;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/array_iteration.rs"] pub(crate) mod array_iteration;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/add.rs"] pub(crate) mod add;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/naming.rs"] pub(crate) mod naming;
// The Wasm rows' machine layer: the staged Wasm rules and the concrete machine they run on, so a
// Wasm stencil is the interpreter's own rule compiled by rustc.
#[path = "../../quench-runtime/src/staged/machine.rs"] pub(crate) mod machine;
#[path = "../../quench-runtime/src/staged/wasm.rs"] pub(crate) mod wasm;
#[path = "../../quench-runtime/src/staged/wasm_float.rs"] pub(crate) mod wasm_float;
#[path = "../../quench-runtime/src/staged/wasm_concrete.rs"] pub(crate) mod wasm_concrete;
// The SIMD operators' bit semantics, shared with the interpreter (`wasm_simd_lanes.rs`).
#[cfg(not(stencil_family = "js"))] #[path = "../../quench-runtime/src/staged/wasm_simd_lanes.rs"] pub(crate) mod wasm_simd_lanes;
#[cfg(not(stencil_family = "js"))] #[path = "../../quench-runtime/src/staged/wasm_simd_rules.rs"] pub(crate) mod wasm_simd_rules;
#[cfg(not(stencil_family = "js"))] #[path = "../../quench-runtime/src/staged/wasm_memory.rs"] pub(crate) mod wasm_memory;
#[cfg(not(stencil_family = "js"))] #[path = "../../quench-runtime/src/staged/wasm_memory_vector.rs"] pub(crate) mod wasm_memory_vector;
#[cfg(not(stencil_family = "js"))] #[path = "../../quench-runtime/src/staged/wasm_call.rs"] #[allow(dead_code)] pub(crate) mod wasm_call;
#[cfg(not(stencil_family = "js"))] #[path = "../../quench-runtime/src/staged/wasm_roots.rs"] pub(crate) mod wasm_roots;
#[cfg(not(stencil_family = "js"))] #[path = "../../quench-runtime/src/staged/wasm_instance.rs"] #[allow(dead_code)] pub(crate) mod wasm_instance;

// The interpreter's whole staged dispatch (call protocol v2 / Futamura: every stencil row is the
// interpreter's staged row run on `StencilStage`). The runtime's `staged.rs` root names the opcode
// catalog, so its leaf modules are included here one by one with the root items they share:
// `Operation` (its decode is the runtime's: `cfg(not(stencil_build))`), the family steps and the
// frame traits. A row builds its `Operation` from its holes and runs the family step over `Places`.
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/operation.rs"] mod operation;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use operation::Operation;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/steps.rs"] mod steps;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use steps::{step, RowStep, step_flow, step_local, step_number, step_value, step_names, step_property, step_keyed, step_generic, step_arith};
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/names.rs"] mod names;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use names::Protocol;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/protocol_literals.rs"] pub(crate) mod protocol_literals;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/protocol_classes.rs"] pub(crate) mod protocol_classes;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/protocol_values.rs"] pub(crate) mod protocol_values;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/protocol_owned.rs"] pub(crate) mod protocol_owned;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/suspend.rs"] mod suspend;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use suspend::{await_source, suspend as suspension_step, SuspendEffect, Suspending, Suspension, YieldConversion};
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/delegation.rs"] mod delegation;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use delegation::DelegationStep;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/captures.rs"] mod captures;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/control.rs"] mod control;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/generic.rs"] mod generic;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/callout.rs"] mod callout;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use callout::CallOut;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/spread.rs"] mod spread;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use spread::Passing;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/compare.rs"] pub(crate) mod compare;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use compare::{nullish_of, Comparison};
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/delete.rs"] mod delete;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/heap.rs"] mod heap;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/element_rows.rs"] pub(crate) mod element_rows;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/element_frame.rs"] mod element_frame;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use element_frame::ElementFrame;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/property.rs"] pub(crate) mod property;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use property::{NoPlans, PropertyFrame};
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/property_write.rs"] mod property_write;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/plan_fields.rs"] pub(crate) mod plan_fields;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/store.rs"] pub(crate) mod store;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/holder.rs"] pub(crate) mod holder;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/forms.rs"] pub(crate) mod forms;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use forms::{Arguments, CallFacts, Callee, FunctionId, Invocation, Receiver, RegisterList};
// The runtime's root names its stencil layout numbers `layout_offsets`; the stencils' are `crate::layout`.
pub(crate) use crate::layout as layout_offsets;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/protocol.rs"] pub(crate) mod protocol;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use protocol::CallOperation;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/invoke.rs"] pub(crate) mod invoke;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use invoke::{CallFrame, Entered, Room};
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/plan.rs"] pub(crate) mod plan;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use plan::{plan_entry, CalleeKind, CallEntry, DirectPlan, Identity, Resolved};
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/native_call.rs"] mod native_call;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/allocation.rs"] mod allocation;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use allocation::{Allocation, InlineEmpty};
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/effects.rs"] pub(crate) mod effects;
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/flow.rs"] pub(crate) mod flow;
// The Wasm jumps `step` dispatches to (pure word arithmetic over the stage); the Concrete-only Wasm
// families are the runtime's alone. The Wasm call rows' staged ABI, roots and instance words.
#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/wasm_jump.rs"] pub(crate) mod wasm_jump;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use wasm_jump::{BranchTable, WasmJump};

#[cfg(not(stencil_family = "wasm"))] #[path = "../../quench-runtime/src/staged/edge.rs"] mod edge;
#[cfg(not(stencil_family = "wasm"))] pub(crate) use edge::Edge;
