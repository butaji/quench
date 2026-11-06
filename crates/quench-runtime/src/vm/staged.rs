//! Operation semantics written once, over a binding-time domain (`Stage`).
//!
//! Futamura's first projection made literal, by Ershov's mixed computation: each operation
//! below is one Rust function generic over the stage that runs it. The interpreter runs it
//! on machine words (`Concrete`); the specializer runs it on IR values, which emits the
//! residual code (`compiled::object_slots::Emit`), for the JIT and, under a build-time
//! division, AOT. What the function holds as a plain Rust value (an exact shape word, a
//! slot) is static: the interpreter decides on it as it runs, the specializer folds it, so a
//! branch on it leaves only the taken arm in native code. An improvement to an operation here
//! is one for every tier.

use crate::heap::{PlainPropertyStorage, NATIVE_OBJECT_LAYOUT as OBJECT};
use crate::value::{NarrowLayout, ValueLayout};
use crate::value_vec::INLINE_PROPERTY_COUNT;

/// A binding-time domain: the machine words an operation computes on and the guards that
/// leave it. Words are 64 bits; a 32-bit load zero-extends.
pub(crate) trait Stage {
    type Word: Copy;
    type Cond: Copy;
    /// Where a failed guard goes: the interpreter's slow path, or a native exit.
    type Miss: Copy;
    fn constant(&mut self, bits: u64) -> Self::Word;
    fn load_u32(&mut self, base: Self::Word, offset: i32) -> Self::Word;
    fn load_u64(&mut self, base: Self::Word, offset: i32) -> Self::Word;
    fn store_u32(&mut self, base: Self::Word, offset: i32, value: Self::Word);
    fn store_u64(&mut self, base: Self::Word, offset: i32, value: Self::Word);
    /// A load from a table that never changes while the process runs.
    fn load_table(&mut self, address: Self::Word) -> Self::Word;
    fn add(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    fn or(&mut self, a: Self::Word, b: Self::Word) -> Self::Word;
    fn add_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn mul_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn and_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn or_imm(&mut self, a: Self::Word, imm: u64) -> Self::Word;
    fn shr_imm(&mut self, a: Self::Word, shift: u32) -> Self::Word;
    /// The low 32 bits shifted right arithmetically, zero-extended to the word.
    fn sar32_imm(&mut self, a: Self::Word, shift: u32) -> Self::Word;
    fn eq_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    /// Whether `a` is below `imm`, both unsigned.
    fn below_imm(&mut self, a: Self::Word, imm: u64) -> Self::Cond;
    /// The word `then` computes where `cond` holds, else `otherwise`'s: only the arm taken runs.
    fn choose(&mut self, cond: Self::Cond, then: impl FnOnce(&mut Self) -> Self::Word, otherwise: impl FnOnce(&mut Self) -> Self::Word) -> Self::Word;
    /// Continues where `cond` holds; `None` where it never can, so nothing runs past it.
    fn guard(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
    /// The property arena's base, whose values a flat object's run starts at its position.
    fn property_base(&mut self) -> Self::Word;
}

/// Where an object of a proved exact shape keeps its named values: the payload word that packs
/// them, read at once or at each read, or the address of its arena run.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slots<W> {
    Packed(W),
    PackedRecord(W),
    Arena(W),
}

/// The layout a proved exact shape gives an object record (`layout`), and how many values its
/// shape holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout<W> {
    pub(crate) slots: Slots<W>,
    pub(crate) storage_len: usize,
}

/// Whether the object record at `record` has the exact shape word `exact`
/// (`Object::exact_shape`), which proves its storage form with its shape.
#[inline(always)]
pub(crate) fn exact_shape_is<S: Stage>(stage: &mut S, record: S::Word, exact: u32) -> S::Cond {
    let shape = stage.load_u32(record, OBJECT.auxiliary_offset as i32);
    stage.eq_imm(shape, u64::from(exact))
}

/// The layout of the object record at `record`, whose exact shape the caller proved `exact`:
/// the form that word names fixes where the values live (`PlainPropertyStorage::of_exact`), so
/// it reads the payload word once and tests nothing. `snapshot` keeps the packed values as read
/// here, for a caller no store reaches; else each read loads them again. `None` for values
/// behind extras, which the VM's own read takes.
#[inline(always)]
pub(crate) fn layout<S: Stage>(stage: &mut S, record: S::Word, exact: u32, storage_len: usize, snapshot: bool) -> Option<Layout<S::Word>> {
    let slots = match PlainPropertyStorage::of_exact(exact)? {
        PlainPropertyStorage::PackedInline if snapshot => Slots::Packed(stage.load_u64(record, OBJECT.payload_offset as i32)),
        PlainPropertyStorage::PackedInline => Slots::PackedRecord(record),
        PlainPropertyStorage::Arena => {
            // A flat object's payload word is its run's position alone (`Object::plain_storage_start`).
            let start = stage.load_u64(record, OBJECT.payload_offset as i32);
            let offset = stage.mul_imm(start, OBJECT.property_value_size as u64);
            let base = stage.property_base();
            Slots::Arena(stage.add(base, offset))
        }
    };
    Some(Layout { slots, storage_len })
}

/// Value `slot` of an object of `layout`, which holds it.
#[inline(always)]
pub(crate) fn read_slot<S: Stage>(stage: &mut S, layout: Layout<S::Word>, slot: u16) -> S::Word {
    debug_assert!(usize::from(slot) < layout.storage_len);
    let payload = match layout.slots {
        Slots::Arena(base) => return stage.load_u64(base, i32::from(slot) * OBJECT.property_value_size as i32),
        Slots::Packed(payload) => payload,
        Slots::PackedRecord(record) => stage.load_u64(record, OBJECT.payload_offset as i32),
    };
    debug_assert!(usize::from(slot) < INLINE_PROPERTY_COUNT);
    let half = stage.shr_imm(payload, u32::from(slot) * u32::BITS);
    let narrow = stage.and_imm(half, u64::from(u32::MAX));
    decode_narrow(stage, narrow)
}

/// Stores `value` at `slot` of an object of `layout`, as every tier stores an own field of a
/// proved exact shape: an arena slot takes the word itself; a packed slot takes an integer's
/// narrow word (`Value::to_narrow`) where it fits the narrow range, and an integer needs no
/// write barrier. `int` says the value is known an integer, so its tag is not tested. `None`
/// for any other packed value (a reference's narrow form, or a wide value), which the caller's
/// general store takes, the object unchanged; `miss` where a test of the value fails.
#[inline(always)]
pub(crate) fn store_slot<S: Stage>(stage: &mut S, layout: Layout<S::Word>, slot: u16, value: S::Word, int: bool, miss: S::Miss) -> Option<()> {
    debug_assert!(usize::from(slot) < layout.storage_len);
    let record = match layout.slots {
        Slots::Arena(base) => {
            stage.store_u64(base, i32::from(slot) * OBJECT.property_value_size as i32, value);
            return Some(());
        }
        // A snapshot of packed values serves only a caller no store reaches.
        Slots::Packed(_) => return None,
        Slots::PackedRecord(record) => record,
    };
    if !int {
        let upper = stage.and_imm(value, !u64::from(u32::MAX));
        let integer = stage.eq_imm(upper, ValueLayout::INT_TAG);
        stage.guard(integer, miss)?;
    }
    // The int's low word, biased by half the narrow range, lies below the whole range.
    let half = 1_u64 << (NarrowLayout::INTEGER_BITS - 1);
    let low = stage.and_imm(value, u64::from(u32::MAX));
    let biased = stage.add_imm(low, half);
    let biased = stage.and_imm(biased, u64::from(u32::MAX));
    let fits = stage.below_imm(biased, half << 1);
    stage.guard(fits, miss)?;
    let shifted = stage.mul_imm(low, 1 << NarrowLayout::CODE_SHIFT);
    let narrow = stage.or_imm(shifted, u64::from(NarrowLayout::INTEGER_FLAG));
    let narrow = stage.and_imm(narrow, u64::from(u32::MAX));
    let offset = OBJECT.payload_offset + usize::from(slot) * std::mem::size_of::<u32>();
    stage.store_u32(record, offset as i32, narrow);
    Some(())
}

/// The value a narrow word (`Value::to_narrow`), zero-extended, encodes (`Value::from_narrow`):
/// an integer's code shifted down under the int tag, or a reference's upper word by its code
/// beside its index; only the arm the word's integer flag takes runs.
#[inline(always)]
pub(crate) fn decode_narrow<S: Stage>(stage: &mut S, narrow: S::Word) -> S::Word {
    let flag = stage.and_imm(narrow, u64::from(NarrowLayout::INTEGER_FLAG));
    let is_reference = stage.eq_imm(flag, 0);
    stage.choose(is_reference, |stage| {
        let code = stage.shr_imm(narrow, NarrowLayout::CODE_SHIFT);
        let code = stage.and_imm(code, u64::from(NarrowLayout::CODE_MASK));
        let offset = stage.mul_imm(code, std::mem::size_of::<u64>() as u64);
        let table = stage.constant(NarrowLayout::upper_words() as u64);
        let entry = stage.add(table, offset);
        let upper = stage.load_table(entry);
        let lower = stage.shr_imm(narrow, NarrowLayout::INDEX_SHIFT);
        stage.or(upper, lower)
    }, |stage| {
        let shifted = stage.sar32_imm(narrow, NarrowLayout::CODE_SHIFT);
        stage.or_imm(shifted, ValueLayout::INT_TAG)
    })
}

/// The interpreter's stage: words are the machine's own, and a failed guard returns to the
/// caller's slow path. The property arena gives its base where an operation addresses it.
pub(crate) struct Concrete {
    pub(crate) arena: *const crate::value_vec::ValueArena,
}

impl Stage for Concrete {
    type Word = u64;
    type Cond = bool;
    type Miss = ();
    #[inline(always)]
    fn constant(&mut self, bits: u64) -> u64 { bits }
    #[inline(always)]
    fn load_u32(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: an operation loads only the layout offsets of a live record it was given.
        u64::from(unsafe { ((base as usize).wrapping_add_signed(offset as isize) as *const u32).read() })
    }
    #[inline(always)]
    fn load_u64(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`, of a word-aligned field or value.
        unsafe { ((base as usize).wrapping_add_signed(offset as isize) as *const u64).read() }
    }
    #[inline(always)]
    fn load_table(&mut self, address: u64) -> u64 { self.load_u64(address, 0) }
    #[inline(always)]
    fn store_u32(&mut self, base: u64, offset: i32, value: u64) {
        // SAFETY: as `load_u32`, into a record or value the caller owns exclusively.
        unsafe { ((base as usize).wrapping_add_signed(offset as isize) as *mut u32).write(value as u32) }
    }
    #[inline(always)]
    fn store_u64(&mut self, base: u64, offset: i32, value: u64) {
        // SAFETY: as `store_u32`.
        unsafe { ((base as usize).wrapping_add_signed(offset as isize) as *mut u64).write(value) }
    }
    #[inline(always)]
    fn add(&mut self, a: u64, b: u64) -> u64 { a.wrapping_add(b) }
    #[inline(always)]
    fn or(&mut self, a: u64, b: u64) -> u64 { a | b }
    #[inline(always)]
    fn add_imm(&mut self, a: u64, imm: u64) -> u64 { a.wrapping_add(imm) }
    #[inline(always)]
    fn mul_imm(&mut self, a: u64, imm: u64) -> u64 { a.wrapping_mul(imm) }
    #[inline(always)]
    fn and_imm(&mut self, a: u64, imm: u64) -> u64 { a & imm }
    #[inline(always)]
    fn or_imm(&mut self, a: u64, imm: u64) -> u64 { a | imm }
    #[inline(always)]
    fn shr_imm(&mut self, a: u64, shift: u32) -> u64 { a >> shift }
    #[inline(always)]
    fn sar32_imm(&mut self, a: u64, shift: u32) -> u64 { u64::from(((a as u32 as i32) >> shift) as u32) }
    #[inline(always)]
    fn eq_imm(&mut self, a: u64, imm: u64) -> bool { a == imm }
    #[inline(always)]
    fn below_imm(&mut self, a: u64, imm: u64) -> bool { a < imm }
    #[inline(always)]
    fn choose(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> u64, otherwise: impl FnOnce(&mut Self) -> u64) -> u64 {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn guard(&mut self, cond: bool, _: ()) -> Option<()> { cond.then_some(()) }
    #[inline(always)]
    // SAFETY: the stage lives within the operation, while the arena lives with its heap.
    fn property_base(&mut self) -> u64 { unsafe { (*self.arena).native_values().0 as u64 } }
}

/// The own data value `slot` of the object `object` where its exact shape is `exact`, as every
/// tier reads it (`exact_shape_is`, `layout`, `read_slot`); `None` where the shape differs or
/// its values sit behind extras, which the caller's general read takes.
#[inline(always)]
pub(crate) fn own_field(heap: &crate::heap::Heap, object: &crate::heap::Object, exact: u32, slot: u16, storage_len: usize) -> Option<crate::Value> {
    let mut stage = Concrete { arena: heap.property_arena() };
    let record = object as *const crate::heap::Object as u64;
    let matched = exact_shape_is(&mut stage, record, exact);
    stage.guard(matched, ())?;
    let layout = layout(&mut stage, record, exact, storage_len, false)?;
    Some(crate::Value::from_bits(read_slot(&mut stage, layout, slot)))
}

/// `store_slot` of `value` at `slot` of `object`, of the exact shape `exact`, into the property
/// arena `arena`, as the interpreter stores a proved own field; `false` where the shared store
/// does not take it (`store_slot`), the object unchanged.
#[inline(always)]
pub(crate) fn store_own_field(arena: &mut crate::value_vec::ValueArena, object: &mut crate::heap::Object, exact: u32, slot: u16, value: crate::Value) -> bool {
    let mut stage = Concrete { arena };
    let record = object as *mut crate::heap::Object as u64;
    layout(&mut stage, record, exact, usize::from(slot) + 1, false)
        .and_then(|layout| store_slot(&mut stage, layout, slot, value.to_bits(), false, ()))
        .is_some()
}

/// A stage with a register file: the frame an instruction's semantics reads and writes. The
/// interpreter's is the activation's registers; the specializer's is its register state, whose
/// facts (a value known, a representation proved) it keeps and folds on.
pub(crate) trait Machine: Stage {
    /// The value of `register`, as a tagged word.
    fn register(&mut self, register: crate::bytecode::Register) -> Self::Word;
    /// Writes the tagged word `value` to `register`.
    fn set_register(&mut self, register: crate::bytecode::Register, value: Self::Word);
    /// Writes `value`, which the semantics knows, to `register`: the interpreter stores its
    /// bits, the specializer takes it as a fact and folds on it.
    fn set_static(&mut self, register: crate::bytecode::Register, value: crate::Value);
    /// Copies `source` to `destination`, with everything known of it.
    fn copy_register(&mut self, destination: crate::bytecode::Register, source: crate::bytecode::Register) {
        let value = self.register(source);
        self.set_register(destination, value);
    }
}

/// The interpreter's register file, as words: the activation's registers and whatever the
/// interpreter keeps of them outside memory (`dispatch_fast`'s accumulator).
pub(crate) trait RegisterFile {
    fn get(&mut self, register: crate::bytecode::Register) -> u64;
    fn set(&mut self, register: crate::bytecode::Register, value: u64);
}

/// The interpreter's machine: the concrete stage over a register file.
pub(crate) struct ConcreteFrame<R> {
    pub(crate) stage: Concrete,
    pub(crate) registers: R,
}

macro_rules! delegate_concrete {
    ($($name:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(#[inline(always)] fn $name(&mut self, $($arg: $ty),*) $(-> $ret)? { self.stage.$name($($arg),*) })*
    };
}

impl<R: RegisterFile> Stage for ConcreteFrame<R> {
    type Word = u64;
    type Cond = bool;
    type Miss = ();
    delegate_concrete! {
        constant(bits: u64) -> u64;
        load_u32(base: u64, offset: i32) -> u64;
        load_u64(base: u64, offset: i32) -> u64;
        store_u32(base: u64, offset: i32, value: u64);
        store_u64(base: u64, offset: i32, value: u64);
        load_table(address: u64) -> u64;
        add(a: u64, b: u64) -> u64;
        or(a: u64, b: u64) -> u64;
        add_imm(a: u64, imm: u64) -> u64;
        mul_imm(a: u64, imm: u64) -> u64;
        and_imm(a: u64, imm: u64) -> u64;
        or_imm(a: u64, imm: u64) -> u64;
        shr_imm(a: u64, shift: u32) -> u64;
        sar32_imm(a: u64, shift: u32) -> u64;
        eq_imm(a: u64, imm: u64) -> bool;
        below_imm(a: u64, imm: u64) -> bool;
        guard(cond: bool, miss: ()) -> Option<()>;
        property_base() -> u64;
    }
    #[inline(always)]
    fn choose(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> u64, otherwise: impl FnOnce(&mut Self) -> u64) -> u64 {
        if cond { then(self) } else { otherwise(self) }
    }
}

impl<R: RegisterFile> Machine for ConcreteFrame<R> {
    #[inline(always)]
    fn register(&mut self, register: crate::bytecode::Register) -> u64 { self.registers.get(register) }
    #[inline(always)]
    fn set_register(&mut self, register: crate::bytecode::Register, value: u64) { self.registers.set(register, value) }
    #[inline(always)]
    fn set_static(&mut self, register: crate::bytecode::Register, value: crate::Value) { self.registers.set(register, value.to_bits()) }
}

/// An instruction's meaning as data: what `step` interprets, decoded once from whichever
/// record carries it (a bytecode instruction, a lane record). Operands the instruction names
/// statically (registers, constants) are plain values, which every stage folds.
#[derive(Clone, Copy)]
pub(crate) enum Operation {
    /// `destination = source`.
    Move { destination: crate::bytecode::Register, source: crate::bytecode::Register },
    /// `destination = constant`, a value that needs no heap cell (`facts::immediate`).
    LoadConst { destination: crate::bytecode::Register, constant: crate::Value },
}

/// Where control goes after `step`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    /// The next instruction.
    Next,
}

/// The interpreter: each operation's semantics, over any machine (`Machine`). The lane runs
/// it on the activation's registers; the specializer runs it on its register state, which
/// emits the residual (Futamura's first projection), and `step` for the emitting machine is
/// the compiler (the second).
#[inline(always)]
pub(crate) fn step<M: Machine>(machine: &mut M, operation: Operation) -> Flow {
    match operation {
        Operation::Move { destination, source } => machine.copy_register(destination, source),
        Operation::LoadConst { destination, constant } => machine.set_static(destination, constant),
    }
    Flow::Next
}

/// A number result whose representation the semantics knows statically: an int32 or a
/// double. A join of arms of different representations takes the double.
#[derive(Clone, Copy)]
pub(crate) enum Number<I, F> {
    Int(I),
    Float(F),
}

/// The number domain of a machine: int32 and double words, the operations on them, typed
/// operand reads and the static facts that decide which apply. What the facts say (an operand
/// proved int32, a result read only through ToInt32) is static: the interpreter, which proves
/// nothing ahead, sees none and tests the values; the specializer folds them.
pub(crate) trait Numbers {
    type Int: Copy;
    type Float: Copy;
    type Cond: Copy;
    type Miss: Copy;
    /// Whether `operand` is proved an int32.
    fn known_int(&self, operand: crate::bytecode::Operand) -> bool;
    /// Whether `operand` is held as a double, or is a constant no int32 holds.
    fn known_float(&self, operand: crate::bytecode::Operand) -> bool;
    /// Whether `register`'s next value is read only through ToInt32 (`| 0` and its kin).
    fn truncated(&self, register: crate::bytecode::Register) -> bool;
    fn int_operand(&mut self, operand: crate::bytecode::Operand, miss: Self::Miss) -> Option<Self::Int>;
    fn float_operand(&mut self, operand: crate::bytecode::Operand, miss: Self::Miss) -> Option<Self::Float>;
    /// Whether both operands are int32s.
    fn int_operands(&mut self, left: crate::bytecode::Operand, right: crate::bytecode::Operand) -> Self::Cond;
    /// `operation` on two int32s, and whether its exact result leaves int32.
    fn int_checked(&mut self, operation: super::operations::NumberArithmetic, left: Self::Int, right: Self::Int) -> (Self::Int, Self::Cond);
    /// A sum or difference of int32s, wrapped to int32.
    fn int_wrapping(&mut self, operation: super::operations::NumberArithmetic, left: Self::Int, right: Self::Int) -> Self::Int;
    /// Whether the int32 product `result` of `left` and `right` is a zero no int32 holds, -0.
    fn negative_zero_product(&mut self, result: Self::Int, left: Self::Int, right: Self::Int) -> Self::Cond;
    fn or_cond(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    fn float_op(&mut self, operation: super::operations::NumberArithmetic, left: Self::Float, right: Self::Float) -> Self::Float;
    /// ToInt32 of a double; `miss` where the machine does not compute it exactly.
    fn float_to_int32(&mut self, number: Self::Float, miss: Self::Miss) -> Option<Self::Int>;
    /// Continues where `cond` does not hold.
    fn guard_not(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
    /// The number `then` makes where `cond` holds, else `otherwise`'s, each arm's register
    /// facts joined after; the second arm is the rare one.
    fn choose_number(
        &mut self,
        cond: Self::Cond,
        then: impl FnOnce(&mut Self) -> Option<Number<Self::Int, Self::Float>>,
        otherwise: impl FnOnce(&mut Self) -> Option<Number<Self::Int, Self::Float>>,
    ) -> Option<Number<Self::Int, Self::Float>>;
    fn set_number(&mut self, register: crate::bytecode::Register, number: Number<Self::Int, Self::Float>);
}

/// `destination = left op right` for Number operands, JavaScript's arithmetic on Numbers:
/// int32 operands give an int32 where the exact result is one, else the IEEE double; a
/// division takes the doubles. Operands proved int32 stay int32 and leave at an overflow or a
/// product's -0, as C keeps them; a sum or difference read only through ToInt32 wraps.
/// `miss` for an operand that is not a Number.
#[inline(always)]
pub(crate) fn arithmetic<N: Numbers>(
    machine: &mut N,
    operation: super::operations::NumberArithmetic,
    destination: crate::bytecode::Register,
    left: crate::bytecode::Operand,
    right: crate::bytecode::Operand,
    miss: N::Miss,
) -> Option<()> {
    use super::operations::NumberArithmetic;
    let doubles = |machine: &mut N| -> Option<Number<N::Int, N::Float>> {
        let (a, b) = (machine.float_operand(left, miss)?, machine.float_operand(right, miss)?);
        Some(Number::Float(machine.float_op(operation, a, b)))
    };
    let wraps = matches!(operation, NumberArithmetic::Add | NumberArithmetic::Subtract) && machine.truncated(destination);
    let result = if operation == NumberArithmetic::Divide || machine.known_float(left) || machine.known_float(right) {
        doubles(machine)?
    } else if machine.known_int(left) && machine.known_int(right) {
        let (a, b) = (machine.int_operand(left, miss)?, machine.int_operand(right, miss)?);
        if wraps {
            Number::Int(machine.int_wrapping(operation, a, b))
        } else {
            let (result, overflow) = machine.int_checked(operation, a, b);
            machine.guard_not(overflow, miss)?;
            if operation == NumberArithmetic::Multiply {
                let negative_zero = machine.negative_zero_product(result, a, b);
                machine.guard_not(negative_zero, miss)?;
            }
            Number::Int(result)
        }
    } else {
        let ints = machine.int_operands(left, right);
        machine.choose_number(ints, |machine| {
            let (a, b) = (machine.int_operand(left, miss)?, machine.int_operand(right, miss)?);
            if wraps {
                return Some(Number::Int(machine.int_wrapping(operation, a, b)));
            }
            let (result, overflow) = machine.int_checked(operation, a, b);
            let inexact = if operation == NumberArithmetic::Multiply {
                let negative_zero = machine.negative_zero_product(result, a, b);
                machine.or_cond(overflow, negative_zero)
            } else {
                overflow
            };
            machine.choose_number(inexact, doubles, |_| Some(Number::Int(result)))
        }, |machine| match doubles(machine)? {
            Number::Float(number) if wraps => Some(Number::Int(machine.float_to_int32(number, miss)?)),
            number => Some(number),
        })?
    };
    machine.set_number(destination, result);
    Some(())
}

/// The interpreter's number machine for two operand values and a result
/// (`arithmetic_values`): operand register 0 is the left value, any other the right.
struct ValueOperands {
    left: crate::Value,
    right: crate::Value,
    result: crate::Value,
}

impl ValueOperands {
    #[inline(always)]
    fn value(&self, operand: crate::bytecode::Operand) -> crate::Value {
        if operand.register_index() == Some(0) { self.left } else { self.right }
    }
}

impl Numbers for ValueOperands {
    type Int = i32;
    type Float = f64;
    type Cond = bool;
    type Miss = ();
    #[inline(always)]
    fn known_int(&self, _: crate::bytecode::Operand) -> bool { false }
    #[inline(always)]
    fn known_float(&self, _: crate::bytecode::Operand) -> bool { false }
    #[inline(always)]
    fn truncated(&self, _: crate::bytecode::Register) -> bool { false }
    #[inline(always)]
    fn int_operand(&mut self, operand: crate::bytecode::Operand, _: ()) -> Option<i32> { self.value(operand).as_int() }
    #[inline(always)]
    fn float_operand(&mut self, operand: crate::bytecode::Operand, _: ()) -> Option<f64> { self.value(operand).as_number() }
    #[inline(always)]
    fn int_operands(&mut self, _: crate::bytecode::Operand, _: crate::bytecode::Operand) -> bool {
        crate::Value::int_pair(self.left, self.right).is_some()
    }
    #[inline(always)]
    fn int_checked(&mut self, operation: super::operations::NumberArithmetic, left: i32, right: i32) -> (i32, bool) {
        match super::operations::checked_int32_arithmetic(operation, left, right) {
            Some(result) => (result, false),
            None => (0, true),
        }
    }
    #[inline(always)]
    fn int_wrapping(&mut self, operation: super::operations::NumberArithmetic, left: i32, right: i32) -> i32 {
        if operation == super::operations::NumberArithmetic::Add { left.wrapping_add(right) } else { left.wrapping_sub(right) }
    }
    #[inline(always)]
    fn negative_zero_product(&mut self, result: i32, left: i32, right: i32) -> bool { result == 0 && (left | right) < 0 }
    #[inline(always)]
    fn or_cond(&mut self, a: bool, b: bool) -> bool { a || b }
    #[inline(always)]
    fn float_op(&mut self, operation: super::operations::NumberArithmetic, left: f64, right: f64) -> f64 {
        super::operations::number_arithmetic_result(operation, left, right)
    }
    #[inline(always)]
    fn float_to_int32(&mut self, number: f64, _: ()) -> Option<i32> { Some(crate::value::number_to_u32(number) as i32) }
    #[inline(always)]
    fn guard_not(&mut self, cond: bool, _: ()) -> Option<()> { (!cond).then_some(()) }
    #[inline(always)]
    fn choose_number(
        &mut self,
        cond: bool,
        then: impl FnOnce(&mut Self) -> Option<Number<i32, f64>>,
        otherwise: impl FnOnce(&mut Self) -> Option<Number<i32, f64>>,
    ) -> Option<Number<i32, f64>> {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn set_number(&mut self, _: crate::bytecode::Register, number: Number<i32, f64>) {
        self.result = match number {
            Number::Int(int) => crate::Value::integer(int),
            Number::Float(number) => crate::Value::number(number),
        };
    }
}

/// `arithmetic` of two values, as the interpreter computes `left op right`: `None` where
/// either is not a Number.
#[inline(always)]
pub(crate) fn arithmetic_values(operation: super::operations::NumberArithmetic, left: crate::Value, right: crate::Value) -> Option<crate::Value> {
    let mut machine = ValueOperands { left, right, result: crate::Value::UNDEFINED };
    let (left, right) = (crate::bytecode::Operand::register(0), crate::bytecode::Operand::register(1));
    arithmetic(&mut machine, operation, 2, left, right, ())?;
    Some(machine.result)
}
