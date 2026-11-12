use super::*;

/// Continues at the next row once the destination holds the int32 `value`; the other paths of
/// the row start from the same pinned values (`start`).
pub(super) fn finish_int(b: &mut StencilBuilder, row: Row, value: ir::Value, start: Pins) {
    match row.dst {
        Place::Pin(i) => b.pins.ints[usize::from(i)] = b.b.ins().uextend(types::I64, value),
        Place::MemInt => b.write(Source::Frame(Operand::Hole(0)), value),
        _ => {
            let word = int_word(b, value);
            b.write(Source::Frame(Operand::Hole(0)), word);
        }
    }
    b.next();
    b.pins = start;
}

/// As `finish_int` for a tagged word, which only a frame slot holds.
pub(super) fn finish_word(b: &mut StencilBuilder, dst: Place, word: ir::Value, start: Pins) {
    // A pinned double takes the tagged result as a double, not the frame slot's word.
    if matches!(dst, Place::PinF(_)) {
        return deliver(b, dst, word);
    }
    b.write(Source::Frame(Operand::Hole(0)), word);
    b.next();
    b.pins = start;
}

/// The wrapped result of an add, subtract or multiply of int32s and whether it overflowed.
fn checked(b: &mut StencilBuilder, kind: Kind, x: ir::Value, y: ir::Value) -> (ir::Value, ir::Value) {
    match kind {
        Kind::Add => b.b.ins().sadd_overflow(x, y),
        Kind::Sub => b.b.ins().ssub_overflow(x, y),
        _ => b.b.ins().smul_overflow(x, y),
    }
}

/// Whether `place` holds an int32 rather than a tagged word.
fn holds_int(place: Place) -> bool {
    matches!(place, Place::Pin(_) | Place::MemInt)
}

/// The int32 result of an add, subtract or multiply of int32s, leaving through exit 0 on an
/// overflow and, for a multiply, on a product of -0.
pub(super) fn int_arithmetic(b: &mut StencilBuilder, kind: Kind, x: ir::Value, y: ir::Value) -> ir::Value {
    let (value, overflow) = checked(b, kind, x, y);
    b.leave_when(overflow, true, 0);
    if kind == Kind::Mul {
        // A zero product is -0 when an operand is negative.
        let zero = b.b.ins().icmp_imm_u(IntCC::Equal, value, 0);
        let signs = b.b.ins().bor(x, y);
        let negative = b.b.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
        let minus_zero = b.b.ins().band(zero, negative);
        b.leave_when(minus_zero, true, 0);
    }
    value
}

/// The int32 result of an add, subtract or multiply and whether it is exact (no overflow and,
/// for a multiply, a nonzero product).
pub(super) fn int_exact(b: &mut StencilBuilder, kind: Kind, x: ir::Value, y: ir::Value) -> (ir::Value, ir::Value) {
    let (value, overflow) = checked(b, kind, x, y);
    let exact = b.b.ins().bxor_imm_u(overflow, 1);
    if kind != Kind::Mul {
        return (value, exact);
    }
    let nonzero = b.b.ins().icmp_imm_u(IntCC::NotEqual, value, 0);
    (value, b.b.ins().band(exact, nonzero))
}

/// The general path of an arithmetic row: both operands as doubles.
pub(super) fn double_path(b: &mut StencilBuilder, row: Row, x: ir::Value, y: ir::Value, start: Pins) {
    let (fx, fy) = (to_f64(b, x), to_f64(b, y));
    let value = float_arithmetic(b, row.kind, fx, fy);
    let word = canonical(b, value);
    finish_word(b, row.dst, word, start);
}

pub(super) fn arith_row(b: &mut StencilBuilder, row: Row) {
    let start = b.pins;
    if row.speculate {
        let (x, y) = (int_of(b, row.a, 1), int_of(b, row.b, SECOND));
        let value = int_arithmetic(b, row.kind, x, y);
        return finish_int(b, row, value, start);
    }
    let (x, y) = (word_of(b, row.a, 1), word_of(b, row.b, SECOND));
    if row.kind == Kind::Div {
        return double_path(b, row, x, y, start);
    }
    let both = both_int(b, x, y);
    let (x32, y32) = (b.b.ins().ireduce(types::I32, x), b.b.ins().ireduce(types::I32, y));
    let (value, exact) = int_exact(b, row.kind, x32, y32);
    let (fast, slow) = (b.b.create_block(), b.b.create_block());
    let ready = b.b.ins().band(both, exact);
    b.b.ins().brif(ready, fast, &[], slow, &[]);
    b.b.switch_to_block(fast);
    b.b.seal_block(fast);
    let word = int_word(b, value);
    finish_word(b, row.dst, word, start);
    b.b.switch_to_block(slow);
    b.b.seal_block(slow);
    double_path(b, row, x, y, start);
}

pub(super) fn int_bitwise(b: &mut StencilBuilder, kind: Kind, x: ir::Value, y: ir::Value) -> ir::Value {
    match kind {
        Kind::And => b.b.ins().band(x, y),
        Kind::Or => b.b.ins().bor(x, y),
        Kind::Xor => b.b.ins().bxor(x, y),
        Kind::Shl => b.b.ins().ishl(x, y),
        Kind::Shr => b.b.ins().sshr(x, y),
        _ => b.b.ins().ushr(x, y),
    }
}

pub(super) fn bitwise_row(b: &mut StencilBuilder, row: Row) {
    let start = b.pins;
    if row.speculate {
        let (x, y) = (int_of(b, row.a, 1), int_of(b, row.b, SECOND));
        let value = int_bitwise(b, row.kind, x, y);
        if row.kind != Kind::UShr {
            return finish_int(b, row, value, start);
        }
        if holds_int(row.dst) {
            // A pinned register or int slot holds an int32: a result from 2^31 up is the interpreter's.
            let small = b.b.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, value, 0);
            b.guard(small, 0);
            return finish_int(b, row, value, start);
        }
        let word = unsigned_word(b, value);
        return finish_word(b, row.dst, word, start);
    }
    let (x, y) = (word_of(b, row.a, 1), word_of(b, row.b, SECOND));
    let (x32, y32) = (to_i32(b, x), to_i32(b, y));
    let value = int_bitwise(b, row.kind, x32, y32);
    let word = if row.kind == Kind::UShr { unsigned_word(b, value) } else { int_word(b, value) };
    finish_word(b, row.dst, word, start);
}

/// A move or constant load: into a pinned register an int32, else a tagged word.
pub(super) fn copy_row(b: &mut StencilBuilder, row: Row) {
    let start = b.pins;
    if holds_int(row.dst) {
        let value = int_of(b, row.a, 1);
        return finish_int(b, row, value, start);
    }
    let word = word_of(b, row.a, 1);
    finish_word(b, row.dst, word, start);
}
