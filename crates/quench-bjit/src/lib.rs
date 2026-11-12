//! BaseJIT: stencils built by Cranelift from the same row definitions UltraJIT lowers, copied and
//! patched at run time (copy-and-patch). A row is emitted once through [`Flow`]: here every static
//! operand is a *hole* and every successor is a relocated `return_call`; UltraJIT implements
//! `Flow` with constants and blocks instead. Stencils use the `tail` calling convention with
//! (frame, context) in the first two argument registers.
//!
//! Layout of a generated stencil (x64): `push rbp; mov rbp,rsp` prologue, body, then per exit
//! `mov rsp,rbp; pop rbp; jmp rel32`. [`Stencil::strip`] verifies those exact bytes and removes
//! the frame pair from frameless stencils (the epilogue is overwritten by a 4-byte nop so the
//! stencil's internal branch displacements stay valid).
// Transitional: the Cranelift stencil generator stays only as the differential oracle until deleted.
#![cfg_attr(not(test), allow(dead_code, unused_imports))]
mod hoist;
mod patch;
pub mod numeric;
pub mod row;
mod stencil_builder;
pub use patch::{HOLES, Imm32};
pub use stencil_builder::{Pins, StencilBuilder};
use patch::Mode;

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::{
    self, AbiParam, ExtFuncData, ExternalName, Signature, UserExternalName,
    UserFuncName, types::{F64, I64},
};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_codegen::RelocTarget;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

/// Value registers a stencil takes after (frame, context): the tail convention passes them in
/// rdx, rcx, r8 and r9, so a chain of stencils keeps hot values out of memory.
/// Arguments before the pinned registers: the frame and the context.
const FIXED_ARGS: usize = 2;
pub const PINNED: usize = 4;
/// Double registers a stencil takes after the value registers: xmm0 to xmm7.
pub const PINNED_F: usize = 8;

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
    isa_with_unwind(false)
}

/// The stencil ISA; `unwind_info` true gives the stock tail convention, which saves the
/// callee-saved registers it uses (the vendored x64 ABI change keys on this one setting, V6).
fn isa_with_unwind(unwind_info: bool) -> Result<OwnedTargetIsa, String> {
    if !cfg!(target_arch = "x86_64") {
        return Err("stencil strip knows the x86-64 frame shapes only; aarch64 shapes are pending".into());
    }
    let mut flags = settings::builder();
    for (name, value) in [
        ("opt_level", "speed"),
        ("is_pic", "false"),
        ("enable_verifier", if std::env::var_os("STENCIL_VERIFY").is_some() { "true" } else { "false" }),
        ("preserve_frame_pointers", "true"),
        ("unwind_info", if unwind_info { "true" } else { "false" }),
        ("enable_probestack", "false"),
    ] {
        flags.set(name, value).map_err(|e| e.to_string())?;
    }
    cranelift_native::builder()?.finish(settings::Flags::new(flags)).map_err(|e| e.to_string())
}

fn stencil_signature() -> Signature {
    let mut signature = Signature::new(CallConv::Tail);
    signature.params.extend((0..PINNED + FIXED_ARGS).map(|_| AbiParam::new(I64)));
    signature.params.extend((0..PINNED_F).map(|_| AbiParam::new(F64)));
    signature.returns.push(AbiParam::new(I64));
    signature
}

/// `ret`: what a leave routine ends with.
pub const RET: u8 = 0xc3;

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
    /// The code's 32-bit hole fields.
    pub imm32: Vec<Imm32>,
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
const REL32_LEN: usize = 4;
/// An exit tail: the nop that replaced the frame teardown, then `jmp rel32`.
const UNIT_LEN: usize = NOP4.len() + JMP_REL32_LEN;
const JCC_ESCAPE: u8 = 0x0f;
/// The high nibble of the second opcode byte of `jcc rel32`.
const JCC_NEAR: u8 = 0x80;
const JCC_REL32_LEN: usize = 6;
const JMP_REL32_OPCODE: u8 = 0xe9;

/// Lower one row into a stencil function: `emit` lowers the row through the builder.
fn lower(isa: &OwnedTargetIsa, emit: &impl Fn(&mut StencilBuilder), mode: Mode) -> (ir::Function, u8) {
    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 0), stencil_signature());
    let mut builder_context = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut builder_context);
    let (next, taken) = stencil_builder::new_successors(&mut b);
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    b.seal_block(entry);
    let params = b.block_params(entry).to_vec();
    let pins = Pins { ints: std::array::from_fn(|k| params[FIXED_ARGS + k]), floats: std::array::from_fn(|k| params[PINNED + FIXED_ARGS + k]) };
    let mut stencil = StencilBuilder { b: &mut b, frame: params[0], context: params[1], pins, next, taken, exits: Vec::new(), mode, used: 0, pending: Vec::new() };
    emit(&mut stencil);
    stencil.finish();
    let used = stencil.used;
    b.finalize(isa.frontend_config());
    (func, used)
}

/// The relocations of compiled code, by hole.
fn relocations(ctx: &Context) -> Result<Vec<Reloc>, String> {
    let compiled = ctx.compiled_code().ok_or("no code")?;
    let mut relocs = Vec::new();
    for r in compiled.buffer.relocs() {
        let RelocTarget::ExternalName(ExternalName::User(name)) = &r.target else {
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

fn compile(isa: &OwnedTargetIsa, emit: &impl Fn(&mut StencilBuilder), mode: Mode) -> Result<(Stencil, u8), String> {
    let (function, used) = lower(isa, emit, mode);
    let mut ctx = Context::for_function(function);
    ctx.compile(&**isa, &mut ControlPlane::default()).map_err(|e| format!("{:?}", e.inner))?;
    let relocs = relocations(&ctx)?;
    Ok((Stencil::strip(ctx.compiled_code().ok_or("no code")?.code_buffer(), relocs)?, used))
}

/// Compile one row into a stencil. `emit` lowers the row through the given builder with a
/// sentinel for each 32-bit hole; the fields that hold a sentinel (negated, plus a small
/// constant) are patched in place. Under test and `STENCIL_VERIFY` a second pass with other
/// sentinels must explain exactly the same fields, else the holes are 64-bit immediates.
pub fn generate(isa: &OwnedTargetIsa, emit: impl Fn(&mut StencilBuilder)) -> Result<Stencil, String> {
    let (first, used) = compile(isa, &emit, Mode::Sentinel(0))?;
    let found = patch::scan(&first, used);
    let verify = cfg!(test) || std::env::var_os("STENCIL_VERIFY").is_some();
    // A hole the scan cannot find (dead, or folded into other code) needs the second pass to say which.
    let second = if found.is_none() || verify { Some(compile(isa, &emit, Mode::Sentinel(1))?.0) } else { None };
    let explained = second.as_ref().and_then(|second| patch::fields(&first, second));
    if verify && found.is_some() {
        assert_eq!(found, explained, "the sentinel scan must agree with the two-pass diff");
    }
    match found.or(explained) {
        Some(imm32) => Ok(Stencil { imm32, ..first }),
        None => {
            WIDE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            compile(isa, &emit, Mode::Wide).map(|(stencil, _)| stencil)
        }
    }
}

static WIDE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Stencils whose holes stayed 64-bit immediates because the sentinel passes did not explain them.
pub fn wide_stencils() -> u64 {
    WIDE.load(std::sync::atomic::Ordering::Relaxed)
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
            return Ok(Stencil { next_jump: Self::next_jump(&relocs, code.len()), tail_len: JMP_REL32_LEN, code: code.to_vec(), relocs, imm32: Vec::new(), raw_len });
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
        let (code, relocs) = hoist::hoist_exits(code, relocs);
        let jump = Self::next_jump(&relocs, code.len());
        // A frameless stencil's tail starts with the nop that replaced the frame teardown.
        let nop = jump.filter(|at| *at >= NOP4.len() && code[at - NOP4.len()..*at] == NOP4);
        let (next_jump, tail_len) = match nop {
            Some(at) => (Some(at - NOP4.len()), NOP4.len() + JMP_REL32_LEN),
            None => (jump, JMP_REL32_LEN),
        };
        Ok(Stencil { next_jump, tail_len, code, relocs, imm32: Vec::new(), raw_len })
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
            for field in row.stencil.imm32.iter().filter(|f| (f.offset as usize) < sizes[k]) {
                std::ptr::write_unaligned(base.add(field.offset as usize).cast::<u32>(), field.value((row.hole)(field.index) as u32));
            }
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
        let (value, delta) = (b.read(slot, Ty::I64), b.operand(Operand::Hole(1), Ty::Word));
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

    /// A row with more live values than caller-saved registers.
    fn pressure_row(b: &mut StencilBuilder) {
        let values: Vec<_> = (0..14).map(|k| b.b.ins().load(I64, cranelift_codegen::ir::MemFlagsData::new(), b.frame, k * 8)).collect();
        let sum = values.iter().fold(values[0], |sum, value| b.b.ins().iadd(sum, *value));
        b.b.ins().store(cranelift_codegen::ir::MemFlagsData::new(), sum, b.frame, 0);
        // Every loaded value is live until here.
        for (k, value) in values.iter().enumerate() {
            let place = (k as i32 + 1) * 8;
            b.b.ins().store(cranelift_codegen::ir::MemFlagsData::new(), *value, b.frame, place);
        }
        b.next();
    }

    /// V6: the vendored `CallConv::Tail if !unwind_info` change is what keeps stencils free of
    /// callee-saved saves; a vendor update that drops it fails here.
    #[test]
    fn stencil_isa_saves_no_callee_saved_registers() {
        // `mov [rsp+disp8], rbx`: how the prologue saves the first callee-saved register.
        const SAVE_RBX: [u8; 4] = [0x48, 0x89, 0x5c, 0x24];
        const PROLOGUE_SPAN: usize = 24;
        let saves = |code: &[u8]| code[..PROLOGUE_SPAN].windows(SAVE_RBX.len()).any(|w| w == SAVE_RBX);
        let (saving, saved) = (isa_with_unwind(true).unwrap(), stencil_isa().unwrap());
        let raw = |isa: &OwnedTargetIsa| {
            let (function, _) = lower(isa, &pressure_row, Mode::Sentinel(0));
            let mut ctx = Context::for_function(function);
            ctx.compile(&**isa, &mut ControlPlane::default()).unwrap();
            ctx.compiled_code().unwrap().code_buffer().to_vec()
        };
        let (with, without) = (raw(&saving), raw(&saved));
        assert!(saves(&with), "the stock tail convention saves rbx under pressure");
        assert!(!saves(&without), "the stencil ISA saves nothing");
        assert!(generate(&saved, pressure_row).is_ok());
    }

    /// A hole used as something other than `hole + constant` is not patched in place: the stencil
    /// keeps a 64-bit hole, and the sentinel scan alone never accepts it silently.
    #[test]
    fn a_folded_hole_falls_back_to_a_wide_immediate() {
        let isa = stencil_isa().unwrap();
        let before = wide_stencils();
        let stencil = generate(&isa, |b| {
            let hole = b.operand(Operand::Hole(0), Ty::I64);
            let scaled = b.b.ins().imul_imm_s(hole, 0x1_0001);
            b.b.ins().store(cranelift_codegen::ir::MemFlagsData::new(), scaled, b.frame, 0);
            b.next();
        })
        .unwrap();
        assert!(stencil.imm32.is_empty(), "no field is patched");
        assert!(stencil.relocs.iter().any(|r| !r.pc_relative && r.index == 0), "the hole is a 64-bit relocation");
        assert!(wide_stencils() > before);
    }
}

/// Stencils compiled by rustc from `stencils/rows.rs` (see `build.rs`), as [`Stencil`]s.
pub mod rustc {
    use super::{Imm32, Reloc, Stencil, HOLE_CODE, NEXT};

    /// A stencil as `build.rs` read it from the object: relocations as (offset, class, index, addend).
    pub(crate) struct RawStencil {
        pub(crate) name: &'static str,
        pub(crate) code: &'static [u8],
        pub(crate) relocs: &'static [(u16, u8, u8, i32)],
    }

    include!(concat!(env!("OUT_DIR"), "/rustc_stencils.rs"));

    /// The stencil of the row function `name`, if the object has it.
    pub fn stencil(name: &str) -> Option<Stencil> {
        let raw = &RAW[RAW.binary_search_by(|raw| raw.name.cmp(name)).ok()?];
        let (mut relocs, mut imm32) = (Vec::new(), Vec::new());
        for &(offset, class, index, addend) in raw.relocs {
            let (offset, index) = (u32::from(offset), u32::from(index));
            match class {
                0 => imm32.push(Imm32 { offset, index, negate: false, addend: addend as u32 }),
                1 => relocs.push(Reloc { offset, pc_relative: true, namespace: HOLE_CODE, index, addend: i64::from(addend) }),
                _ => relocs.push(Reloc { offset, pc_relative: false, namespace: super::HOLE_VALUE, index, addend: i64::from(addend) }),
            }
        }
        let len = raw.code.len();
        let next_jump = relocs.iter().find(|r| r.index == NEXT && r.offset as usize + REL32 == len).map(|r| r.offset as usize - 1);
        Some(Stencil { code: raw.code.to_vec(), relocs, imm32, next_jump, tail_len: super::JMP_REL32_LEN, raw_len: len })
    }

    const REL32: usize = 4;
}

#[cfg(test)]
mod rustc_tests {
    use super::{numeric::library, rustc};

    /// Every row the library names was built, and canonicalizing is idempotent.
    #[test]
    fn the_library_is_built() {
        for code in library::library() {
            assert_eq!(library::canonical(code), code);
            assert!(rustc::stencil(&library::name(code)).is_some(), "{code:?}");
        }
        for code in library::heap_library() {
            assert_eq!(library::heap_canonical(code), code);
            assert!(rustc::stencil(&library::heap_name(code)).is_some(), "{code:?}");
        }
    }
}
