//! The registers live at a suspended frame's last suspension point, as its record keeps them
//! (`FrameRecord::suspension_live`).

use crate::compile::liveness::RegisterSet;

/// The pc of a frame's last suspension and the registers live there, when liveness covers its
/// function. Most functions use fewer than 128 registers, so the set is kept at two words,
/// boxed only past them: an `Option<(usize, Option<RegisterSet>)>` took each suspended
/// generator's and async function's record 80 bytes, this 24.
#[derive(Clone, Debug)]
pub(in crate::vm) enum SuspensionLive {
    /// Liveness does not cover the function: every register is traced.
    Uncovered { pc: u32 },
    /// The live set, within the first 128 registers.
    Half { pc: u32, words: [u64; 2] },
    /// The live set, reaching past 128 registers.
    Wide { pc: u32, set: Box<RegisterSet> },
}

const _: () = assert!(size_of::<Option<SuspensionLive>>() <= 24);

impl SuspensionLive {
    /// The suspension at `pc` with `live` its live registers (`None`: liveness does not cover
    /// the function). `None` for a pc past `u32`, which no record then remembers.
    pub(in crate::vm) fn new(pc: usize, live: Option<RegisterSet>) -> Option<Self> {
        let pc = u32::try_from(pc).ok()?;
        Some(match live {
            None => Self::Uncovered { pc },
            Some(live) => match live.as_u128() {
                Some(bits) => Self::Half { pc, words: [bits as u64, (bits >> u64::BITS) as u64] },
                None => Self::Wide { pc, set: Box::new(live) },
            },
        })
    }

    /// `slot` becomes the suspension at `pc` with `live`, a wide set reusing the box a wide one
    /// already holds.
    pub(in crate::vm) fn store(slot: &mut Option<Self>, pc: usize, live: Option<RegisterSet>) {
        if let Some(Self::Wide { pc: at, set }) = slot.as_mut()
            && let Some(live) = live
            && live.as_u128().is_none()
            && let Ok(pc) = u32::try_from(pc)
        {
            (*at, **set) = (pc, live);
            return;
        }
        *slot = Self::new(pc, live);
    }

    /// Whether the suspension was at `pc`.
    #[inline(always)]
    pub(in crate::vm) fn at(&self, pc: usize) -> bool {
        let (Self::Uncovered { pc: at } | Self::Half { pc: at, .. } | Self::Wide { pc: at, .. }) = self;
        *at as usize == pc
    }

    /// The registers live at the suspension, exactly the set it was made from; `None` where
    /// liveness does not cover the function.
    #[inline(always)]
    pub(in crate::vm) fn live(&self) -> Option<RegisterSet> {
        match self {
            Self::Uncovered { .. } => None,
            Self::Half { words: [low, high], .. } => Some(RegisterSet::from(u128::from(*low) | (u128::from(*high) << u64::BITS))),
            Self::Wide { set, .. } => Some(**set),
        }
    }
}
