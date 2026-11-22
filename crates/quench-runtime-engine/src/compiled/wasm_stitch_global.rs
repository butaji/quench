//! The b-jit's rows for a global access (`Instance::GlobalGet`, `Instance::GlobalSet`), planned
//! from the u-jit's facts about the function rather than from the instruction alone: the
//! instruction names a binding register, and only the function's `Plan` knows which environment
//! slot that register binds (`World::global`). The row reads the global's cell at run time from
//! the instance's global area (`wasm_rows_global.rs`), because a compiled program is shared by
//! every instance of its module: a cell address is per instance, so it cannot be baked in.

use super::{Planned, World, leak, slot};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::wasm_instance::{Instance, Storage};
use quench_bjit::padded_holes;
use quench_bjit::wasm::{self as codes};

/// The rows of the global access `step` at `pc` (`a` is the value or the destination register,
/// `b` the binding), or `None` where the global has no row: a reference or a vector, or an access
/// the function's plan did not admit (`World::global` answers `None`).
pub(super) fn global(pc: usize, instruction: WideInstruction, step: Instance, world: &World<'_>) -> Option<Vec<Planned>> {
    let (index, storage) = (world.global)(pc)?;
    let storage = match storage {
        Storage::Bits32 => codes::STORAGE_BITS32,
        Storage::Bits64 => codes::STORAGE_BITS64,
        Storage::Handle => return None,
    };
    let (index, area) = (u64::from(index), world.globals_offset);
    let register = slot(instruction.a());
    let (kind, holes) = match step {
        // D = area[index][0]: hole 0 is D, hole B the environment slot, hole FOURTH the offset
        // of the context's `globals`.
        Instance::GlobalGet => (codes::GLOBAL_GET, [register, 0, index, area]),
        // area[index][0] = A: hole A is A, the rest as above.
        Instance::GlobalSet => (codes::GLOBAL_SET, [0, register, index, area]),
        _ => return None,
    };
    Some(vec![Planned {
        stencil: leak(codes::wasm_name((kind, storage, codes::W64, codes::REGISTER))),
        holes: padded_holes(holes),
        taken: None,
    }])
}
