//! Machine code with relocations: a function compiled with every run-time address a symbol,
//! linked by patching. Stencils (`native_stencil`) and ahead-of-time lane regions
//! (`compiled::lane_region::relocs`) both link through `apply_relocation`.
use cranelift_codegen::{CompiledCode, RelocTarget, binemit::Reloc, ir};

/// AArch64 `b imm26`: the 26-bit word offset a branch patch fills.
pub(in crate::vm) const ARM64_BRANCH_IMMEDIATE_BITS: u32 = 26;
pub(in crate::vm) const ARM64_INSTRUCTION_BYTES: usize = 4;
const ARM64_INSTRUCTION_ALIGNMENT_BITS: u32 = 2;

/// Whether `apply_relocation` fills relocations of `kind`.
pub(in crate::vm) fn patchable(kind: Reloc) -> bool {
    matches!(
        kind,
        Reloc::Abs8 | Reloc::X86PCRel4 | Reloc::X86CallPCRel4 | Reloc::X86CallPLTRel4 | Reloc::Arm64Call
    )
}

/// One relocation of compiled code: the field at `offset` takes the address of symbol
/// `(namespace, key)` plus `addend`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm) struct Relocation {
    pub(in crate::vm) offset: usize,
    pub(in crate::vm) kind: Reloc,
    pub(in crate::vm) namespace: u32,
    pub(in crate::vm) key: u32,
    pub(in crate::vm) addend: i64,
}

/// Position-independent machine code and the relocations that link it.
#[derive(Debug)]
pub(in crate::vm) struct RelocatableCode {
    pub(in crate::vm) code: Box<[u8]>,
    pub(in crate::vm) relocations: Box<[Relocation]>,
}

impl RelocatableCode {
    /// The code `compiled` holds, each relocation resolved to its symbol; `None` where one
    /// names anything but a user symbol or cannot be patched.
    pub(in crate::vm) fn extract(compiled: &CompiledCode, function: &ir::Function) -> Option<Self> {
        let names = function.params.user_named_funcs();
        let relocations = compiled.buffer.relocs().iter().map(|relocation| {
            let RelocTarget::ExternalName(ir::ExternalName::User(name)) = &relocation.target else { return None };
            let name = names.get(*name)?;
            patchable(relocation.kind).then_some(Relocation {
                offset: usize::try_from(relocation.offset).ok()?,
                kind: relocation.kind,
                namespace: name.namespace,
                key: name.index,
                addend: relocation.addend,
            })
        }).collect::<Option<Box<[_]>>>()?;
        Some(Self { code: compiled.code_buffer().into(), relocations })
    }

    /// The code patched for `address`, each symbol resolved by `resolve`; an error names the
    /// first symbol it cannot resolve or relocation it cannot fill.
    pub(in crate::vm) fn link(&self, address: u64, resolve: impl Fn(u32, u32) -> Option<u64>) -> Result<Vec<u8>, String> {
        let mut linked = self.code.to_vec();
        for relocation in &*self.relocations {
            let value = resolve(relocation.namespace, relocation.key)
                .ok_or_else(|| format!("unresolved symbol ({}, {})", relocation.namespace, relocation.key))?;
            apply_relocation(&mut linked, address, (relocation.offset, relocation.kind, relocation.addend), value)?;
        }
        Ok(linked)
    }
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
        _ => return Err("relocation kind is not patchable".into()),
    }
    Ok(())
}
