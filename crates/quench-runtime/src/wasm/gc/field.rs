//! A GC field access's operand, resolved by lowering.
use super::GcFieldAccess;
use crate::Value;
use wasmparser::StorageType;

/// A field operand: the struct field index (0 for an array element) and the field's storage,
/// both resolved by lowering, so an access neither looks up its declaration nor re-checks
/// what validation proved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GcField(u32);

/// The packed storages and the bits each keeps, in tag order after a value field's tag 0.
const PACKED: [(StorageType, i32); 2] = [(StorageType::I8, u8::MAX as i32), (StorageType::I16, u16::MAX as i32)];
const STORAGE_BITS: u32 = PACKED.len().ilog2() + 1;
const STORAGE_MASK: u32 = (1 << STORAGE_BITS) - 1;

impl GcField {
    pub(crate) fn new(index: u32, storage: StorageType) -> Option<Self> {
        let packed = PACKED.iter().position(|(packed, _)| *packed == storage);
        let storage = packed.map_or(0, |at| at + 1) as u32;
        (index <= u32::MAX >> STORAGE_BITS).then_some(Self(index << STORAGE_BITS | storage))
    }

    pub(crate) fn from_tag(tag: u32) -> Option<Self> {
        ((tag & STORAGE_MASK) as usize <= PACKED.len()).then_some(Self(tag))
    }

    pub(crate) fn tag(self) -> u32 {
        self.0
    }

    pub(crate) fn index(self) -> u32 {
        self.0 >> STORAGE_BITS
    }

    /// The bits a packed field keeps; `None` for a value field.
    fn mask(self) -> Option<i32> {
        (self.0 & STORAGE_MASK).checked_sub(1).map(|at| PACKED[at as usize].1)
    }

    /// What the field stores for `value`: a packed field keeps its low bits, so an unsigned
    /// read returns what is stored.
    pub(crate) fn store(self, value: Value) -> Value {
        match self.mask() {
            Some(mask) => Value::integer(value.as_int().expect("validated packed Wasm field") & mask),
            None => value,
        }
    }

    /// What `access` reads of `stored`; a signed read sign-extends the packed bits.
    pub(crate) fn load(self, access: GcFieldAccess, stored: Value) -> Value {
        match (access, self.mask()) {
            (GcFieldAccess::ReadSigned, Some(mask)) => {
                let shift = mask.leading_zeros();
                Value::integer(((stored.as_int().expect("packed Wasm field") & mask) << shift) >> shift)
            }
            _ => stored,
        }
    }
}
