#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/stage.rs"] mod stage;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/word_ops.rs"] pub(crate) mod word_ops;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use stage::{StagedRecordSpace, Stage, Table, word_bytes, word_offset, Passed};
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) type Concrete = crate::stage::StencilStage;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/region.rs"] pub(crate) mod region;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/register_frame.rs"] mod register_frame;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use register_frame::{Constant, Frame, Step, jump_if, read_arguments_binding, load_this, logical_not, read_initialized_local, read_local, write_initialized_local, write_local};
pub(crate) use crate::bytecode::{FieldBase, FieldLookup, Operand, Register};
pub(crate) use crate::typed_ir::Type;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use object::Form;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/global.rs"] mod global;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use global::{lexical_read, GlobalBinding, GlobalFrame};
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/math.rs"] pub(crate) mod math;
#[path = "../../quench-runtime/src/staged/int_math.rs"] pub(crate) mod int_math;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/numeric.rs"] pub(crate) mod numeric;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/proof.rs"] pub(crate) mod proof;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use proof::{Float64, Int32, IntFact, IntOf, Linked, NumberFact, NumberOf, ShapeFact, Shaped};
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/builtins.rs"] pub(crate) mod builtins;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/strings.rs"] pub(crate) mod strings;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/call_native.rs"] pub(crate) mod call_native;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use numeric::{Domain, NegativeZero, NumberOperation, NumericFrame, NumericStage, Overflow};
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/value.rs"] pub(crate) mod value;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/guard.rs"] mod guard;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/object.rs"] pub(crate) mod object;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/narrow.rs"] mod narrow;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/elements.rs"] pub(crate) mod elements;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/environment.rs"] pub(crate) mod environment;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/string_length.rs"] pub(crate) mod string_length;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/code_units.rs"] pub(crate) mod code_units;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/parse_int.rs"] pub(crate) mod parse_int;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/typed_elements.rs"] pub(crate) mod typed_elements;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/entry.rs"] pub(crate) mod entry;
#[path = "../../quench-runtime/src/staged/frame.rs"] pub(crate) mod frame;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/call.rs"] pub(crate) mod call;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/ic.rs"] pub(crate) mod ic;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/ic_flat.rs"] pub(crate) mod ic_flat;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/ic_read.rs"] pub(crate) mod ic_read;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/slot_store.rs"] pub(crate) mod slot_store;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/iteration.rs"] pub(crate) mod iteration;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/array_iteration.rs"] pub(crate) mod array_iteration;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/add.rs"] pub(crate) mod add;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/naming.rs"] pub(crate) mod naming;
// The Wasm rows' machine layer: the staged Wasm rules and the concrete machine they run on, so a
// Wasm stencil is the interpreter's own rule compiled by rustc.
#[path = "../../quench-runtime/src/staged/machine.rs"] pub(crate) mod machine;
#[path = "../../quench-runtime/src/staged/wasm.rs"] pub(crate) mod wasm;
#[path = "../../quench-runtime/src/staged/wasm_float.rs"] pub(crate) mod wasm_float;
#[path = "../../quench-runtime/src/staged/wasm_concrete.rs"] pub(crate) mod wasm_concrete;
// The SIMD operators' bit semantics, shared with the interpreter (`wasm_simd_lanes.rs`).
#[path = "../../quench-runtime/src/staged/wasm_simd_lanes.rs"] pub(crate) mod wasm_simd_lanes;
#[path = "../../quench-runtime/src/staged/wasm_simd_rules.rs"] pub(crate) mod wasm_simd_rules;
#[cfg(not(any(stencil_family = "js", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/wasm_memory.rs"] pub(crate) mod wasm_memory;
#[cfg(not(any(stencil_family = "js", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/wasm_memory_vector.rs"] pub(crate) mod wasm_memory_vector;
#[cfg(not(any(stencil_family = "js", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/wasm_call.rs"] #[allow(dead_code)] pub(crate) mod wasm_call;
#[cfg(not(any(stencil_family = "js", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/wasm_roots.rs"] pub(crate) mod wasm_roots;
#[cfg(not(any(stencil_family = "js", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/wasm_instance.rs"] #[allow(dead_code)] pub(crate) mod wasm_instance;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/barrier.rs"] pub(crate) mod barrier;

// The interpreter's whole staged dispatch (call protocol v2 / Futamura: every stencil row is the
// interpreter's staged row run on `StencilStage`). The runtime's `staged.rs` root names the opcode
// catalog, so its leaf modules are included here one by one with the root items they share:
// `Operation` (its decode is the runtime's: `cfg(not(stencil_build))`), the family steps and the
// frame traits. A row builds its `Operation` from its holes and runs the family step over `Places`.
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/operation.rs"] mod operation;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use operation::Operation;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use operation::kind as op_kind;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/steps.rs"] mod steps;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use steps::{step, RowStep, step_flow, step_local, step_number, step_value, step_names, step_property, step_keyed, step_generic, step_arith, step_of, step_flow_of, step_local_of, step_value_of, step_names_of, step_property_of, step_keyed_of, step_generic_of, step_arith_of};
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/names.rs"] mod names;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use names::Protocol;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/captures.rs"] mod captures;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/control.rs"] mod control;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/generic.rs"] mod generic;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/callout.rs"] mod callout;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use callout::{CallOut, OperandValues};
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/spread.rs"] mod spread;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use spread::Passing;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/compare.rs"] pub(crate) mod compare;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use compare::Comparison;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/delete.rs"] mod delete;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/heap.rs"] mod heap;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/element_rows.rs"] pub(crate) mod element_rows;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/element_frame.rs"] mod element_frame;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use element_frame::ElementFrame;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/property.rs"] pub(crate) mod property;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use property::{NoPlans, PropertyFrame};
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/property_write.rs"] mod property_write;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/plan_fields.rs"] pub(crate) mod plan_fields;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/store.rs"] pub(crate) mod store;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/holder.rs"] pub(crate) mod holder;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/forms.rs"] pub(crate) mod forms;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use forms::{Arguments, CallFacts, Callee, FunctionId, Invocation, Receiver, RegisterList};
// The runtime's root names its stencil layout numbers `layout_offsets`; the stencils' are `crate::layout`.
pub(crate) use crate::layout as layout_offsets;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/protocol.rs"] pub(crate) mod protocol;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/native_frame.rs"] pub(crate) mod native_frame;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use protocol::CallOperation;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/invoke.rs"] pub(crate) mod invoke;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use invoke::{CallFrame, Entered, Room};
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/plan.rs"] pub(crate) mod plan;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use plan::{CalleeKind, CallEntry, DirectPlan, Identity};
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/native_call.rs"] mod native_call;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/allocation.rs"] mod allocation;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use allocation::Allocation;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/effects.rs"] pub(crate) mod effects;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/flow.rs"] pub(crate) mod flow;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/poll.rs"] pub(crate) mod poll;
// The Wasm families `step` dispatches to (pure word arithmetic over the stage).
#[path = "../../quench-runtime/src/staged/wasm_simd.rs"] mod wasm_simd;

#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] #[path = "../../quench-runtime/src/staged/edge.rs"] mod edge;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))] pub(crate) use edge::Edge;
