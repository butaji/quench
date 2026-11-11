//! Hand-written stencil rows for Number loops over tagged `Value` words. A row names where each
//! operand lives: a frame slot holding a tagged word (hole: byte offset), a pinned register
//! holding a raw int32 (rdx, rcx, r8, r9 through the whole chain of stencils: no tag, no
//! store-forwarding), an immediate int32 (hole) or a constant tagged word (hole). A row applies
//! the interpreter's Number semantics (`Value::number` canonicalization) and continues at the
//! next row. Speculating rows take int32 operands only: anything else, an overflow, or a result
//! a pinned register cannot hold leaves through exit 0 before any effect, with the pinned
//! registers as they were at the row's start. General rows take tagged words (pinned operands
//! are tagged on use) and write a frame slot.
//!
//! Holes: 0 is the destination offset, 1 and 2 the operands; a jump polls the interrupt flag
//! pointer at the context offset in hole 0.
use crate::row::{Operand, RowBuilder, Source, Ty};
use crate::{PINNED, Stencil, StencilBuilder, generate, stencil_isa};
use cranelift_codegen::ir::{self, InstBuilder, condcodes::{CondCode, FloatCC, IntCC}, types};
use cranelift_codegen::isa::OwnedTargetIsa;
use std::sync::OnceLock;

const TAG_INT: i64 = 0x7ffd_0000_0000_0000;
const FIRST_TAG: i64 = 0x7ff9_0000_0000_0000;
const TAG_SPAN: i64 = 6 << 48;
const CANONICAL_NAN: i64 = 0x7ff8_0000_0000_0000;
const TAG_SHIFT: i64 = 48;
const WORD_SHIFT: i64 = 32;
/// The high 32 bits of an int word.
const INT_HIGH: i64 = TAG_INT >> WORD_SHIFT;
/// Doubles below this magnitude convert to an i64 exactly.
const I64_LIMIT: f64 = 9.0e18;

/// Where a row's operand lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A frame slot: the hole is its byte offset.
    Mem,
    /// A pinned register holding a raw int32.
    Pin(u8),
    /// A constant tagged `Value` word: the hole.
    Const,
    /// An int32 constant: the hole holds its low 32 bits.
    Imm,
    /// A frame slot known to hold an int word: read and written as its low 32 bits, the tag
    /// staying in place.
    MemInt,
}

/// The operation of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Kind {
    Move,
    Const,
    Add,
    Sub,
    Mul,
    Div,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    UShr,
    /// Compare-and-jump: go to `taken` unless the relation holds.
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    /// Go to `taken` (and poll the interrupt flag first, for `JumpPoll`).
    Jump,
    JumpPoll,
    /// Go to `taken` unless the operand is a Number; into a pinned destination an int word, which
    /// it loads; for an int slot destination, an int word.
    Guard,
    /// Set the high half of the int slot named by hole 0 to the int tag.
    TagInit,
}

const KINDS: usize = Kind::TagInit as usize + 1;
const OPERAND_PLACES: usize = PINNED + 4;
const DESTINATIONS: usize = PINNED + 2;

/// One row: operation and operand places; `speculate` rows take int32 operands only and leave
/// through exit 0 otherwise, general rows also handle doubles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub dst: Place,
    pub a: Place,
    pub b: Place,
    pub speculate: bool,
}

fn operand_index(place: Place) -> usize {
    match place {
        Place::Mem => 0,
        Place::Pin(i) => 1 + usize::from(i),
        Place::Const => OPERAND_PLACES - 3,
        Place::Imm => OPERAND_PLACES - 2,
        Place::MemInt => OPERAND_PLACES - 1,
    }
}

impl Row {
    /// The row's dense index in the stencil table.
    fn slot(self) -> usize {
        let destination = match self.dst {
            Place::Pin(i) => 1 + usize::from(i),
            Place::MemInt => DESTINATIONS - 1,
            _ => 0,
        };
        let operands = operand_index(self.a) * OPERAND_PLACES + operand_index(self.b);
        ((self.kind as usize * DESTINATIONS + destination) * OPERAND_PLACES * OPERAND_PLACES + operands) * 2
            + usize::from(self.speculate)
    }
}

fn is_int(b: &mut StencilBuilder, x: ir::Value) -> ir::Value {
    let tag = b.b.ins().ushr_imm_u(x, TAG_SHIFT);
    b.b.ins().icmp_imm_s(IntCC::Equal, tag, TAG_INT >> TAG_SHIFT)
}

fn is_double(b: &mut StencilBuilder, x: ir::Value) -> ir::Value {
    let field = b.b.ins().iadd_imm_s(x, -FIRST_TAG);
    b.b.ins().icmp_imm_s(IntCC::UnsignedGreaterThanOrEqual, field, TAG_SPAN)
}

fn int_word(b: &mut StencilBuilder, v: ir::Value) -> ir::Value {
    let wide = b.b.ins().uextend(types::I64, v);
    b.b.ins().bor_imm_u(wide, TAG_INT)
}

/// `Value::number(f)`: an integral double within `i32` (not -0) becomes an int word, a NaN is
/// canonical, anything else its bits.
fn canonical(b: &mut StencilBuilder, f: ir::Value) -> ir::Value {
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
fn convert(b: &mut StencilBuilder, x: ir::Value, ty: ir::Type, int: impl FnOnce(&mut StencilBuilder, ir::Value) -> ir::Value, double: impl FnOnce(&mut StencilBuilder, ir::Value) -> ir::Value) -> ir::Value {
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
fn to_f64(b: &mut StencilBuilder, x: ir::Value) -> ir::Value {
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
fn to_i32(b: &mut StencilBuilder, x: ir::Value) -> ir::Value {
    convert(
        b,
        x,
        types::I32,
        |b, x| b.b.ins().ireduce(types::I32, x),
        |b, x| {
            let f = b.b.ins().bitcast(types::F64, ir::MemFlagsData::new(), x);
            let magnitude = b.b.ins().fabs(f);
            let limit = b.b.ins().f64const(I64_LIMIT);
            let inside = b.b.ins().fcmp(FloatCC::LessThan, magnitude, limit);
            b.guard(inside, 0);
            let wide = b.b.ins().fcvt_to_sint_sat(types::I64, f);
            b.b.ins().ireduce(types::I32, wide)
        },
    )
}

/// A uint32 as a Number word: an int word below 2^31, else a double.
fn unsigned_word(b: &mut StencilBuilder, value: ir::Value) -> ir::Value {
    let small = b.b.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, value, 0);
    let int = int_word(b, value);
    let wide = b.b.ins().fcvt_from_uint(types::F64, value);
    let double = b.b.ins().bitcast(types::I64, ir::MemFlagsData::new(), wide);
    b.b.ins().select(small, int, double)
}

/// The tagged word of an operand: a pinned int is tagged on use.
fn word_of(b: &mut StencilBuilder, place: Place, hole: u32) -> ir::Value {
    match place {
        Place::Mem | Place::MemInt => b.read(Source::Frame(Operand::Hole(hole)), Ty::Word),
        Place::Const => b.operand(Operand::Hole(hole), Ty::Word),
        Place::Pin(i) => {
            let low = b.b.ins().ireduce(types::I32, b.values[usize::from(i)]);
            int_word(b, low)
        }
        Place::Imm => {
            let low = immediate(b, hole);
            int_word(b, low)
        }
    }
}

fn immediate(b: &mut StencilBuilder, hole: u32) -> ir::Value {
    let value = b.operand(Operand::Hole(hole), Ty::I64);
    b.b.ins().ireduce(types::I32, value)
}

/// The int32 of an operand, guarded: leaves through exit 0 where a frame slot or constant word
/// is no int word.
fn int_of(b: &mut StencilBuilder, place: Place, hole: u32) -> ir::Value {
    match place {
        Place::Pin(i) => b.b.ins().ireduce(types::I32, b.values[usize::from(i)]),
        Place::Imm => immediate(b, hole),
        Place::MemInt => b.read(Source::Frame(Operand::Hole(hole)), Ty::I32),
        Place::Mem | Place::Const => {
            let word = word_of(b, place, hole);
            let high = b.b.ins().ushr_imm_u(word, WORD_SHIFT);
            let ok = b.b.ins().icmp_imm_u(IntCC::Equal, high, INT_HIGH);
            b.guard(ok, 0);
            b.b.ins().ireduce(types::I32, word)
        }
    }
}

/// Continues at the next row once the destination holds the int32 `value`; the other paths of
/// the row start from the same pinned values (`start`).
fn finish_int(b: &mut StencilBuilder, row: Row, value: ir::Value, start: [ir::Value; PINNED]) {
    match row.dst {
        Place::Pin(i) => b.values[usize::from(i)] = b.b.ins().uextend(types::I64, value),
        Place::MemInt => b.write(Source::Frame(Operand::Hole(0)), value),
        _ => {
            let word = int_word(b, value);
            b.write(Source::Frame(Operand::Hole(0)), word);
        }
    }
    b.next();
    b.values = start;
}

/// As `finish_int` for a tagged word, which only a frame slot holds.
fn finish_word(b: &mut StencilBuilder, word: ir::Value, start: [ir::Value; PINNED]) {
    b.write(Source::Frame(Operand::Hole(0)), word);
    b.next();
    b.values = start;
}

/// The int32 result of an add, subtract or multiply of int32s, leaving through exit 0 on an
/// overflow and, for a multiply, on a product of -0.
fn int_arithmetic(b: &mut StencilBuilder, kind: Kind, x: ir::Value, y: ir::Value) -> ir::Value {
    let (value, overflow) = match kind {
        Kind::Add => b.b.ins().sadd_overflow(x, y),
        Kind::Sub => b.b.ins().ssub_overflow(x, y),
        _ => b.b.ins().smul_overflow(x, y),
    };
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
fn int_exact(b: &mut StencilBuilder, kind: Kind, x: ir::Value, y: ir::Value) -> (ir::Value, ir::Value) {
    let (value, overflow) = match kind {
        Kind::Add => b.b.ins().sadd_overflow(x, y),
        Kind::Sub => b.b.ins().ssub_overflow(x, y),
        _ => b.b.ins().smul_overflow(x, y),
    };
    let exact = b.b.ins().bxor_imm_u(overflow, 1);
    if kind != Kind::Mul {
        return (value, exact);
    }
    let nonzero = b.b.ins().icmp_imm_u(IntCC::NotEqual, value, 0);
    (value, b.b.ins().band(exact, nonzero))
}

fn float_arithmetic(b: &mut StencilBuilder, kind: Kind, x: ir::Value, y: ir::Value) -> ir::Value {
    match kind {
        Kind::Add => b.b.ins().fadd(x, y),
        Kind::Sub => b.b.ins().fsub(x, y),
        Kind::Mul => b.b.ins().fmul(x, y),
        _ => b.b.ins().fdiv(x, y),
    }
}

/// The general path of an arithmetic row: both operands as doubles.
fn double_path(b: &mut StencilBuilder, row: Row, x: ir::Value, y: ir::Value, start: [ir::Value; PINNED]) {
    let (fx, fy) = (to_f64(b, x), to_f64(b, y));
    let value = float_arithmetic(b, row.kind, fx, fy);
    let word = canonical(b, value);
    finish_word(b, word, start);
}

/// Both words are int words.
fn both_int(b: &mut StencilBuilder, x: ir::Value, y: ir::Value) -> ir::Value {
    let tag = b.b.ins().iconst(types::I64, TAG_INT);
    let (dx, dy) = (b.b.ins().bxor(x, tag), b.b.ins().bxor(y, tag));
    let bits = b.b.ins().bor(dx, dy);
    let high = b.b.ins().ushr_imm_u(bits, WORD_SHIFT);
    b.b.ins().icmp_imm_u(IntCC::Equal, high, 0)
}

fn arith_row(b: &mut StencilBuilder, row: Row) {
    let start = b.values;
    if row.speculate {
        let (x, y) = (int_of(b, row.a, 1), int_of(b, row.b, 2));
        let value = int_arithmetic(b, row.kind, x, y);
        return finish_int(b, row, value, start);
    }
    let (x, y) = (word_of(b, row.a, 1), word_of(b, row.b, 2));
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
    finish_word(b, word, start);
    b.b.switch_to_block(slow);
    b.b.seal_block(slow);
    double_path(b, row, x, y, start);
}

fn int_bitwise(b: &mut StencilBuilder, kind: Kind, x: ir::Value, y: ir::Value) -> ir::Value {
    match kind {
        Kind::And => b.b.ins().band(x, y),
        Kind::Or => b.b.ins().bor(x, y),
        Kind::Xor => b.b.ins().bxor(x, y),
        Kind::Shl => b.b.ins().ishl(x, y),
        Kind::Shr => b.b.ins().sshr(x, y),
        _ => b.b.ins().ushr(x, y),
    }
}

fn bitwise_row(b: &mut StencilBuilder, row: Row) {
    let start = b.values;
    if row.speculate {
        let (x, y) = (int_of(b, row.a, 1), int_of(b, row.b, 2));
        let value = int_bitwise(b, row.kind, x, y);
        if row.kind != Kind::UShr {
            return finish_int(b, row, value, start);
        }
        if matches!(row.dst, Place::Pin(_) | Place::MemInt) {
            // A pinned register or int slot holds an int32: a result from 2^31 up is the interpreter's.
            let small = b.b.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, value, 0);
            b.guard(small, 0);
            return finish_int(b, row, value, start);
        }
        let word = unsigned_word(b, value);
        return finish_word(b, word, start);
    }
    let (x, y) = (word_of(b, row.a, 1), word_of(b, row.b, 2));
    let (x32, y32) = (to_i32(b, x), to_i32(b, y));
    let value = int_bitwise(b, row.kind, x32, y32);
    let word = if row.kind == Kind::UShr { unsigned_word(b, value) } else { int_word(b, value) };
    finish_word(b, word, start);
}

/// The relation of a compare kind in both representations: one table, both views.
const RELATIONS: [(Kind, IntCC, FloatCC); 6] = [
    (Kind::Lt, IntCC::SignedLessThan, FloatCC::LessThan),
    (Kind::Le, IntCC::SignedLessThanOrEqual, FloatCC::LessThanOrEqual),
    (Kind::Gt, IntCC::SignedGreaterThan, FloatCC::GreaterThan),
    (Kind::Ge, IntCC::SignedGreaterThanOrEqual, FloatCC::GreaterThanOrEqual),
    (Kind::Eq, IntCC::Equal, FloatCC::Equal),
    (Kind::Ne, IntCC::NotEqual, FloatCC::NotEqual),
];

/// Branch to `taken` on `fails`, else continue at the next row.
fn jump_if(b: &mut StencilBuilder, fails: ir::Value) {
    b.branch(fails);
    b.next();
}

fn compare_row(b: &mut StencilBuilder, row: Row) {
    let (_, int_cc, float_cc) = RELATIONS.iter().copied().find(|r| r.0 == row.kind).unwrap_or(RELATIONS[0]);
    if row.speculate {
        let (x, y) = (int_of(b, row.a, 1), int_of(b, row.b, 2));
        let fails = b.b.ins().icmp(int_cc.complement(), x, y);
        return jump_if(b, fails);
    }
    let (x, y) = (word_of(b, row.a, 1), word_of(b, row.b, 2));
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

fn jump_row(b: &mut StencilBuilder, poll: bool) {
    if poll {
        // The interrupt flag: the context's pointer to a byte, nonzero once requested.
        let offset = b.operand(Operand::Hole(0), Ty::I64);
        let at = b.b.ins().iadd(b.context, offset);
        let flag = b.b.ins().load(types::I64, ir::MemFlagsData::trusted(), at, 0);
        let byte = b.b.ins().uload8(types::I64, ir::MemFlagsData::trusted(), flag, 0);
        let clear = b.b.ins().icmp_imm_u(IntCC::Equal, byte, 0);
        b.guard(clear, 0);
    }
    let always = b.b.ins().iconst(types::I8, 1);
    b.branch(always);
    // `branch` leaves a continuation block; the always-taken jump never reaches it.
    let never = b.b.ins().iconst(types::I64, 0);
    b.b.ins().return_(&[never]);
}

/// Makes a dead register's slot an int slot: its high half is the int tag.
fn tag_init_row(b: &mut StencilBuilder) {
    let offset = b.operand(Operand::Hole(0), Ty::I64);
    let at = b.b.ins().iadd(b.frame, offset);
    let high = b.b.ins().iconst(types::I32, INT_HIGH);
    b.b.ins().store(ir::MemFlagsData::trusted(), high, at, std::mem::size_of::<u32>() as i32);
    b.next();
}

fn guard_row(b: &mut StencilBuilder, row: Row) {
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
        b.values[usize::from(pin)] = b.b.ins().uextend(types::I64, low);
        let fails = b.b.ins().bxor_imm_u(ok, 1);
        return jump_if(b, fails);
    }
    let (int, double) = (is_int(b, word), is_double(b, word));
    let number = b.b.ins().bor(int, double);
    let fails = b.b.ins().bxor_imm_u(number, 1);
    jump_if(b, fails);
}

/// A move or constant load: into a pinned register an int32, else a tagged word.
fn copy_row(b: &mut StencilBuilder, row: Row) {
    let start = b.values;
    if matches!(row.dst, Place::Pin(_) | Place::MemInt) {
        let value = int_of(b, row.a, 1);
        return finish_int(b, row, value, start);
    }
    let word = word_of(b, row.a, 1);
    finish_word(b, word, start);
}

fn emit(b: &mut StencilBuilder, row: Row) {
    match row.kind {
        Kind::Move | Kind::Const => copy_row(b, row),
        Kind::Add | Kind::Sub | Kind::Mul | Kind::Div => arith_row(b, row),
        Kind::And | Kind::Or | Kind::Xor | Kind::Shl | Kind::Shr | Kind::UShr => bitwise_row(b, row),
        Kind::Lt | Kind::Le | Kind::Gt | Kind::Ge | Kind::Eq | Kind::Ne => compare_row(b, row),
        Kind::Jump => jump_row(b, false),
        Kind::JumpPoll => jump_row(b, true),
        Kind::Guard => guard_row(b, row),
        Kind::TagInit => tag_init_row(b),
    }
}

fn isa() -> Result<&'static OwnedTargetIsa, String> {
    static ISA: OnceLock<Result<OwnedTargetIsa, String>> = OnceLock::new();
    ISA.get_or_init(stencil_isa).as_ref().map_err(Clone::clone)
}

static GENERATION_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Nanoseconds spent generating stencils so far.
pub fn generation_ns() -> u64 {
    GENERATION_NS.load(std::sync::atomic::Ordering::Relaxed)
}

const SLOTS: usize = KINDS * DESTINATIONS * OPERAND_PLACES * OPERAND_PLACES * 2;

/// The stencil of `row`: generated on first use into its slot of the table.
pub fn stencil(row: Row) -> Result<&'static Stencil, String> {
    static TABLE: [OnceLock<Result<Stencil, String>>; SLOTS] = [const { OnceLock::new() }; SLOTS];
    TABLE[row.slot()]
        .get_or_init(|| {
            let started = std::time::Instant::now();
            let made = isa().and_then(|isa| generate(isa, |b| emit(b, row)));
            GENERATION_NS.fetch_add(started.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
            made
        })
        .as_ref()
        .map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of every kind generates, with its operands in each kind of place; with
    /// `STENCIL_DUMP=dir`, each row's bytes are written there.
    #[test]
    fn every_kind_generates() {
        let kinds = [
            Kind::Move, Kind::Const, Kind::Add, Kind::Sub, Kind::Mul, Kind::Div, Kind::And, Kind::Or, Kind::Xor,
            Kind::Shl, Kind::Shr, Kind::UShr, Kind::Lt, Kind::Le, Kind::Gt, Kind::Ge, Kind::Eq, Kind::Ne, Kind::Jump,
            Kind::JumpPoll, Kind::Guard, Kind::TagInit,
        ];
        for kind in kinds {
            for (dst, a, b) in [(Place::Mem, Place::Mem, Place::Const), (Place::Pin(0), Place::Pin(1), Place::Pin(2)), (Place::Pin(2), Place::Pin(2), Place::Mem)] {
                for speculate in [true, false] {
                    let row = Row { kind, dst, a, b, speculate };
                    let stencil = stencil(row).unwrap();
                    if let Ok(dir) = std::env::var("STENCIL_DUMP") {
                        let name = format!("{kind:?}-{}-{speculate}", row.slot());
                        std::fs::write(format!("{dir}/{name}.bin"), &stencil.code).unwrap();
                    }
                }
            }
        }
    }
}
