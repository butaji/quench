//! A function's register masks: its live registers before each instruction, at the narrowest
//! width the register file needs, and the windowed form for bodies past one set's width.

use super::{live_masks, register_dead, windows, RegisterSet, ROOT_MAP_REGISTERS};
use crate::bytecode::{FieldSite, Function, Superinstruction};
use std::mem::size_of_val;

/// A function's live registers before each instruction (`analyze`), held at the width they
/// need: most functions use fewer than 64 registers, so their masks take half the space
/// (V8-v7 RegExp: 95 KiB of masks at its heap's peak, all within 64 registers).
#[derive(Clone, Debug)]
pub enum RegisterMasks {
    Narrow(Box<[u64]>),
    Half(Box<[u128]>),
    Wide(Box<[RegisterSet]>),
    /// A body past one set's width: `windows` sets per pc, window `w` holding registers
    /// `w * BITS .. (w + 1) * BITS`.
    Windowed { windows: u16, masks: Box<[RegisterSet]> },
}

/// The largest mask table a collection derives for a function it finds on the stack: past it
/// (a wasm body of 35K instructions over 256 registers: 1.1 MB, dotnet-aot) the collection
/// scans every register of the frame instead, and keeps no table.
pub(crate) const KEPT_MASK_BYTES: usize = 64 * 1024;

impl RegisterMasks {
    /// Whether `function`'s table, at the width its registers need, would pass
    /// `KEPT_MASK_BYTES`.
    pub fn too_large_to_keep(function: &Function) -> bool {
        let width = if u32::from(function.registers) <= u64::BITS {
            std::mem::size_of::<u64>()
        } else if u32::from(function.registers) <= u128::BITS {
            std::mem::size_of::<u128>()
        } else {
            std::mem::size_of::<RegisterSet>()
        };
        function.code.len() * width > KEPT_MASK_BYTES
    }

    /// `program`'s masks for `function` as a collection reads them: `None` (scan every register
    /// of its frames) where they are not derived yet and would pass `KEPT_MASK_BYTES`.
    pub(crate) fn for_collection(program: &crate::bytecode::ResidualProgram, function: u32) -> Option<&Self> {
        let record = program.function_get(function as usize)?;
        (record.register_masks_derived() || !Self::too_large_to_keep(record)).then(|| program.register_root_masks(function))?
    }

    /// `masks` at the narrowest width that holds every one of them. Given the live sets a solve
    /// already returned (`analyze_with`), this builds the same masks `of` derives on its solved
    /// path, without solving again: that solve narrows its sets to the same width.
    pub fn new(masks: Vec<RegisterSet>) -> Self {
        if let Some(narrow) = masks.iter().map(|&mask| u64::try_from(mask).ok()).collect::<Option<Box<[u64]>>>() { return Self::Narrow(narrow); }
        masks.iter().map(|&mask| mask.as_u128()).collect::<Option<Box<[u128]>>>().map_or_else(|| Self::Wide(masks.into_boxed_slice()), Self::Half)
    }

    /// `function`'s masks (`analyze`), solved at the width its registers need: a wide solve's
    /// tables before narrowing were 30 MB at a cold mandreel run's peak (`global_init`: 614K
    /// instructions over 9 registers). A body past the width gets one precise set per window,
    /// or one conservative all-live map when it is wider than the window budget.
    pub(crate) fn of(function: &Function, methods: &(impl super::MethodArguments + ?Sized), fields: &[FieldSite], superinstructions: &[Superinstruction]) -> Option<Self> {
        if !super::budget::admits(function) {
            return Some(Self::Wide(Box::default()));
        }
        if function.registers > ROOT_MAP_REGISTERS {
            return Some(windows::masked(function, methods, fields, superinstructions)
                .unwrap_or_else(|| Self::Wide(Box::default())));
        }
        Some(if u32::from(function.registers) <= u64::BITS {
            Self::Narrow(live_masks::<u64>(function, methods, fields, superinstructions).into_boxed_slice())
        } else if u32::from(function.registers) <= u128::BITS {
            // Solved at two words, a quarter of a set's work per transfer.
            Self::new(live_masks::<u128>(function, methods, fields, superinstructions).into_iter().map(RegisterSet::from).collect())
        } else {
            Self::new(live_masks::<RegisterSet>(function, methods, fields, superinstructions))
        })
    }

    /// The registers live before instruction `pc`, one past the last included. A windowed
    /// mask answers its first window; a register past the set's width is conservatively live.
    pub fn get(&self, pc: usize) -> Option<RegisterSet> {
        match self {
            Self::Narrow(masks) => masks.get(pc).map(|&mask| RegisterSet::from(mask)),
            Self::Half(masks) => masks.get(pc).map(|&mask| RegisterSet::from(mask)),
            Self::Wide(masks) => masks.get(pc).copied(),
            Self::Windowed { windows, masks } => masks.get(pc * usize::from(*windows)).copied(),
        }
    }

    /// Whether `register` is live at `pc`, for a windowed mask at its own window; `None` when
    /// the mask has no per-pc entry there (a collection then scans it conservatively).
    #[cfg(test)]
    pub(crate) fn live_at(&self, pc: usize, register: usize) -> Option<bool> {
        match self {
            Self::Narrow(masks) => masks.get(pc).map(|&mask| register < u64::BITS as usize && mask & (1 << register) != 0),
            Self::Half(masks) => masks.get(pc).map(|&mask| register < u128::BITS as usize && mask & (1 << register) != 0),
            Self::Wide(masks) => masks.get(pc).map(|set| set.contains(register)),
            Self::Windowed { windows, masks } => masks
                .get(pc * usize::from(*windows) + register / RegisterSet::BITS as usize)
                .map(|set| set.contains(register % RegisterSet::BITS as usize)),
        }
    }

    /// The 64 registers from `chunk * 64` set in the mask at `pc`, `None` where it has no
    /// entry there.
    fn word_at(&self, pc: usize, chunk: usize) -> Option<u64> {
        const WORD: usize = u64::BITS as usize;
        let per_set = RegisterSet::BITS as usize / WORD;
        match self {
            Self::Narrow(masks) => masks.get(pc).map(|&mask| if chunk == 0 { mask } else { 0 }),
            Self::Half(masks) => masks.get(pc).map(|&mask| if chunk < 2 { (mask >> (WORD * chunk)) as u64 } else { 0 }),
            Self::Wide(masks) => masks.get(pc).map(|set| set.word(chunk)),
            Self::Windowed { windows, masks } => masks.get(pc * usize::from(*windows) + chunk / per_set).map(|set| set.word(chunk % per_set)),
        }
    }

    /// Appends to `roots` the values of `registers` live at `pc` (`live_at`), each one with no
    /// mask entry there conservatively: each 64 registers' mask word is read once and its set
    /// bits walked, where a collection's root scan asked every register in turn.
    /// Past the registers `raw` holds at `pc`: those whose words are not `Value`s.
    pub(crate) fn extend_live_except(&self, pc: usize, raw: Option<&Self>, registers: &[crate::Value], roots: &mut Vec<crate::Value>) {
        const WORD: usize = u64::BITS as usize;
        for chunk in 0..registers.len().div_ceil(WORD) {
            let word = self.word_at(pc, chunk).unwrap_or(u64::MAX) & !raw.and_then(|raw| raw.word_at(pc, chunk)).unwrap_or(0);
            let base = chunk * WORD;
            let count = (registers.len() - base).min(WORD);
            let mut word = if count < WORD { word & ((1u64 << count) - 1) } else { word };
            while word != 0 {
                roots.push(registers[base + word.trailing_zeros() as usize]);
                word &= word - 1;
            }
        }
    }

    /// Whether the mask holds more than one register window.
    pub fn is_windowed(&self) -> bool { matches!(self, Self::Windowed { .. }) }

    /// A missing mask or a register outside its width remains conservatively live.
    pub fn is_live(&self, pc: usize, register: crate::bytecode::Register) -> bool {
        self.get(pc).is_none_or(|mask| !register_dead(mask, usize::from(register)))
    }

    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Narrow(masks) => masks.len(),
            Self::Half(masks) => masks.len(),
            Self::Wide(masks) => masks.len(),
            Self::Windowed { windows, masks } => masks.len() / usize::from(*windows),
        }
    }

    /// The bytes the masks occupy.
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::Narrow(masks) => size_of_val::<[u64]>(masks),
            Self::Half(masks) => size_of_val::<[u128]>(masks),
            Self::Wide(masks) => size_of_val::<[RegisterSet]>(masks),
            Self::Windowed { masks, .. } => size_of_val::<[RegisterSet]>(masks),
        }
    }

    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn iter(&self) -> impl Iterator<Item = RegisterSet> + '_ {
        (0..self.len()).filter_map(|pc| self.get(pc))
    }
}
