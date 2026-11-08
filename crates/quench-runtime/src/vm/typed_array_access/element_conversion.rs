//! Converting a run of typed-array elements to another Number kind on their bytes.

use super::{TypedArrayElement, TypedArrayKind, Vm};
use crate::Host;

impl<H: Host> Vm<H> {
    /// Each `source_kind` element of `source` into `target` as `target_kind`, as storing the
    /// element's Number would. An integer stored into a modular integer kind keeps its low
    /// bytes, extended by its sign, so that conversion makes no Number.
    pub(super) fn typed_array_convert_elements(
        source: &[u8],
        source_kind: TypedArrayKind,
        target: &mut [u8],
        target_kind: TypedArrayKind,
    ) {
        let (source_width, target_width) = (source_kind.width(), target_kind.width());
        if let (Some(signed), Some(_), false) =
            (source_kind.integer_signed(), target_kind.integer_signed(), target_kind == TypedArrayKind::Uint8Clamped)
        {
            return convert_integers(source, source_width, signed, target, target_width);
        }
        for (from, to) in source.chunks_exact(source_width).zip(target.chunks_exact_mut(target_width)) {
            let mut element = [0; size_of::<u64>()];
            element[..source_width].copy_from_slice(from);
            let number = Self::typed_array_number_of(source_kind, &element);
            let encoded = Self::typed_array_element_bytes(target_kind, &TypedArrayElement::Number(number));
            to.copy_from_slice(&encoded[..target_width]);
        }
    }
}

/// Integer elements into a modular integer kind: each keeps its low `target_width` bytes.
fn convert_integers(source: &[u8], source_width: usize, signed: bool, target: &mut [u8], target_width: usize) {
    if source_width == target_width {
        return target.copy_from_slice(source);
    }
    for (from, to) in source.chunks_exact(source_width).zip(target.chunks_exact_mut(target_width)) {
        let integer = extended_integer(from, signed);
        match to {
            [byte] => *byte = integer as u8,
            [_, _] => to.copy_from_slice(&(integer as u16).to_ne_bytes()),
            _ => to.copy_from_slice(&(integer as u32).to_ne_bytes()),
        }
    }
}

/// The integer element `bytes` hold, extended to 64 bits by its sign.
fn extended_integer(bytes: &[u8], signed: bool) -> u64 {
    match (bytes, signed) {
        (&[byte], false) => u64::from(byte),
        (&[byte], true) => byte as i8 as u64,
        (&[a, b], false) => u64::from(u16::from_ne_bytes([a, b])),
        (&[a, b], true) => i16::from_ne_bytes([a, b]) as u64,
        (&[a, b, c, d], false) => u64::from(u32::from_ne_bytes([a, b, c, d])),
        (&[a, b, c, d], true) => i32::from_ne_bytes([a, b, c, d]) as u64,
        _ => unreachable!("an integer element is 1, 2 or 4 bytes"),
    }
}
