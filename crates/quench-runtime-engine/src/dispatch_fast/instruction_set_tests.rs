use super::*;

#[test]
fn instruction_membership_crosses_word_and_inline_boundaries() {
    for count in [0, 1, INSTRUCTION_WORD_BITS, INSTRUCTION_WORD_BITS + 1,
        INLINE_INSTRUCTION_WORDS * INSTRUCTION_WORD_BITS,
        INLINE_INSTRUCTION_WORDS * INSTRUCTION_WORD_BITS + 1]
    {
        let mut set = InstructionSet::new(count);
        assert_eq!(set.0.len(), count.div_ceil(INSTRUCTION_WORD_BITS));
        assert_eq!(set.0.spilled(), count > INLINE_INSTRUCTION_WORDS * INSTRUCTION_WORD_BITS);
        for pc in 0..count {
            assert!(!set.contains(pc));
            set.insert(pc);
            assert!(set.contains(pc));
            if pc + 1 < count { assert!(!set.contains(pc + 1)); }
        }
        for pc in 0..count { assert!(set.contains(pc)); }
    }
}
