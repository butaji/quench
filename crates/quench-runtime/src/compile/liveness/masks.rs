//! A function's register masks: its live registers before each instruction, at the narrowest
//! width the register file needs, and the windowed form for bodies past one set's width.

use super::{live_masks, register_dead, register_in, windows, RegisterSet, ROOT_MAP_REGISTERS};
use crate::bytecode::{FieldSite, Function, Superinstruction};
use std::mem::size_of_val;

/// A function's live registers before each instruction (`analyze`), held at the width they
/// need: most functions use fewer than 64 registers, so their masks take half the space
/// (V8-v7 RegExp: 95 KiB of masks at its heap's peak, all within 64 registers).
#[derive(Clone, Debug)]
pub(crate) enum RegisterMasks {
    Narrow(Box<[u64]>),
    Half(Box<[u128]>),
    Wide(Box<[RegisterSet]>),
    /// A body past one set's width: `windows` sets per pc, window `w` holding registers
    /// `w * BITS .. (w + 1) * BITS`.
    Windowed { windows: u16, masks: Box<[RegisterSet]> },
}

impl RegisterMasks {
    /// `masks` at the narrowest width that holds every one of them.
    pub(crate) fn new(masks: Vec<RegisterSet>) -> Self {
        if let Some(narrow) = masks.iter().map(|&mask| u64::try_from(mask).ok()).collect::<Option<Box<[u64]>>>() { return Self::Narrow(narrow); }
        masks.iter().map(|&mask| mask.as_u128()).collect::<Option<Box<[u128]>>>().map_or_else(|| Self::Wide(masks.into_boxed_slice()), Self::Half)
    }

    /// `function`'s masks (`analyze`), solved at the width its registers need: a wide solve's
    /// tables before narrowing were 30 MB at a cold mandreel run's peak (`global_init`: 614K
    /// instructions over 9 registers). A body past the width gets one precise set per window,
    /// or one conservative all-live map when it is wider than the window budget.
    pub(crate) fn of(function: &Function, methods: &(impl super::MethodArguments + ?Sized), fields: &[FieldSite], superinstructions: &[Superinstruction]) -> Option<Self> {
        if function.registers > ROOT_MAP_REGISTERS {
            return Some(windows::masked(function, methods, fields, superinstructions)
                .unwrap_or_else(|| Self::Wide(Box::default())));
        }
        Some(if u32::from(function.registers) <= u64::BITS {
            Self::Narrow(live_masks::<u64>(function, methods, fields, superinstructions).into_boxed_slice())
        } else {
            Self::new(live_masks::<RegisterSet>(function, methods, fields, superinstructions))
        })
    }

    /// The registers live before instruction `pc`, one past the last included. A windowed
    /// mask answers its first window; a register past the set's width is conservatively live.
    pub(crate) fn get(&self, pc: usize) -> Option<RegisterSet> {
        match self {
            Self::Narrow(masks) => masks.get(pc).map(|&mask| RegisterSet::from(mask)),
            Self::Half(masks) => masks.get(pc).map(|&mask| RegisterSet::from(mask)),
            Self::Wide(masks) => masks.get(pc).copied(),
            Self::Windowed { windows, masks } => masks.get(pc * usize::from(*windows)).copied(),
        }
    }

    /// Whether `register` is live at `pc`, for a windowed mask at its own window; `None` when
    /// the mask has no per-pc entry there (a collection then scans it conservatively).
    pub(crate) fn live_at(&self, pc: usize, register: usize) -> Option<bool> {
        match self {
            Self::Windowed { windows, masks } => masks
                .get(pc * usize::from(*windows) + register / RegisterSet::BITS as usize)
                .map(|set| set.contains(register % RegisterSet::BITS as usize)),
            _ => self.get(pc).map(|mask| register_in(mask, register)),
        }
    }

    /// Whether the mask holds more than one register window.
    pub(crate) fn is_windowed(&self) -> bool { matches!(self, Self::Windowed { .. }) }

    /// A missing mask or a register outside its width remains conservatively live.
    pub(crate) fn is_live(&self, pc: usize, register: crate::bytecode::Register) -> bool {
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
