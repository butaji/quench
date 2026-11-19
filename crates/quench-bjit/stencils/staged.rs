#[path = "../../quench-runtime/src/staged/stage.rs"] mod stage;
pub(crate) use stage::{StagedRecordSpace, Stage, Table, word_bytes, word_offset, Passed};
pub(crate) type Concrete = crate::stage::StencilStage;
#[path = "../../quench-runtime/src/staged/region.rs"] #[allow(dead_code)] pub(crate) mod region;
#[path = "../../quench-runtime/src/staged/register_frame.rs"] mod register_frame;
pub(crate) use register_frame::{Constant, Frame, Step, bound_this, jump_if, load_this, logical_not, read_arguments_binding, read_initialized_local, read_local, write_initialized_local, write_local};
pub(crate) use crate::bytecode::Register;
// The names the runtime's `staged.rs` root gives its leaves' `use super::*`.
pub(crate) use crate::bytecode::{FieldBase, FieldLookup, Operand};
pub(crate) use crate::typed_ir::Type;
pub(crate) use object::Form;
#[path = "../../quench-runtime/src/staged/global.rs"] mod global;
pub(crate) use global::{lexical_read, GlobalBinding, GlobalFrame};
#[path = "../../quench-runtime/src/staged/math.rs"] pub(crate) mod math;
#[path = "../../quench-runtime/src/staged/numeric.rs"] pub(crate) mod numeric;
#[path = "../../quench-runtime/src/staged/builtins.rs"] pub(crate) mod builtins;
#[path = "../../quench-runtime/src/staged/strings.rs"] pub(crate) mod strings;
pub(crate) use numeric::{Domain, NegativeZero, NumberOperation, NumericFrame, NumericStage, Overflow};
#[path = "../../quench-runtime/src/staged/value.rs"] pub(crate) mod value;
#[path = "../../quench-runtime/src/staged/guard.rs"] mod guard;
#[path = "../../quench-runtime/src/staged/object.rs"] pub(crate) mod object;
#[path = "../../quench-runtime/src/staged/narrow.rs"] mod narrow;
#[path = "../../quench-runtime/src/staged/elements.rs"] pub(crate) mod elements;
#[path = "../../quench-runtime/src/staged/environment.rs"] pub(crate) mod environment;
#[path = "../../quench-runtime/src/staged/string_length.rs"] pub(crate) mod string_length;
#[path = "../../quench-runtime/src/staged/code_units.rs"] pub(crate) mod code_units;
#[path = "../../quench-runtime/src/staged/parse_int.rs"] pub(crate) mod parse_int;
#[path = "../../quench-runtime/src/staged/typed_elements.rs"] pub(crate) mod typed_elements;
#[path = "../../quench-runtime/src/staged/entry.rs"] pub(crate) mod entry;
#[path = "../../quench-runtime/src/staged/frame.rs"] #[allow(dead_code)] pub(crate) mod frame;
#[path = "../../quench-runtime/src/staged/call.rs"] pub(crate) mod call;
#[path = "../../quench-runtime/src/staged/ic.rs"] pub(crate) mod ic;
#[path = "../../quench-runtime/src/staged/slot_store.rs"] pub(crate) mod slot_store;
#[path = "../../quench-runtime/src/staged/iteration.rs"] pub(crate) mod iteration;
#[path = "../../quench-runtime/src/staged/array_iteration.rs"] pub(crate) mod array_iteration;
#[path = "../../quench-runtime/src/staged/add.rs"] pub(crate) mod add;
#[path = "../../quench-runtime/src/staged/naming.rs"] pub(crate) mod naming;
// The Wasm rows' machine layer: the staged Wasm rules and the concrete machine they run on, so a
// Wasm stencil is the interpreter's own rule compiled by rustc.
#[path = "../../quench-runtime/src/staged/machine.rs"] pub(crate) mod machine;
#[path = "../../quench-runtime/src/staged/wasm.rs"] pub(crate) mod wasm;
#[path = "../../quench-runtime/src/staged/wasm_float.rs"] pub(crate) mod wasm_float;
#[path = "../../quench-runtime/src/staged/wasm_concrete.rs"] pub(crate) mod wasm_concrete;
// The SIMD operators' bit semantics, shared with the interpreter (`wasm_simd_lanes.rs`).
#[path = "../../quench-runtime/src/staged/wasm_simd_lanes.rs"] pub(crate) mod wasm_simd_lanes;
#[path = "../../quench-runtime/src/staged/wasm_memory.rs"] pub(crate) mod wasm_memory;

// The interpreter's whole staged dispatch (call protocol v2 / Futamura: every stencil row is the
// interpreter's staged row run on `StencilStage`). The runtime's `staged.rs` root names the opcode
// catalog, so its leaf modules are included here one by one with the root items they share:
// `Operation` (its decode is the runtime's: `cfg(not(stencil_build))`), the family steps and the
// frame traits. A row builds its `Operation` from its holes and runs the family step over `Places`.
#[path = "../../quench-runtime/src/staged/operation.rs"] mod operation;
pub(crate) use operation::Operation;
#[path = "../../quench-runtime/src/staged/steps.rs"] mod steps;
pub(crate) use steps::{step, RowStep, step_flow, step_local, step_number, step_value, step_names, step_property, step_keyed, step_generic, step_arith};
#[path = "../../quench-runtime/src/staged/names.rs"] mod names;
pub(crate) use names::Protocol;
#[path = "../../quench-runtime/src/staged/protocol_literals.rs"] pub(crate) mod protocol_literals;
#[path = "../../quench-runtime/src/staged/protocol_classes.rs"] pub(crate) mod protocol_classes;
#[path = "../../quench-runtime/src/staged/protocol_values.rs"] pub(crate) mod protocol_values;
#[path = "../../quench-runtime/src/staged/protocol_owned.rs"] pub(crate) mod protocol_owned;
#[path = "../../quench-runtime/src/staged/suspend.rs"] mod suspend;
pub(crate) use suspend::{await_source, suspend as suspension_step, SuspendEffect, Suspending, Suspension, YieldConversion};
#[path = "../../quench-runtime/src/staged/delegation.rs"] mod delegation;
pub(crate) use delegation::DelegationStep;
#[path = "../../quench-runtime/src/staged/captures.rs"] mod captures;
#[path = "../../quench-runtime/src/staged/control.rs"] mod control;
#[path = "../../quench-runtime/src/staged/generic.rs"] mod generic;
#[path = "../../quench-runtime/src/staged/callout.rs"] mod callout;
pub(crate) use callout::CallOut;
#[path = "../../quench-runtime/src/staged/spread.rs"] mod spread;
pub(crate) use spread::Passing;
#[path = "../../quench-runtime/src/staged/compare.rs"] pub(crate) mod compare;
pub(crate) use compare::{nullish_of, Comparison};
#[path = "../../quench-runtime/src/staged/delete.rs"] mod delete;
#[path = "../../quench-runtime/src/staged/heap.rs"] mod heap;
#[path = "../../quench-runtime/src/staged/element_rows.rs"] pub(crate) mod element_rows;
#[path = "../../quench-runtime/src/staged/element_frame.rs"] mod element_frame;
pub(crate) use element_frame::ElementFrame;
#[path = "../../quench-runtime/src/staged/property.rs"] pub(crate) mod property;
pub(crate) use property::{NoPlans, PropertyFrame};
#[path = "../../quench-runtime/src/staged/property_write.rs"] mod property_write;
#[path = "../../quench-runtime/src/staged/plan_fields.rs"] pub(crate) mod plan_fields;
#[path = "../../quench-runtime/src/staged/store.rs"] pub(crate) mod store;
#[path = "../../quench-runtime/src/staged/holder.rs"] pub(crate) mod holder;
#[path = "../../quench-runtime/src/staged/forms.rs"] pub(crate) mod forms;
pub(crate) use forms::{Arguments, CallFacts, Callee, FunctionId, Invocation, Receiver, RegisterList};
// The runtime's root names its stencil layout numbers `layout_offsets`; the stencils' are `crate::layout`.
pub(crate) use crate::layout as layout_offsets;
#[path = "../../quench-runtime/src/staged/protocol.rs"] pub(crate) mod protocol;
pub(crate) use protocol::CallOperation;
#[path = "../../quench-runtime/src/staged/invoke.rs"] pub(crate) mod invoke;
pub(crate) use invoke::{CallFrame, Entered, Room};
#[path = "../../quench-runtime/src/staged/plan.rs"] pub(crate) mod plan;
pub(crate) use plan::{plan_entry, CalleeKind, CallEntry, DirectPlan, Identity, Resolved};
#[path = "../../quench-runtime/src/staged/native_call.rs"] mod native_call;
#[path = "../../quench-runtime/src/staged/allocation.rs"] mod allocation;
pub(crate) use allocation::{Allocation, InlineEmpty};
#[path = "../../quench-runtime/src/staged/effects.rs"] pub(crate) mod effects;
#[path = "../../quench-runtime/src/staged/flow.rs"] pub(crate) mod flow;
// The Wasm jumps `step` dispatches to (pure word arithmetic over the stage); the Concrete-only Wasm
// families are the runtime's alone. The Wasm call rows' staged ABI, roots and instance words.
#[path = "../../quench-runtime/src/staged/wasm_jump.rs"] pub(crate) mod wasm_jump;
pub(crate) use wasm_jump::{BranchTable, WasmJump};
#[path = "../../quench-runtime/src/staged/wasm_call.rs"] #[allow(dead_code)] pub(crate) mod wasm_call;
#[path = "../../quench-runtime/src/staged/wasm_roots.rs"] pub(crate) mod wasm_roots;
#[path = "../../quench-runtime/src/staged/wasm_instance.rs"] #[allow(dead_code)] pub(crate) mod wasm_instance;

#[path = "../../quench-runtime/src/staged/edge.rs"] mod edge;
pub(crate) use edge::Edge;
