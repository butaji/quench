//! The JIT's compile routes, which link only into a binary whose runtimes run them: `Vm::new`
//! installs them (`quench_layer_enable_jit`) as it makes a runtime, while a binary that only
//! resumes a runtime saved at rest (`Vm::new_at_rest`) never names them, so it links no
//! Cranelift. Without them every route declines and the interpreter runs, as under `--no-jit`.
//! The lane's handlers are installed the same way: every opcode's by `Vm::new`
//! (`install_all_lane_rows`), a saved program's own by its image (`install_lane_rows`).
#[allow(unused_imports)]
use crate::ext_traits::*;

/// The JIT's route of `specialize_in_lane` for a function with no ahead image.
pub(crate) type LaneJit = fn(*mut quench_runtime::vm::engine_types::LaneContext, &quench_runtime::vm::engine_types::LaneView, u32, &[quench_runtime::Value],
    Option<(u64, u64)>, crate::compiled::LaneBodyWork<'_>) -> crate::compiled::LaneBodyOutcome;

/// The routes `Vm::new` installs.
pub(crate) struct JitRoutes {
    /// `dispatch_fast::specialize_in_lane`'s route for a function with no ahead image.
    pub(crate) lane: LaneJit,
    /// A lane body's plan and compile (`Vm::body_work_in_lane`).
    pub(crate) body: crate::compiled::lane_compile::BodyWork,
    /// A numeric function's image (`js_compiled::start`).
    pub(crate) numeric: crate::js_compiled::NumericStart,
    /// A hot loop's region (`compiled::start_osr_region`).
    pub(crate) region: crate::compiled::RegionStart,
    /// A Wasm program's code (`compile_program_with_diagnostics_and_root_mode`).
    pub(crate) wasm: crate::compiled::WasmCompile,
    /// The compiler of hot regexp programs (`regexp_native::enable`).
    pub(crate) regexp: fn(bool, (u32, u32)),
    /// The lane's fusion passes, which name every fused and chain row.
    pub(crate) fuse: LaneFusion,
}

pub(crate) type LaneFusion = fn(&mut [crate::dispatch_fast::LaneInstruction], &[quench_runtime::bytecode::WideInstruction],
    Option<&quench_runtime::compile::liveness::RegisterMasks>, bool, bool) -> Vec<(usize, usize)>;

static JIT: std::sync::OnceLock<JitRoutes> = std::sync::OnceLock::new();

/// The installed routes, or `None` in a binary that never makes a runtime with `Vm::new`.
#[inline]
pub(crate) fn jit() -> Option<&'static JitRoutes> {
    JIT.get()
}

#[unsafe(no_mangle)]
fn quench_layer_enable_jit() {
    // An execution-only build (no `jit`) runs image bodies alone: it installs neither the
    // JIT's routes nor any lane row, so no opcode's handlers are reachable.
    #[cfg(feature = "jit")]
    {
        // A runtime made with `Vm::new` may run any opcode in the lane.
        install_all_lane_rows();
        JIT.get_or_init(|| JitRoutes {
        lane: crate::dispatch_fast::jit_lane_body,
        body: <quench_runtime::vm::Vm as crate::compiled::lane_compile::CompiledLaneCompileVm>::compile_body_work,
        numeric: crate::js_compiled::start_numeric,
        region: crate::compiled::compile_osr_region,
        wasm: <quench_runtime::vm::Vm as crate::compiled::CompiledVm4>::compile_wasm_program,
        regexp: crate::regexp_native::enable_compiler,
        // Chains, the target back edges, then pairs (`derive_lane_view`); an image, whose bodies leave to the
        // lane only at misses, links no fused row.
        fuse: |records, decoded, live, loop_tier, raw_results| {
            use crate::dispatch_fast::{fuse_target_backedges, lane_frame::{fuse_chains, fuse_rows}};
            let mut fused = fuse_chains(records, decoded, live, raw_results);
            fuse_target_backedges(records, loop_tier);
            fused.extend(fuse_rows(records, decoded, live, loop_tier, raw_results));
            fused
        },
    });
    }
}
const _: quench_runtime::vm::layer_hooks::signatures::enable_jit = quench_layer_enable_jit;

use crate::dispatch_fast::{LaneHandler, LaneTable, ACC_POSITIONS, TABLE_ROW, TABLE_ROWS};

/// One opcode's lane handlers, by jump direction and accumulator field, as a static of its own
/// (`lane_rows`): a binary links an opcode's handlers exactly when it names the opcode's row,
/// which this crate instantiated, so naming it compiles nothing again.
pub struct LaneRow(u16, [[Option<LaneHandler>; ACC_POSITIONS]; 2]);

impl LaneRow {
    const fn of(op: u16) -> Self {
        let tables = LaneTable::TABLES;
        let mut row = [[None; ACC_POSITIONS]; 2];
        let mut entry = 0;
        while entry < 2 * ACC_POSITIONS {
            row[entry / ACC_POSITIONS][entry % ACC_POSITIONS] = tables[entry / ACC_POSITIONS][entry % ACC_POSITIONS][op as usize];
            entry += 1;
        }
        Self(op, row)
    }
}

/// The lane's rows by opcode, in groups of `TABLE_ROW`; `None` for an opcode without lane
/// handlers.
#[cfg(feature = "jit")]
type LaneRows = [[Option<&'static LaneRow>; TABLE_ROW as usize]; TABLE_ROWS];

#[cfg(feature = "jit")]
macro_rules! lane_rows {
    ($($group:ident = $index:literal),*) => {
        /// Each opcode's row (`LaneRow`): opcode `LANE_ROW_GROUP * g + r` is `g{g}::R{r}`.
        pub mod lane_rows {
            $(pub mod $group {
                use super::super::{LaneRow, TABLE_ROW};
                lane_rows!(@group $index R0 0 R1 1 R2 2 R3 3 R4 4 R5 5 R6 6 R7 7 R8 8 R9 9 R10 10 R11 11 R12 12 R13 13 R14 14 R15 15);
            })*
        }
        /// Every opcode's row: a binary that makes runtimes with `Vm::new` installs these.
        static ALL_LANE_ROWS: LaneRows = [$(lane_rows!(@all $group R0 R1 R2 R3 R4 R5 R6 R7 R8 R9 R10 R11 R12 R13 R14 R15)),*]; 
    };
    (@group $index:literal $($row:ident $offset:literal)*) => {
        $(pub static $row: LaneRow = LaneRow::of($index * TABLE_ROW + $offset);)*
    };
    (@all $group:ident $($row:ident)*) => { [$(Some(&lane_rows::$group::$row)),*] };
}
#[cfg(feature = "jit")]
lane_rows!(g0 = 0, g1 = 1, g2 = 2, g3 = 3, g4 = 4, g5 = 5, g6 = 6, g7 = 7, g8 = 8, g9 = 9, g10 = 10, g11 = 11, g12 = 12, g13 = 13, g14 = 14, g15 = 15, g16 = 16, g17 = 17, g18 = 18);
/// An execution-only support names no opcode's handlers (`quench_layer_enable_jit`).
#[cfg(not(feature = "jit"))]
pub mod lane_rows {}

/// Opcodes per `lane_rows` group.
pub const LANE_ROW_GROUP: u16 = TABLE_ROW;

static LANE_ROWS: std::sync::OnceLock<&'static [[Option<&'static LaneRow>; TABLE_ROW as usize]; TABLE_ROWS]> = std::sync::OnceLock::new();

/// Installs every opcode's lane handlers; the first installation holds.
pub fn install_all_lane_rows() {
    #[cfg(feature = "jit")]
    let _ = LANE_ROWS.set(&ALL_LANE_ROWS);
}

/// Installs only `rows` and the plain jump every view relies on: a program's image names the
/// rows of the opcodes its programs run and links no other opcode's handlers. An instruction of
/// another opcode has no lane handler and runs on the general path, as one whose fields do not
/// fit the lane does.
pub fn install_lane_rows(rows: &[&'static LaneRow]) {
    static JUMP: LaneRow = LaneRow::of(quench_runtime::staged::JUMP_ROW);
    let mut installed: Box<[[Option<&'static LaneRow>; TABLE_ROW as usize]; TABLE_ROWS]> = Box::new([[None; TABLE_ROW as usize]; TABLE_ROWS]);
    for row in rows.iter().copied().chain([&JUMP]) {
        installed[usize::from(row.0 / TABLE_ROW)][usize::from(row.0 % TABLE_ROW)] = Some(row);
    }
    let _ = LANE_ROWS.set(Box::leak(installed));
}

/// The installed handler of opcode `op` for one jump direction and accumulator field.
#[inline]
pub(crate) fn lane_handler(backward: bool, acc: u8, op: usize) -> Option<LaneHandler> {
    let group = LANE_ROWS.get()?.get(op / usize::from(TABLE_ROW))?;
    group[op % usize::from(TABLE_ROW)]?.1[usize::from(backward)][usize::from(acc)]
}
