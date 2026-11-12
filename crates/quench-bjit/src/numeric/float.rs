use super::*;

pub(super) fn float_arithmetic(b: &mut StencilBuilder, kind: Kind, x: ir::Value, y: ir::Value) -> ir::Value {
    match kind {
        Kind::Add => b.b.ins().fadd(x, y),
        Kind::Sub => b.b.ins().fsub(x, y),
        Kind::Mul => b.b.ins().fmul(x, y),
        _ => b.b.ins().fdiv(x, y),
    }
}

/// An operand as `f64`; a frame slot that is no Number leaves through exit 0.
pub fn f64_of(b: &mut StencilBuilder, place: Place, hole: u32) -> ir::Value {
    let flags = ir::MemFlagsData::new();
    match place {
        Place::PinF(i) => b.pins.floats[usize::from(i)],
        Place::Pin(_) | Place::Imm | Place::MemInt => {
            let narrow = int_of(b, place, hole);
            b.b.ins().fcvt_from_sint(types::F64, narrow)
        }
        Place::Const => {
            let word = word_of(b, place, hole);
            b.b.ins().bitcast(types::F64, flags, word)
        }
        Place::Mem => {
            let word = word_of(b, place, hole);
            to_f64(b, word)
        }
    }
}

/// A row with a double register: the arithmetic and relations compute in `f64` with no
/// exit except a frame operand that is no Number; a result lands in an xmm register or, as a
/// canonical word, in a frame slot.
pub(super) fn float_row(b: &mut StencilBuilder, row: Row) {
    let start = b.pins;
    let x = f64_of(b, row.a, 1);
    if matches!(row.kind, Kind::Move | Kind::Const) {
        return finish_float(b, row, x, start);
    }
    let y = f64_of(b, row.b, SECOND);
    if row.kind >= Kind::Lt {
        let (_, float_cc) = relation(row.kind);
        let fails = b.b.ins().fcmp(float_cc.complement(), x, y);
        return jump_if(b, fails);
    }
    let value = float_arithmetic(b, row.kind, x, y);
    finish_float(b, row, value, start);
}

pub(super) fn finish_float(b: &mut StencilBuilder, row: Row, value: ir::Value, start: Pins) {
    match row.dst {
        Place::PinF(i) => b.pins.floats[usize::from(i)] = value,
        _ => {
            let word = canonical(b, value);
            b.write(Source::Frame(Operand::Hole(0)), word);
        }
    }
    b.next();
    b.pins = start;
}

pub(super) fn spill_row(b: &mut StencilBuilder, row: Row) {
    let word = word_of(b, row.a, 1);
    b.write(Source::Frame(Operand::Hole(0)), word);
    b.next();
}

/// The int32 an array index operand holds: a double must be integral, else exit 0.
pub fn index_of(b: &mut StencilBuilder, place: Place, hole: u32) -> ir::Value {
    let Place::PinF(i) = place else { return int_of(b, place, hole) };
    let f = b.pins.floats[usize::from(i)];
    let narrow = b.b.ins().fcvt_to_sint_sat(types::I32, f);
    let back = b.b.ins().fcvt_from_sint(types::F64, narrow);
    let same = b.b.ins().fcmp(FloatCC::Equal, back, f);
    b.guard(same, 0);
    narrow
}

/// The tagged word of a Number operand a store writes: a frame slot that is no Number leaves
/// through exit 0.
pub fn number_word(b: &mut StencilBuilder, place: Place, hole: u32) -> ir::Value {
    let word = word_of(b, place, hole);
    if place == Place::Mem {
        let (int, double) = (is_int(b, word), is_double(b, word));
        let number = b.b.ins().bor(int, double);
        b.guard(number, 0);
    }
    word
}

/// A word into pinned double `i`: a double word is the hot case, its bits; an int word converts;
/// else exit 0.
fn deliver_double(b: &mut StencilBuilder, i: u8, word: ir::Value) {
        let (double_block, other, join) = (b.b.create_block(), b.b.create_block(), b.b.create_block());
        let result = b.b.append_block_param(join, types::F64);
        let is_double = is_double(b, word);
        b.b.ins().brif(is_double, double_block, &[], other, &[]);
        b.b.switch_to_block(double_block);
        b.b.seal_block(double_block);
        let bits = b.b.ins().bitcast(types::F64, ir::MemFlagsData::new(), word);
        b.b.ins().jump(join, &[bits.into()]);
        b.b.set_cold_block(other);
        b.b.switch_to_block(other);
        b.b.seal_block(other);
        let int = is_int(b, word);
        b.guard(int, 0);
        let narrow = b.b.ins().ireduce(types::I32, word);
        let converted = b.b.ins().fcvt_from_sint(types::F64, narrow);
        b.b.ins().jump(join, &[converted.into()]);
        b.b.switch_to_block(join);
        b.b.seal_block(join);
        b.pins.floats[usize::from(i)] = result;
}

/// Continues at the next row once `dst` holds the tagged `word` a row read: into a pinned
/// register or int slot an int word, into a pinned double a Number, else any word; a word the
/// place cannot hold leaves through exit 0 before anything is written.
pub fn deliver(b: &mut StencilBuilder, dst: Place, word: ir::Value) {
    let start = b.pins;
    match dst {
        Place::PinF(i) => deliver_double(b, i, word),
        Place::Pin(i) => {
            let ok = is_int(b, word);
            b.guard(ok, 0);
            let low = b.b.ins().ireduce(types::I32, word);
            b.pins.ints[usize::from(i)] = b.b.ins().uextend(types::I64, low);
        }
        Place::MemInt => {
            let ok = is_int(b, word);
            b.guard(ok, 0);
            let low = b.b.ins().ireduce(types::I32, word);
            b.write(Source::Frame(Operand::Hole(0)), low);
        }
        _ => b.write(Source::Frame(Operand::Hole(0)), word),
    }
    b.next();
    b.pins = start;
}
