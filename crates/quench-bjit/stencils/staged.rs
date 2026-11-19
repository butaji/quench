#[path = "../../quench-runtime/src/staged/stage.rs"] mod stage;
pub(crate) use stage::{RecordSpace, Stage, Table};
pub(crate) type Concrete = crate::stage::StencilStage;
#[path = "../../quench-runtime/src/staged/region.rs"] #[allow(dead_code)] pub(crate) mod region;
/// The effect vocabulary of the runtime's call protocol, which a stencil row never makes: the frame's default `call` names them.
pub(crate) mod effects {
    pub(crate) struct Effects;
    pub(crate) struct CallTarget<W>(core::marker::PhantomData<W>);
}
/// The frame traits the staged Number rows require of a frame, which a stencil frame implements empty.
pub(crate) trait PropertyFrame<S: Stage>: Frame<S> {}
pub(crate) trait ElementFrame<S: Stage>: PropertyFrame<S> {}
pub(crate) trait CallFrame<S: Stage>: Frame<S> {}
/// The runtime vocabulary the frame's default methods name, which a stencil frame never makes: uninhabited here.
pub(crate) enum Allocation {}
pub(crate) enum Edge {}
pub(crate) enum CallOut {}
#[path = "../../quench-runtime/src/staged/register_frame.rs"] mod register_frame;
pub(crate) use register_frame::{Constant, Frame, Step, jump_if, load_this, logical_not, read_initialized_local, read_local, write_initialized_local, write_local};
pub(crate) use crate::bytecode::Register;
#[path = "../../quench-runtime/src/staged/global.rs"] mod global;
pub(crate) use global::{lexical_read, GlobalFrame};
#[path = "../../quench-runtime/src/staged/math.rs"] pub(crate) mod math;
#[path = "../../quench-runtime/src/staged/numeric.rs"] pub(crate) mod numeric;
pub(crate) use numeric::{Domain, NegativeZero, NumberOperation, NumericFrame, NumericStage, Overflow};
#[path = "../../quench-runtime/src/staged/value.rs"] pub(crate) mod value;
#[path = "../../quench-runtime/src/staged/object.rs"] pub(crate) mod object;
#[path = "../../quench-runtime/src/staged/narrow.rs"] mod narrow;
#[path = "../../quench-runtime/src/staged/elements.rs"] pub(crate) mod elements;
#[path = "../../quench-runtime/src/staged/environment.rs"] pub(crate) mod environment;
#[path = "../../quench-runtime/src/staged/string_length.rs"] pub(crate) mod string_length;
#[path = "../../quench-runtime/src/staged/typed_elements.rs"] pub(crate) mod typed_elements;
#[path = "../../quench-runtime/src/staged/entry.rs"] pub(crate) mod entry;
#[path = "../../quench-runtime/src/staged/frame.rs"] #[allow(dead_code)] pub(crate) mod frame;
#[path = "../../quench-runtime/src/staged/call.rs"] pub(crate) mod call;
#[path = "../../quench-runtime/src/staged/ic.rs"] pub(crate) mod ic;
#[path = "../../quench-runtime/src/staged/slot_store.rs"] pub(crate) mod slot_store;
#[path = "../../quench-runtime/src/staged/iteration.rs"] pub(crate) mod iteration;
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
#[path = "../../quench-runtime/src/staged/wasm_simd_rules.rs"] pub(crate) mod wasm_simd_rules;
#[path = "../../quench-runtime/src/staged/wasm_memory.rs"] pub(crate) mod wasm_memory;
#[path = "../../quench-runtime/src/staged/wasm_memory_vector.rs"] pub(crate) mod wasm_memory_vector;
#[path = "../../quench-runtime/src/staged/wasm_call.rs"] #[allow(dead_code)] pub(crate) mod wasm_call;
#[path = "../../quench-runtime/src/staged/wasm_roots.rs"] pub(crate) mod wasm_roots;
#[path = "../../quench-runtime/src/staged/wasm_instance.rs"] #[allow(dead_code)] pub(crate) mod wasm_instance;
