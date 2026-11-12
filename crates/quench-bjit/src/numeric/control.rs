use super::*;

/// The relation of a compare kind in both representations: one table, both views.
pub(super) const RELATIONS: [(Kind, IntCC, FloatCC); 6] = [
    (Kind::Lt, IntCC::SignedLessThan, FloatCC::LessThan),
    (Kind::Le, IntCC::SignedLessThanOrEqual, FloatCC::LessThanOrEqual),
    (Kind::Gt, IntCC::SignedGreaterThan, FloatCC::GreaterThan),
    (Kind::Ge, IntCC::SignedGreaterThanOrEqual, FloatCC::GreaterThanOrEqual),
    (Kind::Eq, IntCC::Equal, FloatCC::Equal),
    (Kind::Ne, IntCC::NotEqual, FloatCC::NotEqual),
];

/// The relation of a compare kind in both representations.
pub(super) fn relation(kind: Kind) -> (IntCC, FloatCC) {
    let (_, int_cc, float_cc) = RELATIONS.iter().copied().find(|r| r.0 == kind).unwrap_or(RELATIONS[0]);
    (int_cc, float_cc)
}

/// Branch to `taken` on `fails`, else continue at the next row.
pub fn jump_if(b: &mut StencilBuilder, fails: ir::Value) {
    b.branch(fails);
    b.next();
}

pub(super) fn compare_row(b: &mut StencilBuilder, row: Row) {
    let (int_cc, float_cc) = relation(row.kind);
    if row.speculate {
        let (x, y) = (int_of(b, row.a, 1), int_of(b, row.b, SECOND));
        let fails = b.b.ins().icmp(int_cc.complement(), x, y);
        return jump_if(b, fails);
    }
    let (x, y) = (word_of(b, row.a, 1), word_of(b, row.b, SECOND));
    let both = both_int(b, x, y);
    let (int, general) = (b.b.create_block(), b.b.create_block());
    b.b.ins().brif(both, int, &[], general, &[]);
    b.b.switch_to_block(general);
    b.b.seal_block(general);
    let (fx, fy) = (to_f64(b, x), to_f64(b, y));
    let fails = b.b.ins().fcmp(float_cc.complement(), fx, fy);
    jump_if(b, fails);
    b.b.switch_to_block(int);
    b.b.seal_block(int);
    let (x32, y32) = (b.b.ins().ireduce(types::I32, x), b.b.ins().ireduce(types::I32, y));
    let fails = b.b.ins().icmp(int_cc.complement(), x32, y32);
    jump_if(b, fails);
}

/// Counts a taken backward edge in the run's counter, word 0 of the loop entry record (holes 1
/// and 2 are the offsets of the context's payload pointer and of the entry pointer in the
/// payload): the tier prices a run by the iterations it completes.
fn count_backedge(b: &mut StencilBuilder) {
    const ONE: i64 = 1;
    const AT: i32 = 0;
    let payload_at = b.operand(Operand::Hole(1), Ty::I64);
    let entry_at = b.operand(Operand::Hole(SECOND), Ty::I64);
    let payload_address = b.b.ins().iadd(b.context, payload_at);
    let payload = b.b.ins().load(types::I64, ir::MemFlagsData::trusted().with_readonly().with_can_move(), payload_address, AT);
    let entry_address = b.b.ins().iadd(payload, entry_at);
    let entry = b.b.ins().load(types::I64, ir::MemFlagsData::trusted(), entry_address, AT);
    let count = b.b.ins().load(types::I64, ir::MemFlagsData::trusted(), entry, AT);
    let more = b.b.ins().iadd_imm_s(count, ONE);
    b.b.ins().store(ir::MemFlagsData::trusted(), more, entry, AT);
}

pub(super) fn jump_row(b: &mut StencilBuilder, poll: bool) {
    if poll {
        // The interrupt flag: the context's pointer to a byte, nonzero once requested.
        let offset = b.operand(Operand::Hole(0), Ty::I64);
        let at = b.b.ins().iadd(b.context, offset);
        let flag = b.b.ins().load(types::I64, ir::MemFlagsData::trusted(), at, 0);
        let byte = b.b.ins().uload8(types::I64, ir::MemFlagsData::trusted(), flag, 0);
        let clear = b.b.ins().icmp_imm_u(IntCC::Equal, byte, 0);
        b.guard(clear, 0);
        count_backedge(b);
    }
    let always = b.b.ins().iconst(types::I8, 1);
    b.branch(always);
    // `branch` leaves a continuation block; the always-taken jump never reaches it.
    let never = b.b.ins().iconst(types::I64, 0);
    b.b.ins().return_(&[never]);
}

/// Makes a dead register's slot an int slot: its high half is the int tag.
pub(super) fn tag_init_row(b: &mut StencilBuilder) {
    let offset = b.operand(Operand::Hole(0), Ty::I64);
    let at = b.b.ins().iadd(b.frame, offset);
    let high = b.b.ins().iconst(types::I32, INT_HIGH);
    b.b.ins().store(ir::MemFlagsData::trusted(), high, at, std::mem::size_of::<u32>() as i32);
    b.next();
}

pub(super) fn guard_row(b: &mut StencilBuilder, row: Row) {
    let word = word_of(b, row.a, 1);
    if matches!(row.dst, Place::MemInt) {
        let high = b.b.ins().ushr_imm_u(word, WORD_SHIFT);
        let fails = b.b.ins().icmp_imm_u(IntCC::NotEqual, high, INT_HIGH);
        return jump_if(b, fails);
    }
    if let Place::Pin(pin) = row.dst {
        // Loads a pinned register: an int word, else the entry fails.
        let high = b.b.ins().ushr_imm_u(word, WORD_SHIFT);
        let ok = b.b.ins().icmp_imm_u(IntCC::Equal, high, INT_HIGH);
        let low = b.b.ins().ireduce(types::I32, word);
        b.pins.ints[usize::from(pin)] = b.b.ins().uextend(types::I64, low);
        let fails = b.b.ins().bxor_imm_u(ok, 1);
        return jump_if(b, fails);
    }
    let (int, double) = (is_int(b, word), is_double(b, word));
    if let Place::PinF(pin) = row.dst {
        // Loads a pinned double: an int word converts, a double word is its bits, else the entry fails.
        let narrow = b.b.ins().ireduce(types::I32, word);
        let (from_int, bits) = (b.b.ins().fcvt_from_sint(types::F64, narrow), b.b.ins().bitcast(types::F64, ir::MemFlagsData::new(), word));
        b.pins.floats[usize::from(pin)] = b.b.ins().select(int, from_int, bits);
        let number = b.b.ins().bor(int, double);
        let fails = b.b.ins().bxor_imm_u(number, 1);
        return jump_if(b, fails);
    }
    let number = b.b.ins().bor(int, double);
    let fails = b.b.ins().bxor_imm_u(number, 1);
    jump_if(b, fails);
}
