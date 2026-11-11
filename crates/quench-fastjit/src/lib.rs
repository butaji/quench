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
use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::{
    self, AbiParam, ExtFuncData, ExternalName, GlobalValueData, InstBuilder, Signature, UserExternalName,
    UserFuncName, types::I64,
};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{Context, FinalizedRelocTarget};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

/// Namespace of value holes (`symbol_value` → 64-bit immediate) and of successor holes.
const HOLE_VALUE: u32 = 1;
const HOLE_CODE: u32 = 2;
/// Successor hole indexes.
pub const NEXT: u32 = 0;
pub const TAKEN: u32 = 1;
pub const EXIT: u32 = 2;

/// An operand of a row: a patched hole in a stencil, a constant in UltraJIT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Hole(u32),
    Const(i64),
}

/// What differs between lowering a row as a stencil and as UltraJIT code.
pub trait Flow {
    /// The value of an operand.
    fn imm(&mut self, b: &mut FunctionBuilder, operand: Operand) -> ir::Value;
    /// Continue with the next row.
    fn next(&mut self, b: &mut FunctionBuilder, frame: ir::Value, context: ir::Value);
    /// Continue at the branch target when `condition` is nonzero, else fall through.
    fn taken(&mut self, b: &mut FunctionBuilder, frame: ir::Value, context: ir::Value, condition: ir::Value);
    /// Leave through exit `kind` when `condition` is nonzero.
    fn exit(&mut self, b: &mut FunctionBuilder, frame: ir::Value, context: ir::Value, condition: ir::Value, kind: u32);
}

/// The ISA stencils compile for: no PIC, frame pointers (x64 `return_call` requires them).
pub fn stencil_isa() -> Result<OwnedTargetIsa, String> {
    let mut flags = settings::builder();
    for (name, value) in [
        ("opt_level", "speed"),
        ("is_pic", "false"),
        ("enable_verifier", "false"),
        ("preserve_frame_pointers", "true"),
        ("enable_probestack", "false"),
    ] {
        flags.set(name, value).map_err(|e| e.to_string())?;
    }
    cranelift_native::builder()?.finish(settings::Flags::new(flags)).map_err(|e| e.to_string())
}

fn stencil_signature() -> Signature {
    let mut signature = Signature::new(CallConv::Tail);
    signature.params.extend([AbiParam::new(I64), AbiParam::new(I64)]);
    signature.returns.push(AbiParam::new(I64));
    signature
}

struct StencilFlow {
    next: ir::FuncRef,
    taken: ir::FuncRef,
    exit: ir::FuncRef,
}

impl Flow for StencilFlow {
    fn imm(&mut self, b: &mut FunctionBuilder, operand: Operand) -> ir::Value {
        match operand {
            Operand::Const(value) => b.ins().iconst(I64, value),
            Operand::Hole(index) => {
                let name = b.func.declare_imported_user_function(UserExternalName::new(HOLE_VALUE, index));
                let symbol = b.func.create_global_value(GlobalValueData::Symbol {
                    name: ExternalName::User(name),
                    offset: 0.into(),
                    colocated: false,
                    tls: false,
                });
                b.ins().symbol_value(I64, symbol)
            }
        }
    }
    fn next(&mut self, b: &mut FunctionBuilder, frame: ir::Value, context: ir::Value) {
        b.ins().return_call(self.next, &[frame, context]);
    }
    fn taken(&mut self, b: &mut FunctionBuilder, frame: ir::Value, context: ir::Value, condition: ir::Value) {
        let (jump, rest) = (b.create_block(), b.create_block());
        b.ins().brif(condition, jump, &[], rest, &[]);
        b.switch_to_block(jump);
        b.ins().return_call(self.taken, &[frame, context]);
        b.seal_block(jump);
        b.switch_to_block(rest);
        b.seal_block(rest);
    }
    fn exit(&mut self, b: &mut FunctionBuilder, frame: ir::Value, context: ir::Value, condition: ir::Value, _kind: u32) {
        let (leave, rest) = (b.create_block(), b.create_block());
        b.set_cold_block(leave);
        b.ins().brif(condition, leave, &[], rest, &[]);
        b.switch_to_block(leave);
        b.ins().return_call(self.exit, &[frame, context]);
        b.seal_block(leave);
        b.switch_to_block(rest);
        b.seal_block(rest);
    }
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
    /// Where the trailing `jmp next` starts when it ends the code (so adjacency drops it).
    pub next_jump: Option<usize>,
    /// Bytes before stripping.
    pub raw_len: usize,
}

const PROLOGUE: [u8; 4] = [0x55, 0x48, 0x89, 0xe5];
const EPILOGUE: [u8; 4] = [0x48, 0x89, 0xec, 0x5d];
const NOP4: [u8; 4] = [0x0f, 0x1f, 0x40, 0x00];

/// Compile one row into a stencil. `emit` lowers the row through the given flow.
pub fn generate(
    isa: &OwnedTargetIsa,
    emit: impl FnOnce(&mut FunctionBuilder, &mut dyn Flow, ir::Value, ir::Value),
) -> Result<Stencil, String> {
    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, 0), stencil_signature());
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut func, &mut builder_context);
        let successor = |b: &mut FunctionBuilder, index: u32| {
            let name = b.func.declare_imported_user_function(UserExternalName::new(HOLE_CODE, index));
            let signature = b.import_signature(stencil_signature());
            b.import_function(ExtFuncData { name: ExternalName::User(name), signature, colocated: true, patchable: false })
        };
        let mut flow = StencilFlow { next: successor(&mut b, NEXT), taken: successor(&mut b, TAKEN), exit: successor(&mut b, EXIT) };
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let (frame, context) = (b.block_params(entry)[0], b.block_params(entry)[1]);
        emit(&mut b, &mut flow, frame, context);
        b.finalize(isa.frontend_config());
    }
    let mut ctx = Context::for_function(func);
    ctx.compile(&**isa, &mut ControlPlane::default()).map_err(|e| format!("{:?}", e.inner))?;
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
    Stencil::strip(compiled.code_buffer(), relocs)
}

impl Stencil {
    /// Verify the frame pair's exact bytes and remove it from a frameless stencil; a framed one
    /// (spill slots: `sub rsp` after the prologue) is kept whole.
    fn strip(code: &[u8], relocs: Vec<Reloc>) -> Result<Stencil, String> {
        let raw_len = code.len();
        if code.len() < 8 || code[..4] != PROLOGUE {
            return Err(format!("unexpected stencil prologue {:02x?}", &code[..code.len().min(8)]));
        }
        if code[4..7] == [0x48, 0x83, 0xec] {
            return Ok(Stencil { next_jump: Self::next_jump(&relocs, code.len()), code: code.to_vec(), relocs, raw_len });
        }
        let mut code = code.to_vec();
        let mut ends: Vec<usize> = Vec::new();
        for r in relocs.iter().filter(|r| r.pc_relative) {
            ends.push(r.offset as usize - 1 - EPILOGUE.len()); // `jmp rel32`: opcode byte, then the displacement
        }
        if code.last() == Some(&0xc3) {
            ends.push(code.len() - 1 - EPILOGUE.len());
        }
        for at in ends {
            if code[at..at + 4] != EPILOGUE {
                return Err(format!("unexpected stencil epilogue at {at}: {:02x?}", &code[at..at + 4]));
            }
            code[at..at + 4].copy_from_slice(&NOP4);
        }
        code.drain(..PROLOGUE.len());
        let relocs: Vec<Reloc> = relocs.into_iter().map(|r| Reloc { offset: r.offset - PROLOGUE.len() as u32, ..r }).collect();
        Ok(Stencil { next_jump: Self::next_jump(&relocs, code.len()), code, relocs, raw_len })
    }

    fn next_jump(relocs: &[Reloc], len: usize) -> Option<usize> {
        relocs
            .iter()
            .find(|r| r.pc_relative && r.namespace == HOLE_CODE && r.index == NEXT && r.offset as usize + 4 == len)
            .map(|r| r.offset as usize - 1)
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

/// Copy `rows` into `out` back to back (a `jmp next` is dropped when the next stencil follows)
/// and patch their holes. `exit` gives each row's exit stub address; `taken_row` its branch row.
/// Returns the code length and each row's start offset.
///
/// # Safety
/// `out` must be writable for the whole code.
pub unsafe fn copy_patch(
    rows: &[(&Stencil, &dyn Fn(u32) -> u64, Option<usize>)],
    out: *mut u8,
    exit: impl Fn(usize) -> usize,
) -> (usize, Vec<usize>) {
    let mut starts = Vec::with_capacity(rows.len());
    let mut sizes = Vec::with_capacity(rows.len());
    let mut len = 0;
    for (k, (stencil, _, _)) in rows.iter().enumerate() {
        starts.push(len);
        let drop = if stencil.next_jump.is_some() && k + 1 < rows.len() { 5 } else { 0 };
        sizes.push(stencil.code.len() - drop);
        len += sizes[k];
    }
    for (k, (stencil, hole, taken)) in rows.iter().enumerate() {
        // SAFETY: caller guarantees `out` holds the layout computed above.
        unsafe {
            let base = out.add(starts[k]);
            std::ptr::copy_nonoverlapping(stencil.code.as_ptr(), base, sizes[k]);
            for r in &stencil.relocs {
                let at = r.offset as usize;
                if at >= sizes[k] {
                    continue; // inside a dropped fallthrough jump
                }
                let place = base.add(at);
                if !r.pc_relative {
                    std::ptr::write_unaligned(place.cast::<u64>(), hole(r.index));
                } else {
                    let target = match r.index {
                        NEXT => out as usize + starts[k + 1],
                        TAKEN => out as usize + starts[taken.expect("a branch row names its target")],
                        _ => exit(k),
                    };
                    std::ptr::write_unaligned(place.cast::<i32>(), (target as i64 + r.addend - place as i64) as i32);
                }
            }
        }
    }
    (len, starts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::ir::MemFlagsData;

    /// `frame[hole0 / 8] += hole1`, guarded against i64 overflow, then next.
    fn add_row(b: &mut FunctionBuilder, flow: &mut dyn Flow, frame: ir::Value, context: ir::Value) {
        let (slot, delta) = (flow.imm(b, Operand::Hole(0)), flow.imm(b, Operand::Hole(1)));
        let address = b.ins().iadd(frame, slot);
        let value = b.ins().load(I64, MemFlagsData::trusted(), address, 0);
        let (sum, overflow) = b.ins().sadd_overflow(value, delta);
        flow.exit(b, frame, context, overflow, 0);
        b.ins().store(MemFlagsData::trusted(), sum, address, 0);
        flow.next(b, frame, context);
    }

    #[test]
    fn copies_patches_and_chains_stencils() {
        let isa = stencil_isa().unwrap();
        let add = generate(&isa, add_row).unwrap();
        assert!(add.raw_len > add.code.len(), "the frame pair of a frameless stencil is stripped");
        // The final stencil returns the slot: `mov rax, [rdi]; ret`.
        let ret = generate(&isa, |b, _, frame, _| {
            let value = b.ins().load(I64, MemFlagsData::trusted(), frame, 0);
            b.ins().return_(&[value]);
        })
        .unwrap();
        let (one, two): (&dyn Fn(u32) -> u64, &dyn Fn(u32) -> u64) = (&|i| [0, 5][i as usize], &|i| [0, i64::MAX as u64][i as usize]);
        let none: &dyn Fn(u32) -> u64 = &|_| 0;
        unsafe {
            let memory = libc::mmap(std::ptr::null_mut(), 4096, 7, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0).cast::<u8>();
            assert_ne!(memory.cast(), libc::MAP_FAILED);
            let stub = memory.add(2048);
            std::ptr::copy_nonoverlapping([0xb8u8, 0xff, 0xff, 0xff, 0x7f, 0xc3].as_ptr(), stub, 6); // mov eax, 0x7fffffff; ret
            let rows = [(&add, one, None), (&add, one, None), (&ret, none, None)];
            let (len, _) = copy_patch(&rows, memory, |_| stub as usize);
            assert!(len < 2048);
            let run: extern "C" fn(*mut i64, *mut u8) -> i64 = std::mem::transmute(memory);
            let mut frame = [1i64];
            assert_eq!(run(frame.as_mut_ptr(), std::ptr::null_mut()), 11, "two additions of 5");
            let rows = [(&add, two, None), (&add, two, None), (&ret, none, None)];
            copy_patch(&rows, memory, |_| stub as usize);
            let mut frame = [1i64];
            assert_eq!(run(frame.as_mut_ptr(), std::ptr::null_mut()), 0x7fff_ffff, "overflow leaves through the exit stub");
            libc::munmap(memory.cast(), 4096);
        }
    }
}
