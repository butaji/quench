use super::*;
use quench_bjit::Reloc;

/// The machine code of a bare return on the host architecture.
#[cfg(target_arch = "x86_64")]
fn ret() -> Vec<u8> {
    vec![quench_bjit::RET]
}

/// The machine code of a bare return on the host architecture.
#[cfg(target_arch = "aarch64")]
fn ret() -> Vec<u8> {
    quench_bjit::RET.to_vec()
}

/// A row that is one `jmp rel32` through its first exit hole.
#[cfg(target_arch = "x86_64")]
fn exiting() -> Stencil {
    let exit = Reloc { offset: 1, class: quench_bjit::REL32, index: quench_bjit::EXIT, addend: -(std::mem::size_of::<i32>() as i64) };
    Stencil { code: vec![quench_bjit::stub::JMP_REL32, 0, 0, 0, 0], relocs: vec![exit], imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 5, hot_len: 5 }
}

/// A row that is one `b` through its first exit hole (its displacement counts from its start).
#[cfg(target_arch = "aarch64")]
fn exiting() -> Stencil {
    let exit = Reloc { offset: 0, class: quench_bjit::BRANCH26, index: quench_bjit::EXIT, addend: 0 };
    let code = quench_bjit::stub::A64_B.to_le_bytes().to_vec();
    Stencil { code, relocs: vec![exit], imm32: Vec::new(), next_jump: None, tail_len: 0, raw_len: 4, hot_len: 4 }
}

/// Where the row at `at` of `code` jumps to.
#[cfg(target_arch = "x86_64")]
fn jump_target(code: &[u8], at: usize) -> usize {
    let field = at + 1;
    let displacement = i32::from_le_bytes(code[field..field + std::mem::size_of::<i32>()].try_into().expect("a displacement"));
    (field as i64 + std::mem::size_of::<i32>() as i64 + i64::from(displacement)) as usize
}

/// Where the row at `at` of `code` branches to (its `b` displacement counts from `at`).
#[cfg(target_arch = "aarch64")]
fn jump_target(code: &[u8], at: usize) -> usize {
    let insn = u32::from_le_bytes(code[at..at + JUMP_LEN].try_into().expect("an instruction"));
    (at as i64 + quench_bjit::patch::branch_delta(insn, quench_bjit::class::BRANCH26)) as usize
}

/// A row's stub and a leave row lie after every hot row, the stub after the routines, and each
/// exit reaches its own target.
#[test]
fn exits_land_in_the_cold_area_behind_the_hot_rows() {
    let (hot, leave) = (exiting(), leave_row(ret()));
    let hole = [0u64; HOLES];
    let rows = [Placed { stencil: &hot, hole: &hole, taken: None, cold: false }, Placed { stencil: &hot, hole: &hole, taken: None, cold: false }, Placed { stencil: &leave, hole: &hole, taken: None, cold: false }];
    let exits = [vec![Target::Stub { pc: 7, routine: 0 }], vec![Target::Row(2)], Vec::new()];
    let routines = [ret()];
    let (code, ..) = place_rows(Ending::Body, &rows, (&exits, &routines), &[], 0, &[]).expect("a layout");
    let head = thunk(Ending::Body, 0, 0).expect("a thunk").len();
    let (second, leave_at) = (head + hot.code.len(), head + 2 * hot.code.len());
    let stub = leave_at + leave.code.len() + routines[0].len();
    assert_eq!(jump_target(&code, head), stub, "the first row's exit is its stub");
    assert_eq!(jump_target(&code, second), leave_at, "the second row's exit is the leave row");
    assert_eq!(code.len(), stub + EXIT_STUB_LEN);
}
