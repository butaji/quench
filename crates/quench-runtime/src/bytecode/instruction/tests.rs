use super::*;

#[test]
fn packed_word_preserves_tagged_fields() {
    let values = [
        0,
        Instr::FIELD_PAYLOAD_MASK,
        SET_THIS_REGISTER,
        RETURN_REGISTER,
        Instr::FIELD_PAYLOAD_MASK | SET_THIS_REGISTER | RETURN_REGISTER,
    ];
    for value in values {
        let instruction = Instr::new(
            Op::Binary,
            value,
            value,
            value,
            Instr::NARROW_IMMEDIATE_MASK,
        );
        assert_eq!(
            (instruction.a(), instruction.b(), instruction.c()),
            (value, value, value)
        );
        assert_eq!(instruction.imm(), Instr::NARROW_IMMEDIATE_MASK);
    }
    assert_eq!(std::mem::size_of::<Instr>(), 8);
}

#[test]
fn packed_word_preserves_declared_special_domains() {
    for base in [Instr::FIELD_SENTINEL_START, u16::MAX] {
        assert_eq!(Instr::new(Op::GetField, 0, base, 0, 0).b(), base);
    }
    let compound = (PACKED_PAIR_HIGH_MASK << PACKED_PAIR_SOURCE_SHIFT) | PACKED_PAIR_LOW_MASK;
    assert_eq!(Instr::new(Op::Call, 0, 0, 0, compound).imm(), compound);
}

#[test]
fn packed_word_rejects_every_overflow_axis() {
    assert!(Instr::try_new(Op::Binary, Instr::FIELD_PAYLOAD_MASK + 1, 0, 0, 0).is_none());
    assert!(Instr::try_new(Op::Binary, 0, Instr::FIELD_RESERVED_TAG_MASK, 0, 0).is_none());
    assert!(Instr::try_new(Op::Binary, 0, 0, Instr::FIELD_RESERVED_TAG_MASK, 0).is_none());
    assert!(Instr::try_new(Op::Binary, 0, 0, 0, Instr::NARROW_IMMEDIATE_MASK + 1).is_none());
    assert!(
        Instr::try_new(
            Op::Call,
            0,
            0,
            0,
            (PACKED_PAIR_HIGH_MASK + 1) << PACKED_PAIR_SOURCE_SHIFT
        )
        .is_none()
    );
    assert!(Instr::try_new(Op::Call, 0, 0, 0, PACKED_PAIR_LOW_MASK + 1).is_none());
}

#[test]
fn wide_marker_round_trips_the_full_side_table_index() {
    let index = (1_usize << (u64::BITS - Instr::OP_BITS - 1))
        | (0x1234 << (Instr::FIELD_BITS * 2))
        | (0x2345 << Instr::FIELD_BITS)
        | 0x3456;
    let instruction = Instr::wide(index).unwrap();
    assert!(instruction.is_wide());
    assert_eq!(instruction.wide_index(), index);
    assert_eq!(
        (instruction.a(), instruction.b(), instruction.c()),
        (0x3456, 0x2345, 0x1234)
    );
}
