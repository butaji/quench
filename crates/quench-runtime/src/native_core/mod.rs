//! Native execution data owned by the VM hot path.
//!
//! The value word is the single physical representation used by registers,
//! mutable slots, native entries, and copy-and-patch operands.  Semantic
//! `Value` objects are decoded only at explicit interpreter/host boundaries.

pub mod value_word;

pub const WORD_BYTES: usize = std::mem::size_of::<value_word::TaggedValue>();

const _: () = assert!(WORD_BYTES == std::mem::size_of::<u64>());

#[cfg(test)]
mod tests {
    #[test]
    fn native_word_is_one_machine_word() {
        assert_eq!(super::WORD_BYTES, std::mem::size_of::<u64>());
    }
}
