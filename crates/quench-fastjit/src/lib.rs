//! FastJIT: stencils built by Cranelift from the same row definitions UltraJIT lowers, copied and
//! patched at run time (copy-and-patch). A row is emitted once through [`Flow`]: here every static
//! operand is a *hole* and every successor is a relocated `return_call`; UltraJIT implements
//! `Flow` with constants and blocks instead. Stencils use the `tail` calling convention with
//! (frame, context) in the first two argument registers.
//!
//! Layout of a generated stencil (x64): `push rbp; mov rbp,rsp` prologue, body, then per exit
//! `mov rsp,rbp; pop rbp; jmp rel32`. [`Stencil::strip`] verifies those exact bytes and removes
//! the frame pair from frameless stencils (the epilogue is overwritten by a 4-byte nop so the
//! stencil's internal branch displacements stay valid).
pub mod numeric;
pub mod row;
mod stencil_builder;
pub use stencil_builder::{Pins, StencilBuilder};

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::{
    self, AbiParam, ExtFuncData, ExternalName, Signature, UserExternalName,
    UserFuncName, types::{F64, I64},
};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{Context, FinalizedRelocTarget};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

/// Value registers a stencil takes after (frame, context): the tail convention passes them in
/// rdx, rcx, r8 and r9, so a chain of stencils keeps hot values out of memory.
pub const PINNED: usize = 4;
/// Double registers a stencil takes after the value registers: xmm0 to xmm3.
pub const PINNED_F: usize = 4;

/// Namespace of value holes (`symbol_value` → 64-bit immediate) and of successor holes.
const HOLE_VALUE: u32 = 1;
const HOLE_CODE: u32 = 2;
/// Successor hole indexes.
pub const NEXT: u32 = 0;
pub const TAKEN: u32 = 1;
/// Exit hole indexes start here: exit `kind` is hole `EXIT + kind`, so each guard kind reaches
/// its own exit stub (an `ExitSite` id), not one shared hole.
pub const EXIT: u32 = 2;

/// A successor hole: a colocated function whose call is a relocated `jmp rel32`.
fn successor(b: &mut FunctionBuilder, index: u32) -> ir::FuncRef {
    let name = b.func.declare_imported_user_function(UserExternalName::new(HOLE_CODE, index));
    let signature = b.import_signature(stencil_signature());
    b.import_function(ExtFuncData { name: ExternalName::User(name), signature, colocated: true, patchable: false })
}

/// The ISA stencils compile for: no PIC, frame pointers (x64 `return_call` requires them).
pub fn stencil_isa() -> Result<OwnedTargetIsa, String> {
    if !cfg!(target_arch = "x86_64") {
        return Err("stencil strip knows the x86-64 frame shapes only; aarch64 shapes are pending".into());
    }
    let mut flags = settings::builder();
    for (name, value) in [
        ("opt_level", "speed"),
        ("is_pic", "false"),
        ("enable_verifier", if std::env::var_os("STENCIL_VERIFY").is_some() { "true" } else { "false" }),
        ("preserve_frame_pointers", "true"),
        ("enable_probestack", "false"),
    ] {
        flags.set(name, value).map_err(|e| e.to_string())?;
    }
    cranelift_native::builder()?.finish(settings::Flags::new(flags)).map_err(|e| e.to_string())
}

fn stencil_signature() -> Signature {
    let mut signature = Signature::new(CallConv::Tail);
    signature.params.extend((0..PINNED + 2).map(|_| AbiParam::new(I64)));
    signature.params.extend((0..PINNED_F).map(|_| AbiParam::new(F64)));
    signature.returns.push(AbiParam::new(I64));
    signature
}

/// A relocation of a stencil: a 64-bit hole value, or a 32-bit pc-relative successor.
#[derive(Clone, Debug)]
pub struct Reloc {
    pub offset: u32,
    pub pc_relative: bool,
    pub namespace: u32,
    pub index: u32,
    pub addend: i64,
}

/// One compiled stencil, after [`Stencil::strip`].
#[derive(Clone, Debug)]
pub struct Stencil {
    pub code: Vec<u8>,
    pub relocs: Vec<Reloc>,
    /// Where the trailing tail (`jmp next`, and the nop that replaced a stripped frame teardown
    /// before it) starts when it ends the code, so adjacency drops it.
    pub next_jump: Option<usize>,
    /// Bytes of that tail.
    pub tail_len: usize,
    /// Bytes before stripping.
    pub raw_len: usize,
}

const PROLOGUE: [u8; 4] = [0x55, 0x48, 0x89, 0xe5];
const EPILOGUE: [u8; 4] = [0x48, 0x89, 0xec, 0x5d];
const NOP4: [u8; 4] = [0x0f, 0x1f, 0x40, 0x00];
/// `jmp rel32`: the opcode byte, then the displacement a relocation patches.
const JMP_OPCODE_LEN: usize = 1;
const JMP_REL32_LEN: usize = 5;
/// `sub rsp, imm` after the prologue marks a stencil with spill slots.
const SUB_RSP: [u8; 3] = [0x48, 0x83, 0xec];
const RET: u8 = 0xc3;
const REL32_LEN: usize = 4;
/// An exit tail: the nop that replaced the frame teardown, then `jmp rel32`.
const UNIT_LEN: usize = NOP4.len() + JMP_REL32_LEN;
const JCC_ESCAPE: u8 = 0x0f;
/// The high nibble of the second opcode byte of `jcc rel32`.
const JCC_NEAR: u8 = 0x80;
const JCC_REL32_LEN: usize = 6;
const JMP_REL32_OPCODE: u8 = 0xe9;

/// Lower one row into a stencil function: `emit` lowers the row through the builder.
fn lower(isa: &OwnedTargetIsa, emit: impl FnOnce(&mut StencilBuilder)) -> ir::Function {
    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 0), stencil_signature());
    let mut builder_context = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut builder_context);
    let (next, taken) = stencil_builder::new_successors(&mut b);
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    b.seal_block(entry);
    let params = b.block_params(entry).to_vec();
    let pins = Pins { ints: [params[2], params[3], params[4], params[5]], floats: [params[6], params[7], params[8], params[9]] };
    let mut stencil = StencilBuilder { b: &mut b, frame: params[0], context: params[1], pins, next, taken, exits: Vec::new(), pending: Vec::new() };
    emit(&mut stencil);
    stencil.finish();
    b.finalize(isa.frontend_config());
    func
}

/// The relocations of compiled code, by hole.
fn relocations(ctx: &Context) -> Result<Vec<Reloc>, String> {
    let compiled = ctx.compiled_code().ok_or("no code")?;
    let mut relocs = Vec::new();
    for r in compiled.buffer.relocs() {
        let FinalizedRelocTarget::ExternalName(ExternalName::User(name)) = &r.target else {
            return Err("stencil relocation to a non-hole target".into());
        };
        let name = &ctx.func.params.user_named_funcs()[*name];
        relocs.push(Reloc {
            offset: r.offset,
            pc_relative: format!("{:?}", r.kind).contains("PCRel"),
            namespace: name.namespace,
            index: name.index,
            addend: r.addend,
        });
    }
    Ok(relocs)
}

/// Compile one row into a stencil. `emit` lowers the row through the given flow.
pub fn generate(isa: &OwnedTargetIsa, emit: impl FnOnce(&mut StencilBuilder)) -> Result<Stencil, String> {
    let mut ctx = Context::for_function(lower(isa, emit));
    ctx.compile(&**isa, &mut ControlPlane::default()).map_err(|e| format!("{:?}", e.inner))?;
    let relocs = relocations(&ctx)?;
    Stencil::strip(ctx.compiled_code().ok_or("no code")?.code_buffer(), relocs)
}

impl Stencil {
    /// Verify the frame pair's exact bytes and remove it from a frameless stencil; a framed one
    /// (spill slots: `sub rsp` after the prologue) is kept whole.
    fn strip(code: &[u8], relocs: Vec<Reloc>) -> Result<Stencil, String> {
        let raw_len = code.len();
        if code.len() < PROLOGUE.len() + SUB_RSP.len() || code[..PROLOGUE.len()] != PROLOGUE {
            return Err(format!("unexpected stencil prologue {:02x?}", &code[..code.len().min(PROLOGUE.len() + SUB_RSP.len())]));
        }
        if code[PROLOGUE.len()..PROLOGUE.len() + SUB_RSP.len()] == SUB_RSP {
            return Ok(Stencil { next_jump: Self::next_jump(&relocs, code.len()), tail_len: JMP_REL32_LEN, code: code.to_vec(), relocs, raw_len });
        }
        let mut code = code.to_vec();
        for at in Self::epilogues(&relocs, &code) {
            if code[at..at + EPILOGUE.len()] != EPILOGUE {
                return Err(format!("unexpected stencil epilogue at {at}: {:02x?}", &code[at..at + EPILOGUE.len()]));
            }
            code[at..at + NOP4.len()].copy_from_slice(&NOP4);
        }
        code.drain(..PROLOGUE.len());
        let relocs: Vec<Reloc> = relocs.into_iter().map(|r| Reloc { offset: r.offset - PROLOGUE.len() as u32, ..r }).collect();
        let (code, relocs) = Self::hoist_exits(code, relocs);
        let jump = Self::next_jump(&relocs, code.len());
        // A frameless stencil's tail starts with the nop that replaced the frame teardown.
        let nop = jump.filter(|at| *at >= NOP4.len() && code[at - NOP4.len()..*at] == NOP4);
        let (next_jump, tail_len) = match nop {
            Some(at) => (Some(at - NOP4.len()), NOP4.len() + JMP_REL32_LEN),
            None => (jump, JMP_REL32_LEN),
        };
        Ok(Stencil { next_jump, tail_len, code, relocs, raw_len })
    }

    /// Moves the exit tails off the end: a frameless stencil's cold exits follow its hot
    /// `jmp next` as units of `nop4; jmp rel32`. Each jump in the hot part that lands on a unit
    /// becomes a jump relocated to that exit directly, and the units are cut, so the hot tail ends
    /// the stencil and adjacency drops it. A stencil of any other shape is kept whole.
    fn hoist_exits(code: Vec<u8>, relocs: Vec<Reloc>) -> (Vec<u8>, Vec<Reloc>) {
        let Some(next) = relocs.iter().find(|r| r.pc_relative && r.namespace == HOLE_CODE && r.index == NEXT) else {
            return (code, relocs);
        };
        let hot_end = next.offset as usize + REL32_LEN;
        let tail = code.len() - hot_end;
        let unit_at = |k: usize| hot_end + k * UNIT_LEN;
        let units = tail / UNIT_LEN;
        let unit_reloc = |k: usize| relocs.iter().find(|r| r.pc_relative && r.offset as usize == unit_at(k) + NOP4.len() + JMP_OPCODE_LEN);
        let shaped = tail % UNIT_LEN == 0
            && (0..units).all(|k| code[unit_at(k)..unit_at(k) + NOP4.len()] == NOP4 && unit_reloc(k).is_some());
        if tail == 0 || !shaped {
            return (code, relocs);
        }
        let relocated = |at: usize| relocs.iter().any(|r| (r.offset as usize..r.offset as usize + REL32_LEN).contains(&at));
        let mut code = code;
        let mut added = Vec::new();
        for p in 0..hot_end.saturating_sub(JMP_REL32_LEN) {
            let (length, disp_at) = match (code[p], code[p + 1] & 0xf0) {
                (JCC_ESCAPE, JCC_NEAR) => (JCC_REL32_LEN, p + 2),
                (JMP_REL32_OPCODE, _) => (JMP_REL32_LEN, p + 1),
                _ => continue,
            };
            if relocated(disp_at) || relocated(p) {
                continue;
            }
            let disp = i32::from_le_bytes([code[disp_at], code[disp_at + 1], code[disp_at + 2], code[disp_at + 3]]);
            let target = (p + length) as i64 + i64::from(disp);
            let Some(k) = (0..units).find(|k| unit_at(*k) as i64 == target) else { continue };
            let exit = unit_reloc(k).expect("shaped above");
            added.push(Reloc { offset: disp_at as u32, ..exit.clone() });
            code[disp_at..disp_at + REL32_LEN].fill(0);
        }
        code.truncate(hot_end);
        let mut relocs: Vec<Reloc> = relocs.into_iter().filter(|r| (r.offset as usize) < hot_end).collect();
        relocs.extend(added);
        (code, relocs)
    }

    /// Where each exit's frame teardown starts: before every `jmp rel32` and a closing `ret`.
    fn epilogues(relocs: &[Reloc], code: &[u8]) -> Vec<usize> {
        let mut ends: Vec<usize> =
            relocs.iter().filter(|r| r.pc_relative).map(|r| r.offset as usize - JMP_OPCODE_LEN - EPILOGUE.len()).collect();
        if code.last() == Some(&RET) {
            ends.push(code.len() - 1 - EPILOGUE.len());
        }
        ends
    }

    fn next_jump(relocs: &[Reloc], len: usize) -> Option<usize> {
        relocs
            .iter()
            .find(|r| r.pc_relative && r.namespace == HOLE_CODE && r.index == NEXT && r.offset as usize + JMP_REL32_LEN - JMP_OPCODE_LEN == len)
            .map(|r| r.offset as usize - JMP_OPCODE_LEN)
    }
}

/// Where a copied stencil's holes and successors resolve.
pub struct Placement<'a> {
    pub stencil: &'a Stencil,
    /// The value of value hole `index`.
    pub hole: &'a dyn Fn(u32) -> u64,
    /// The address of the `taken` successor, if the row has one.
    pub taken: Option<usize>,
    /// The address of the exit stub for this row's exits.
    pub exit: usize,
}

/// One row of a program: its stencil, hole values and branch target row.
pub struct Row<'a> {
    pub stencil: &'a Stencil,
    pub hole: &'a dyn Fn(u32) -> u64,
    pub taken: Option<usize>,
}

/// Each row's start offset and the code length once copied back to back, a `jmp next` dropped
/// where the next stencil follows.
fn layout(rows: &[Row]) -> (Vec<usize>, Vec<usize>, usize) {
    let (mut starts, mut sizes, mut len) = (Vec::new(), Vec::new(), 0);
    for (k, row) in rows.iter().enumerate() {
        starts.push(len);
        let drop = if row.stencil.next_jump.is_some() && k + 1 < rows.len() { row.stencil.tail_len } else { 0 };
        sizes.push(row.stencil.code.len() - drop);
        len += sizes[k];
    }
    (starts, sizes, len)
}

/// Copy `rows` into `out` back to back and patch their holes; `exit` gives each row's exit
/// stub address for an exit kind. Returns the code length and each row's start offset.
///
/// # Safety
/// `out` must be writable for the whole code.
pub unsafe fn copy_patch(rows: &[Row], out: *mut u8, exit: impl Fn(usize, u32) -> usize) -> (usize, Vec<usize>) {
    let (starts, sizes, len) = layout(rows);
    for (k, row) in rows.iter().enumerate() {
        // SAFETY: the caller guarantees `out` holds the layout computed above.
        unsafe {
            let base = out.add(starts[k]);
            std::ptr::copy_nonoverlapping(row.stencil.code.as_ptr(), base, sizes[k]);
            for r in row.stencil.relocs.iter().filter(|r| (r.offset as usize) < sizes[k]) {
                let place = base.add(r.offset as usize);
                if !r.pc_relative {
                    std::ptr::write_unaligned(place.cast::<u64>(), (row.hole)(r.index));
                    continue;
                }
                let target = match r.index {
                    NEXT => out as usize + starts[k + 1],
                    TAKEN => out as usize + starts[row.taken.expect("a branch row names its target")],
                    index => exit(k, index - EXIT),
                };
                std::ptr::write_unaligned(place.cast::<i32>(), (target as i64 + r.addend - place as i64) as i32);
            }
        }
    }
    (len, starts)
}

#[cfg(test)]
mod tests {
    use cranelift_codegen::ir::InstBuilder;
    use super::row::{Effects, MemKind, Op, Operand, Region, RowBuilder, Source, Ty};
    use super::*;

    /// `frame[hole0] += hole1`, guarded against overflow, then next: the same row body any builder lowers.
    fn add_row<B: RowBuilder<Exit = u32>>(b: &mut B) {
        let slot = Source::Frame(Operand::Hole(0));
        let (value, delta) = (b.read(slot, Ty::I64), b.operand(Operand::Hole(1), Ty::I64));
        let overflow = b.op(Op::Overflow, Ty::I64, &[value, delta], Effects::Pure);
        let fine = b.op(Op::Not, Ty::I64, &[overflow], Effects::Pure);
        b.guard(fine, 0);
        let sum = b.op(Op::Add, Ty::I64, &[value, delta], Effects::Pure);
        b.write(slot, sum);
        b.next();
    }

    /// `return frame[0]`.
    fn return_row<B: RowBuilder<Exit = u32>>(b: &mut B) {
        let _ = (Region::Frame, MemKind::Plain);
        let value = b.read(Source::Frame(Operand::Const(0)), Ty::I64);
        b.write(Source::Pinned(1), value);
    }

    #[test]
    fn copies_patches_and_chains_stencils() {
        let isa = stencil_isa().unwrap();
        let add = generate(&isa, |b| add_row(b)).unwrap();
        assert!(add.raw_len > add.code.len(), "the frame pair of a frameless stencil is stripped");
        // The final stencil returns the slot: `mov rax, [rdi]; ret`.
        let ret = generate(&isa, |b| {
            let value = b.read(Source::Frame(Operand::Const(0)), Ty::I64);
            b.b.ins().return_(&[value]);
        })
        .unwrap();
        let _ = return_row::<StencilBuilder>;
        let (one, two): (&dyn Fn(u32) -> u64, &dyn Fn(u32) -> u64) = (&|i| [0, 5][i as usize], &|i| [0, i64::MAX as u64][i as usize]);
        let none: &dyn Fn(u32) -> u64 = &|_| 0;
        unsafe {
            let memory = libc::mmap(std::ptr::null_mut(), 4096, 7, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0).cast::<u8>();
            assert_ne!(memory.cast(), libc::MAP_FAILED);
            let stub = memory.add(2048);
            std::ptr::copy_nonoverlapping([0xb8u8, 0xff, 0xff, 0xff, 0x7f, 0xc3].as_ptr(), stub, 6); // mov eax, 0x7fffffff; ret
            let row = |stencil, hole| Row { stencil, hole, taken: None };
            let rows = [row(&add, one), row(&add, one), row(&ret, none)];
            let (len, _) = copy_patch(&rows, memory, |_, _| stub as usize);
            assert!(len < 2048);
            let run: extern "C" fn(*mut i64, *mut u8) -> i64 = std::mem::transmute(memory);
            let mut frame = [1i64];
            assert_eq!(run(frame.as_mut_ptr(), std::ptr::null_mut()), 11, "two additions of 5");
            let rows = [row(&add, two), row(&add, two), row(&ret, none)];
            copy_patch(&rows, memory, |_, _| stub as usize);
            let mut frame = [1i64];
            assert_eq!(run(frame.as_mut_ptr(), std::ptr::null_mut()), 0x7fff_ffff, "overflow leaves through the exit stub");
            libc::munmap(memory.cast(), 4096);
        }
    }
}
