//! Embedding reads and writes of primitive and builtin values (Node-API's BigInt, Date and
//! ArrayBuffer detach calls); none of them runs guest code.
use super::*;

/// What `detach_array_buffer_embedding` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detach {
    NotArrayBuffer,
    NotDetachable,
    Detached,
}

impl Vm {
    /// A BigInt's sign (true when negative) and magnitude as little-endian 64-bit words.
    pub(crate) fn bigint_words_embedding(&self, value: Value) -> Option<(bool, Vec<u64>)> {
        let Some(Cell::BigInt(integer)) = self.heap.get(value) else { return None };
        let (sign, words) = integer.to_num().to_u64_digits();
        Some((sign == num_bigint::Sign::Minus, words))
    }

    /// The BigInt `-magnitude` when `negative`, else `magnitude`, from little-endian words.
    pub(crate) fn bigint_from_words_embedding(&mut self, negative: bool, words: &[u64]) -> Value {
        let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        let magnitude = num_bigint::BigUint::from_bytes_le(&bytes);
        let sign = if negative { num_bigint::Sign::Minus } else { num_bigint::Sign::Plus };
        self.heap.alloc(Cell::bigint(num_bigint::BigInt::from_biguint(sign, magnitude)))
    }

    /// A Date with the realm's intrinsic prototype and time value TimeClip(`milliseconds`).
    pub(crate) fn date_embedding(&mut self, milliseconds: f64) -> Value {
        let prototype = self.intrinsic_builtin_prototype(Native::Date);
        self.heap.alloc(Cell::Date {
            milliseconds: date::time_clip(milliseconds),
            object: Box::new(Self::empty_object(prototype)),
        })
    }

    /// A Date's time value.
    pub(crate) fn date_value_embedding(&self, value: Value) -> Option<f64> {
        match self.heap.get(value) {
            Some(Cell::Date { milliseconds, .. }) => Some(*milliseconds),
            _ => None,
        }
    }

    /// DetachArrayBuffer with an undefined key: shared, Wasm memory and keyed buffers refuse.
    pub(crate) fn detach_array_buffer_embedding(&mut self, value: Value) -> Detach {
        let (shared, detached, wasm_view) = match self.heap.get(value) {
            Some(Cell::ArrayBuffer { shared, bytes, detached, .. }) => (*shared, *detached, bytes.is_wasm_view()),
            _ => return Detach::NotArrayBuffer,
        };
        let keyed = self.heap.object(value).is_some_and(|object| object.record().is_structured_transfer_forbidden()); // record-only: detach key
        if shared {
            return Detach::NotArrayBuffer;
        }
        if wasm_view || keyed {
            return Detach::NotDetachable;
        }
        if !detached {
            self.detach_array_buffer(value);
        }
        Detach::Detached
    }

    /// Whether `value` is a detached ArrayBuffer.
    pub(crate) fn array_buffer_detached_embedding(&self, value: Value) -> bool {
        self.array_buffer_detached(value)
    }
}
