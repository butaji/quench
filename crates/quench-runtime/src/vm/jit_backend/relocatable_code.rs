//! Machine code with relocations: a function compiled with every run-time address a symbol,
//! linked by patching. Ahead-of-time images (`aot_image`) and lane regions
//! (`compiled::lane_region::relocs`) both link through `apply_relocation`.
use cranelift_codegen::{CompiledCode, RelocTarget, binemit::Reloc, ir};

/// AArch64 `b imm26`: the 26-bit word offset a branch patch fills.
pub(in crate::vm) const ARM64_BRANCH_IMMEDIATE_BITS: u32 = 26;
pub(in crate::vm) const ARM64_INSTRUCTION_BYTES: usize = 4;
const ARM64_INSTRUCTION_ALIGNMENT_BITS: u32 = 2;

/// AArch64 page size, and the immediate fields of `adrp` (a 21-bit signed page count
/// split into `immlo`/`immhi`) and `add` (an unsigned 12-bit offset within the page).
const ARM64_PAGE_BITS: u32 = 12;
const ARM64_PAGE_MASK: u64 = (1 << ARM64_PAGE_BITS) - 1;
const ARM64_ADRP_IMM_BITS: u32 = 21;
const ARM64_ADRP_IMMLO_SHIFT: u32 = 29;
const ARM64_ADRP_IMMHI_SHIFT: u32 = 5;
const ARM64_ADRP_IMMHI_BITS: u32 = 19;
/// The `adrp` bits that are not its immediate: the opcode and the destination register.
const ARM64_ADRP_FIXED_MASK: u32 = 0x9F00_001F;
const ARM64_ADD_IMM12_SHIFT: u32 = 10;
const ARM64_ADD_IMM12_MASK: u32 = 0xFFF << ARM64_ADD_IMM12_SHIFT;
/// `add xN, xN, #imm12` with no shift: the only form a page offset links into here.
const ARM64_ADD_OPCODE_MASK: u32 = 0x7F00_0000;
const ARM64_ADD_OPCODE: u32 = 0x1100_0000;

/// The relocation kinds `apply_relocation` fills, in the order an image file numbers them
/// (`aot_image::file` writes a kind as its index here), so a new kind is appended.
pub const PATCHABLE: [Reloc; 7] = [Reloc::Abs8, Reloc::X86PCRel4, Reloc::X86CallPCRel4, Reloc::X86CallPLTRel4,
    Reloc::Arm64Call, Reloc::Arm64Page21, Reloc::Arm64PageOff12];

/// Whether `apply_relocation` fills relocations of `kind`.
pub(in crate::vm) fn patchable(kind: Reloc) -> bool {
    PATCHABLE.contains(&kind)
}

/// One relocation of compiled code: the field at `offset` takes the address of symbol
/// `(namespace, key)` plus `addend`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relocation {
    pub offset: usize,
    pub kind: Reloc,
    pub namespace: u32,
    pub key: u32,
    pub addend: i64,
}

/// Position-independent machine code and the relocations that link it.
#[derive(Debug)]
pub struct RelocatableCode {
    pub code: Box<[u8]>,
    pub relocations: Box<[Relocation]>,
}

impl RelocatableCode {
    /// The code `compiled` holds, each relocation resolved to its symbol; `None` where one
    /// names anything but a user symbol or cannot be patched.
    pub fn extract(compiled: &CompiledCode, function: &ir::Function) -> Option<Self> {
        Self::extract_or_reason(compiled, function).ok()
    }

    /// `extract`, naming the first relocation it cannot carry: its target is no user symbol, the
    /// function's table does not hold its name, or its kind is one an image cannot patch.
    pub fn extract_or_reason(compiled: &CompiledCode, function: &ir::Function) -> Result<Self, String> {
        let names = function.params.user_named_funcs();
        let mut relocations = Vec::with_capacity(compiled.buffer.relocs().len());
        for relocation in compiled.buffer.relocs() {
            let RelocTarget::ExternalName(ir::ExternalName::User(name)) = &relocation.target else {
                return Err(format!("relocation {:#x} targets no user symbol ({:?})", relocation.offset, relocation.target));
            };
            let name = names.get(*name).ok_or_else(|| format!("relocation {:#x} names a symbol the function does not hold", relocation.offset))?;
            if !patchable(relocation.kind) {
                return Err(format!("relocation {:#x} names {:?}, which an image cannot patch", relocation.offset, relocation.kind));
            }
            relocations.push(Relocation {
                offset: usize::try_from(relocation.offset).map_err(|_| format!("relocation {:#x} is out of range", relocation.offset))?,
                kind: relocation.kind,
                namespace: name.namespace,
                key: name.index,
                addend: relocation.addend,
            });
        }
        Ok(Self { code: compiled.code_buffer().into(), relocations: relocations.into_boxed_slice() })
    }

    /// The code patched for `address`, each symbol resolved by `resolve`; an error names the
    /// first symbol it cannot resolve or relocation it cannot fill.
    pub fn link(&self, address: u64, resolve: impl FnMut(u32, u32) -> Option<u64>) -> Result<Vec<u8>, String> {
        self.link_through(address, resolve, |target| target)
    }

    /// `link`, where an `Arm64Call` the branch cannot reach is re-pointed at `veneer(target)`, a
    /// stub beside the code that reaches it (a 16-byte local veneer). A target in range keeps its
    /// direct branch.
    pub fn link_through(&self, address: u64, mut resolve: impl FnMut(u32, u32) -> Option<u64>,
        mut veneer: impl FnMut(u64) -> u64) -> Result<Vec<u8>, String> {
        let mut linked = self.code.to_vec();
        for relocation in &*self.relocations {
            let mut value = resolve(relocation.namespace, relocation.key)
                .ok_or_else(|| format!("unresolved symbol ({}, {})", relocation.namespace, relocation.key))?;
            // A branch into a code heap the loader placed away from the target takes its stub.
            let target = value.wrapping_add(relocation.addend as u64);
            if relocation.kind == Reloc::Arm64Call && !arm64_call_reaches(address + relocation.offset as u64, target) {
                value = veneer(target).wrapping_sub(relocation.addend as u64);
            }
            apply_relocation(&mut linked, address, (relocation.offset, relocation.kind, relocation.addend), value)
                .map_err(|error| format!("symbol ({}, {}) at {value:#x} (relocation {:#x} of {:#x}): {error}",
                    relocation.namespace, relocation.key, relocation.offset, address))?;
        }
        Ok(linked)
    }
}

/// Whether an `Arm64Call` at `from` reaches `to` directly: an aligned displacement inside the
/// branch's signed immediate, which a 16-byte veneer stands in for beyond it.
pub(crate) fn arm64_call_reaches(from: u64, to: u64) -> bool {
    let delta = to as i64 - from as i64;
    let words = delta >> ARM64_INSTRUCTION_ALIGNMENT_BITS;
    let limit = 1_i64 << (ARM64_BRANCH_IMMEDIATE_BITS - 1);
    delta & ((1 << ARM64_INSTRUCTION_ALIGNMENT_BITS) - 1) == 0 && (-limit..limit).contains(&words)
}

/// Fills the field at `offset` of `code`, which will run at `address`, with `value` as
/// relocation `kind` with `addend` says.
pub(in crate::vm) fn apply_relocation(code: &mut [u8], address: u64, (offset, kind, addend): (usize, Reloc, i64), value: u64) -> Result<(), String> {
    let site = address.checked_add(offset as u64).ok_or("relocation address overflow")?;
    let relative = || -> Result<i64, String> {
        (value as i64)
            .checked_add(addend)
            .and_then(|target| target.checked_sub(site as i64))
            .ok_or_else(|| "relocation displacement overflow".to_owned())
    };
    let bytes = code.get_mut(offset..).ok_or("relocation is outside its code")?;
    match kind {
        Reloc::Abs8 => {
            let word = value.wrapping_add(addend as u64).to_le_bytes();
            bytes.get_mut(..word.len()).ok_or("relocation is outside its code")?.copy_from_slice(&word);
        }
        Reloc::X86PCRel4 | Reloc::X86CallPCRel4 | Reloc::X86CallPLTRel4 => {
            let displacement = i32::try_from(relative()?).map_err(|_| "relocation is out of rel32 range")?.to_le_bytes();
            bytes.get_mut(..displacement.len()).ok_or("relocation is outside its code")?.copy_from_slice(&displacement);
        }
        Reloc::Arm64Call => {
            let delta = relative()?;
            let words = delta >> ARM64_INSTRUCTION_ALIGNMENT_BITS;
            let limit = 1_i64 << (ARM64_BRANCH_IMMEDIATE_BITS - 1);
            if delta & ((1 << ARM64_INSTRUCTION_ALIGNMENT_BITS) - 1) != 0 || !(-limit..limit).contains(&words) {
                return Err("relocation is out of branch range".into());
            }
            let field = bytes.get_mut(..ARM64_INSTRUCTION_BYTES).ok_or("relocation is outside its code")?;
            let mask = (1_u32 << ARM64_BRANCH_IMMEDIATE_BITS) - 1;
            let word = u32::from_le_bytes((&*field).try_into().expect("four-byte instruction"));
            field.copy_from_slice(&((word & !mask) | (words as u32 & mask)).to_le_bytes());
        }
        Reloc::Arm64Page21 => {
            let target = value.wrapping_add(addend as u64);
            let page = |address: u64| (address >> ARM64_PAGE_BITS) as i64;
            let imm = page(target).wrapping_sub(page(site));
            let limit = 1_i64 << (ARM64_ADRP_IMM_BITS - 1);
            if !(-limit..limit).contains(&imm) {
                return Err("relocation is out of page range".into());
            }
            let field = bytes.get_mut(..ARM64_INSTRUCTION_BYTES).ok_or("relocation is outside its code")?;
            let word = u32::from_le_bytes((&*field).try_into().expect("four-byte instruction"));
            let immlo = (imm as u32 & 0x3) << ARM64_ADRP_IMMLO_SHIFT;
            let immhi = ((imm as u32 >> 2) & ((1 << ARM64_ADRP_IMMHI_BITS) - 1)) << ARM64_ADRP_IMMHI_SHIFT;
            field.copy_from_slice(&((word & ARM64_ADRP_FIXED_MASK) | immlo | immhi).to_le_bytes());
        }
        Reloc::Arm64PageOff12 => {
            let target = value.wrapping_add(addend as u64);
            let field = bytes.get_mut(..ARM64_INSTRUCTION_BYTES).ok_or("relocation is outside its code")?;
            let word = u32::from_le_bytes((&*field).try_into().expect("four-byte instruction"));
            if word & ARM64_ADD_OPCODE_MASK != ARM64_ADD_OPCODE {
                return Err("a page-offset relocation fills an add immediate".into());
            }
            let imm12 = (target & ARM64_PAGE_MASK) as u32;
            field.copy_from_slice(&((word & !ARM64_ADD_IMM12_MASK) | (imm12 << ARM64_ADD_IMM12_SHIFT)).to_le_bytes());
        }
        _ => return Err("relocation kind is not patchable".into()),
    }
    Ok(())
}
