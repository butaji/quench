//! What a string's drop frees and what its record is charged for.
use super::*;

impl JsString {
    /// Starts loading the allocation this string's drop will free (`RecordSpace::warm`).
    #[inline]
    pub(crate) fn prefetch_allocation(&self) {
        let address = match &self.repr {
            Repr::Flat(FlatUnits::Ascii(text)) => Rc::as_ptr(text).cast::<u8>(),
            Repr::Flat(FlatUnits::Wide(units)) => Rc::as_ptr(units).cast::<u8>(),
            _ => return,
        };
        #[cfg(target_arch = "x86_64")]
        // SAFETY: a prefetch hint reads nothing and cannot fault.
        unsafe {
            std::arch::x86_64::_mm_prefetch::<{ std::arch::x86_64::_MM_HINT_T0 }>(address.cast());
        }
        #[cfg(not(target_arch = "x86_64"))]
        let _ = address;
    }

    /// Bytes of flat units this string holds; a rope owns only its node until it flattens.
    #[inline]
    pub(crate) fn flat_unit_bytes(&self) -> usize {
        match self.existing_flat() {
            // A slice holds its record; its root was charged for the units it shares.
            Some(FlatUnits::AsciiSlice(_)) => std::mem::size_of::<AsciiSlice>(),
            Some(FlatUnits::WideSlice(_)) => std::mem::size_of::<WideSlice>(),
            Some(FlatUnits::AsciiPrefix(buffer, length)) => buffer.charge(*length),
            Some(FlatUnits::WidePrefix(buffer, length)) => buffer.charge(*length),
            Some(FlatUnits::Parts(_)) | None => 0,
            Some(units) => match units.view() {
                Units::Ascii(bytes) => bytes.len(),
                Units::Wide(units) => std::mem::size_of_val(units),
            },
        }
    }
}

impl Clone for JsString {
    #[inline(always)]
    fn clone(&self) -> Self {
        Self { repr: self.repr.clone() }
    }
}

impl Clone for Repr {
    /// Inlined at every string copy: the derived match grew past the inliner's budget with
    /// the parts arm, and regexp's string copies paid a call each.
    #[inline(always)]
    fn clone(&self) -> Self {
        match self {
            Self::Flat(units) => Self::Flat(units.clone()),
            Self::Shared(inner) => Self::Shared(Rc::clone(inner)),
        }
    }
}
