use super::*;

pub fn is_int(b: &mut StencilBuilder, x: ir::Value) -> ir::Value {
    let tag = b.b.ins().ushr_imm_u(x, TAG_SHIFT);
    b.b.ins().icmp_imm_s(IntCC::Equal, tag, TAG_INT >> TAG_SHIFT)
}

pub fn is_double(b: &mut StencilBuilder, x: ir::Value) -> ir::Value {
    let field = b.b.ins().iadd_imm_s(x, -FIRST_TAG);
    b.b.ins().icmp_imm_s(IntCC::UnsignedGreaterThanOrEqual, field, TAG_SPAN)
}

pub fn int_word(b: &mut StencilBuilder, v: ir::Value) -> ir::Value {
    let wide = b.b.ins().uextend(types::I64, v);
    b.b.ins().bor_imm_u(wide, TAG_INT)
}

/// `Value::number(f)`: an integral double within `i32` (not -0) becomes an int word, a NaN is
/// canonical, anything else its bits.
pub fn canonical(b: &mut StencilBuilder, f: ir::Value) -> ir::Value {
    let flags = ir::MemFlagsData::new();
    let i = b.b.ins().fcvt_to_sint_sat(types::I32, f);
    let back = b.b.ins().fcvt_from_sint(types::F64, i);
    let (back_bits, bits) = (b.b.ins().bitcast(types::I64, flags, back), b.b.ins().bitcast(types::I64, flags, f));
    let same = b.b.ins().icmp(IntCC::Equal, back_bits, bits);
    let int = int_word(b, i);
    let nan = b.b.ins().fcmp(FloatCC::Unordered, f, f);
    let canonical_nan = b.b.ins().iconst(types::I64, CANONICAL_NAN);
    let double = b.b.ins().select(nan, canonical_nan, bits);
    b.b.ins().select(same, int, double)
}

/// A two-way split that rejoins with one value of type `ty`: `int` computes the value of an int
/// word, `double` of a double word (after the caller's guard on it).
pub(super) fn convert(b: &mut StencilBuilder, x: ir::Value, ty: ir::Type, int: impl FnOnce(&mut StencilBuilder, ir::Value) -> ir::Value, double: impl FnOnce(&mut StencilBuilder, ir::Value) -> ir::Value) -> ir::Value {
    let (int_block, double_block, join) = (b.b.create_block(), b.b.create_block(), b.b.create_block());
    let result = b.b.append_block_param(join, ty);
    let test = is_int(b, x);
    b.b.ins().brif(test, int_block, &[], double_block, &[]);
    b.b.switch_to_block(int_block);
    b.b.seal_block(int_block);
    let value = int(b, x);
    b.b.ins().jump(join, &[value.into()]);
    b.b.switch_to_block(double_block);
    b.b.seal_block(double_block);
    let ok = is_double(b, x);
    b.guard(ok, 0);
    let value = double(b, x);
    b.b.ins().jump(join, &[value.into()]);
    b.b.switch_to_block(join);
    b.b.seal_block(join);
    result
}

/// A Number operand as `f64`; leaves through exit 0 when it is not a Number.
pub fn to_f64(b: &mut StencilBuilder, x: ir::Value) -> ir::Value {
    convert(
        b,
        x,
        types::F64,
        |b, x| {
            let narrow = b.b.ins().ireduce(types::I32, x);
            b.b.ins().fcvt_from_sint(types::F64, narrow)
        },
        |b, x| b.b.ins().bitcast(types::F64, ir::MemFlagsData::new(), x),
    )
}

/// `ToInt32` of a Number operand within the exact range of an i64; leaves through exit 0
/// otherwise (a NaN, an infinity, or a magnitude from 2^63 up).
pub(super) fn to_i32(b: &mut StencilBuilder, x: ir::Value) -> ir::Value {
    convert(
        b,
        x,
        types::I32,
        |b, x| b.b.ins().ireduce(types::I32, x),
        |b, x| {
            let f = b.b.ins().bitcast(types::F64, ir::MemFlagsData::new(), x);
            f64_to_i32(b, f)
        },
    )
}

/// `ToInt32` of a double within the exact range of an i64; leaves through exit 0 otherwise.
pub fn f64_to_i32(b: &mut StencilBuilder, f: ir::Value) -> ir::Value {
    let magnitude = b.b.ins().fabs(f);
    let limit = b.b.ins().f64const(I64_LIMIT);
    let inside = b.b.ins().fcmp(FloatCC::LessThan, magnitude, limit);
    b.guard(inside, 0);
    let wide = b.b.ins().fcvt_to_sint_sat(types::I64, f);
    b.b.ins().ireduce(types::I32, wide)
}

/// A uint32 as a Number word: an int word below 2^31, else a double.
pub(super) fn unsigned_word(b: &mut StencilBuilder, value: ir::Value) -> ir::Value {
    let small = b.b.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, value, 0);
    let int = int_word(b, value);
    let wide = b.b.ins().fcvt_from_uint(types::F64, value);
    let double = b.b.ins().bitcast(types::I64, ir::MemFlagsData::new(), wide);
    b.b.ins().select(small, int, double)
}

/// The tagged word of an operand: a pinned int is tagged on use.
pub fn word_of(b: &mut StencilBuilder, place: Place, hole: u32) -> ir::Value {
    match place {
        Place::Mem | Place::MemInt => b.read(Source::Frame(Operand::Hole(hole)), Ty::Word),
        Place::Const => b.operand(Operand::Hole(hole), Ty::Word),
        Place::Pin(i) => {
            let low = b.b.ins().ireduce(types::I32, b.pins.ints[usize::from(i)]);
            int_word(b, low)
        }
        Place::Imm => {
            let low = immediate(b, hole);
            int_word(b, low)
        }
        Place::PinF(i) => canonical(b, b.pins.floats[usize::from(i)]),
    }
}

pub(super) fn immediate(b: &mut StencilBuilder, hole: u32) -> ir::Value {
    let value = b.operand(Operand::Hole(hole), Ty::I64);
    b.b.ins().ireduce(types::I32, value)
}

/// The int32 of an operand, guarded: leaves through exit 0 where a frame slot or constant word
/// is no int word.
pub fn int_of(b: &mut StencilBuilder, place: Place, hole: u32) -> ir::Value {
    match place {
        Place::Pin(i) => b.b.ins().ireduce(types::I32, b.pins.ints[usize::from(i)]),
        Place::Imm => immediate(b, hole),
        Place::MemInt => b.read(Source::Frame(Operand::Hole(hole)), Ty::I32),
        Place::PinF(i) => f64_to_i32(b, b.pins.floats[usize::from(i)]),
        Place::Mem | Place::Const => {
            let word = word_of(b, place, hole);
            let high = b.b.ins().ushr_imm_u(word, WORD_SHIFT);
            let ok = b.b.ins().icmp_imm_u(IntCC::Equal, high, INT_HIGH);
            b.guard(ok, 0);
            b.b.ins().ireduce(types::I32, word)
        }
    }
}

/// Both words are int words.
pub(super) fn both_int(b: &mut StencilBuilder, x: ir::Value, y: ir::Value) -> ir::Value {
    let tag = b.b.ins().iconst(types::I64, TAG_INT);
    let (dx, dy) = (b.b.ins().bxor(x, tag), b.b.ins().bxor(y, tag));
    let bits = b.b.ins().bor(dx, dy);
    let high = b.b.ins().ushr_imm_u(bits, WORD_SHIFT);
    b.b.ins().icmp_imm_u(IntCC::Equal, high, 0)
}
