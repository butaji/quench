//! TypeScript's declared parameter types (`Function::declared_parameters`) as a body's entry facts.
//! A declared type is no proof (TypeScript is unsound): the entry checks each declared parameter
//! once and every row after it trusts the check. A word the declaration lies about fails the
//! entry (`LaneExit::Entry`): the body is not entered and the interpreter runs the activation, so
//! a lie costs an entry, never a wrong result.
//!
//! A declared Number enters as a double (`seen`) where nothing observed it an int, so the existing
//! Number guard of its home (`link::entry_rows`) proves it and the rows of its entry web read it
//! unboxed; its first write starts a web of its own (`repr::representations`), so the fact lasts
//! until then. The other sets a tag alone decides are `EntryGuard`s, kept for parameters the
//! function never writes, which hold the fact at every row.

use super::*;
use super::super::declared_checks::{entry_checks, ARRAY, BASE_TESTED, EntryCheck, OBJECT_PLAIN, STRING};
use super::super::lane_region::scalar::parameter_register;
use quench_runtime::bytecode::{Function, Operand};
use quench_runtime::staged::value::language_typeof_index;
use quench_runtime::staged::{DecodeContext, Operation};
use quench_runtime::typed_ir::Type;
use quench_runtime::ValueType;
use super::function::ClassProto;

const fn language(kind: ValueType) -> u16 {
    1 << kind as u16
}

/// The stencil's language bits (`quench_bjit::shared_words::language`) are these types' bits.
const _: () = {
    use quench_bjit::shared_words::language as bits;
    assert!(language(ValueType::Undefined) == bits::UNDEFINED && language(ValueType::Null) == bits::NULL);
    assert!(language(ValueType::Boolean) == bits::BOOLEAN && language(ValueType::Number) == bits::NUMBER);
    assert!(language(ValueType::Object) == bits::OBJECT && language(ValueType::Function) == bits::FUNCTION);
    assert!(language(ValueType::String) == bits::STRING);
    assert!(bits::ARRAY & bits::OBJECT_PLAIN == 0 && bits::ARRAY & (bits::OBJECT | bits::FUNCTION) == 0);
    assert!(bits::OBJECT_PLAIN & (bits::OBJECT | bits::FUNCTION) == 0);
    use quench_runtime::staged::value::language as tier;
    assert!(tier::UNDEFINED == bits::UNDEFINED && tier::NULL == bits::NULL && tier::BOOLEAN == bits::BOOLEAN);
    assert!(tier::NUMBER == bits::NUMBER && tier::OBJECT == bits::OBJECT && tier::FUNCTION == bits::FUNCTION);
    assert!(tier::STRING == bits::STRING);
    assert!(tier::ARRAY == bits::ARRAY && tier::OBJECT_PLAIN == bits::OBJECT_PLAIN);
};

/// What a declared parameter's entry check gives the b-jit's entry: exactly a Number, proved through the
/// register's representation, or a set of language types (`language` bits), tested by tag at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::compiled) enum DeclaredFact {
    Number,
    Languages(u16),
}

impl DeclaredFact {
    /// The fact `check` gives where this tier's entry test decides it; None for a check this tier does not
    /// decide by a tag. A string is the direct string family word, one tag test (`tag_languages`).
    pub(in crate::compiled) fn of(check: EntryCheck) -> Option<Self> {
        match check {
            EntryCheck::Number => Some(Self::Number),
            EntryCheck::String => Some(Self::Languages(STRING)),
            EntryCheck::Array => Some(Self::Languages(ARRAY)),
            EntryCheck::PlainObject => Some(Self::Languages(OBJECT_PLAIN)),
            EntryCheck::Languages(languages) => (languages & !BASE_TESTED == 0).then_some(Self::Languages(languages)),
        }
    }
}

/// Each parameter of `function` with a checkable declared fact, by the register it enters in.
fn declared(function: &Function) -> impl Iterator<Item = (Register, DeclaredFact)> + '_ {
    entry_checks(function)
        .filter_map(|(index, check)| Some((parameter_register(function, index)?, DeclaredFact::of(check)?)))
}

/// The registers some instruction of `function` writes.
fn written(function: &Function) -> Bits {
    let mut written = Bits::default();
    for instruction in function.instructions() {
        for register in instruction.written_registers() {
            written.insert(usize::from(register));
        }
    }
    written
}

/// The types the entry holds (`BodySpec::seen`, by register), each declared Number parameter a Number where
/// nothing observed it as an int or another Number. A body's entry web of the register ends at its first
/// write; a region is entered at its loop's header, where only a parameter the function never writes still
/// holds the value it entered with.
pub(super) fn seen(function: &Function, ending: Ending, seen: &[Type]) -> Vec<Type> {
    let written = (ending == Ending::Region).then(|| written(function));
    let mut types = seen.to_vec();
    for (register, _) in declared(function).filter(|(_, fact)| *fact == DeclaredFact::Number) {
        let kept = written.as_ref().is_none_or(|written| !written.contains(usize::from(register)));
        let slot = usize::from(register);
        if kept && types.get(slot).copied().unwrap_or(Type::ANY) == Type::ANY {
            if types.len() <= slot {
                types.resize(slot + 1, Type::ANY);
            }
            types[slot] = Type::NUMBER;
        }
    }
    types
}

/// An entry test of a declared parameter that no Number guard covers. The entry row reads the
/// register's frame slot word and continues only where its language type is one of `languages`
/// as its tag alone decides it: a Number (int32 or double word), `true`/`false`, `null`,
/// `undefined` (the hole is no `undefined`), a direct object family word
/// (`Value::is_direct_object_family`), a direct function word (`Value::direct_function_index`) or a
/// direct string word (`Value::direct_string_index`). Any other word, a heap-decided object or function
/// included, fails the entry with `LaneExit::Entry` before any row runs, as a Number guard's failure does
/// (`Leaves::open`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EntryGuard {
    pub(super) register: Register,
    pub(super) languages: u16,
}

/// The entry tests of `function`'s declared parameters that are no Number and that the function
/// never writes, so the fact holds at every row: what `link::entry_rows` emits after the
/// Number guards (`Heap::Declared`).
pub(super) fn entry_guards(function: &Function) -> Vec<EntryGuard> {
    let written = written(function);
    declared(function)
        .filter(|(register, _)| !written.contains(usize::from(*register)))
        .filter_map(|(register, fact)| match fact {
            DeclaredFact::Number => None,
            DeclaredFact::Languages(languages) => Some(EntryGuard { register, languages }),
        })
        .collect()
}

/// The declared Number parameters the function never writes: the entry's Number guard of each
/// (`link::entry_rows`) holds at every row, so a row reading it in its frame slot needs no proof of its own.
pub(super) fn entry_numbers(function: &Function) -> Vec<Register> {
    let written = written(function);
    declared(function).filter(|(register, fact)| *fact == DeclaredFact::Number && !written.contains(usize::from(*register))).map(|(register, _)| register).collect()
}

/// The registers the entry proves plain direct objects, never written: the `Languages(OBJECT_PLAIN)` parameters and
/// each class-typed receiver of `classes` (`class_prototypes`, a direct object the entry tests). `proofs::prove`
/// starts from them, so the inline-cache rows on them take the known-shape form (`Heap::IcKnownGet`,
/// `Heap::IcKnownSet`), whose record indexing assumes a plain object.
pub(super) fn exact_objects(function: &Function, classes: &[ClassProto]) -> Vec<Register> {
    let guards = entry_guards(function).into_iter().filter(|guard| guard.languages == OBJECT_PLAIN).map(|guard| guard.register);
    guards.chain(classes.iter().map(|proto| proto.register)).collect()
}

/// The prototypes the entry checks for a class-typed parameter (`DeclaredTypes::declared_parameter_class`): each
/// of `resolved` (`VM::class_prototypes`, observed where the body was compiled) whose parameter the function
/// never writes, so the receiver it entered with holds the fact until a row may change it (`proofs::prove`, which
/// seeds the entry's proof of each receiver's shape and link).
pub(super) fn class_prototypes(function: &Function, resolved: &[ClassProto]) -> Vec<ClassProto> {
    let written = written(function);
    resolved.iter().filter(|proto| !written.contains(usize::from(proto.register))).cloned().collect()
}

/// The named read `atom` of a class-typed parameter whose prototype holds the name as a data value (`Known::classes`,
/// already limited to the parameters the function never writes by `class_prototypes`): a held read of the receiver's
/// exact shape and prototype link, checked per use (`Heap::Held`). `proofs::prove` makes it a constant load where the
/// receiver's shape is proven at the row; the prototype's values are a dependency (`Folded::holder`). `None` for any
/// other base or name.
pub(super) fn class_member_read(known: Known, object: Option<Register>, atom: quench_runtime::bytecode::Atom, dst: Register) -> Option<Decoded> {
    let register = object?;
    let proto = known.classes.iter().find(|proto| proto.register == register)?;
    let (_, value) = proto.members.iter().find(|(member, _)| *member == atom)?;
    // The link names the class prototype itself, the holder: a chain of one.
    let folded = Folded { shape: proto.shape, link: proto.link, holder: proto.holder, depth: 1 };
    let row = Decoded::heap(Heap::Held, Some(dst), Some(Source::Register(register)), None);
    Some(Decoded { hole: *value, folded: Some(folded), class_read: true, ..row })
}

/// A `typeof` test of a register the entry proves exactly a direct function (`Languages(FUNCTION)`) or
/// exactly a direct object or array (`OBJECT_PLAIN` and `ARRAY` only: `typeof` of each is `'object'`): the
/// entry test admits no other word, and no instruction writes the register, so the test is that Boolean,
/// written to the destination as a constant. `None` for any other row.
pub(super) fn typeof_fold(function: &Function, instruction: WideInstruction) -> Option<Decoded> {
    if instruction.op().class() != Class::Unary || instruction.result_flags() != 0 {
        return None;
    }
    let (index, negated) = quench_runtime::bytecode::typeof_test(instruction.unary_operator())?;
    let name = language_typeof_index(guard_languages(function, instruction.register_b())?)?;
    let holds = (index as u64 == name) != negated;
    Some(Decoded::number(Kind::Move, Some(instruction.result_register()), Some(Source::Value(Value::boolean(holds).to_bits())), None, None))
}

/// The language set the entry test of `register` admits, where the register is a declared parameter
/// the function never writes (`entry_guards`): the set holds at every row.
fn guard_languages(function: &Function, register: Register) -> Option<u16> {
    entry_guards(function).into_iter().find(|guard| guard.register == register).map(|guard| guard.languages)
}

/// The truth every value of a language set `languages` has, where the set decides it: `null` and
/// `undefined` are falsy, a direct object or function is truthy (`Languages` admits no other word). A
/// set holding a Number, a Boolean or a string may be either, so it has no constant truth.
fn constant_truth(languages: u16) -> Option<bool> {
    const FALSY: u16 = language(ValueType::Null) | language(ValueType::Undefined);
    const TRUTHY: u16 = language(ValueType::Object) | language(ValueType::Function) | ARRAY | OBJECT_PLAIN;
    if languages & !FALSY == 0 {
        Some(false)
    } else if languages & !TRUTHY == 0 {
        Some(true)
    } else {
        None
    }
}

/// The language of a constant operand that is `null` or `undefined`, `None` for any other operand.
fn nullish_language(constants: &[Value], operand: Operand) -> Option<u16> {
    if operand.kind() != Some(OperandKind::Constant) {
        return None;
    }
    let word = constants.get(usize::from(operand.payload()))?.to_bits();
    if word == Value::NULL.to_bits() {
        Some(language(ValueType::Null))
    } else if word == Value::UNDEFINED.to_bits() {
        Some(language(ValueType::Undefined))
    } else {
        None
    }
}

/// The result of an equality test of a guarded register with `null` or `undefined` (`x ?? y` and `x?.y`
/// test `!== null` and `!== undefined` strictly; `x == null` loosely): `Some` where the register's language
/// set decides it. A loose test is equal to `null` and `undefined` alone (a direct object is no IsHTMLDDA
/// object, which a heap cell would be).
fn nullish_result(function: &Function, constants: &[Value], instruction: WideInstruction) -> Option<bool> {
    let (strict, negated) = quench_runtime::vm::operations::equality_operator(instruction.binary_operator_field())?;
    let (b, c) = (instruction.operand_b(), instruction.operand_c());
    let (tested, expected) = [(b, c), (c, b)].into_iter()
        .find_map(|(tested, other)| Some((tested.register_index()?, nullish_language(constants, other)?)))?;
    let languages = guard_languages(function, tested)?;
    let nullish = language(ValueType::Null) | language(ValueType::Undefined);
    let target = if strict { expected } else { nullish };
    let equal = if languages & !target == 0 {
        true
    } else if languages & target == 0 {
        false
    } else {
        return None;
    };
    Some(equal != negated)
}

/// A conditional jump whose outcome every row of a declared register decides, as an unconditional
/// jump: to its target where taken, else to the next row (as `Nop` is). Covers `JumpTrue`/`JumpFalse` on
/// the register's truthiness (`constant_truth`) and the equality jumps `??`, `?.` and `== null` take
/// (`JumpBinaryFalse`, `nullish_result`). `None` for any other row, or where the entry does not decide it.
pub(super) fn branch_fold(function: &Function, constants: &[Value], pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    let taken = match Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY) {
        Some(Operation::JumpIf { register, taken }) => constant_truth(guard_languages(function, register)?)? == taken,
        _ if instruction.op().class() == Class::JumpBinaryFalse => !nullish_result(function, constants, instruction)?,
        _ => return None,
    };
    let target = if taken { instruction.jump_target() } else { pc + 1 };
    Some(Decoded::number(Kind::Jump, None, None, None, Some(target)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fact a declared set gives the b-jit's entry, through the shared entry check.
    fn fact(declared: u16) -> Option<DeclaredFact> {
        EntryCheck::of(declared).and_then(DeclaredFact::of)
    }

    #[test]
    fn a_declared_number_is_proved_through_its_representation() {
        assert_eq!(fact(language(ValueType::Number)), Some(DeclaredFact::Number));
    }

    #[test]
    fn tag_decided_sets_are_tested_at_entry() {
        for set in [language(ValueType::Boolean), language(ValueType::Null) | language(ValueType::Undefined),
            language(ValueType::Object), language(ValueType::Function), language(ValueType::Number) | language(ValueType::Undefined),
            ARRAY, OBJECT_PLAIN, ARRAY | OBJECT_PLAIN | language(ValueType::Undefined),
            language(ValueType::String), language(ValueType::String) | language(ValueType::Undefined)] {
            assert_eq!(fact(set), Some(DeclaredFact::Languages(set)));
        }
    }

    #[test]
    fn sets_a_tag_does_not_decide_get_no_check() {
        assert_eq!(fact(u16::MAX), None);
        assert_eq!(fact(language(ValueType::Number) | language(ValueType::BigInt)), None);
        assert_eq!(fact(language(ValueType::Symbol)), None);
        assert_eq!(fact(0), None);
    }

    #[test]
    fn a_language_set_decides_its_truth_only_where_every_member_agrees() {
        let [null, undefined, object, function, boolean] = [ValueType::Null, ValueType::Undefined, ValueType::Object, ValueType::Function, ValueType::Boolean].map(language);
        assert_eq!(constant_truth(null | undefined), Some(false));
        assert_eq!(constant_truth(object | function), Some(true));
        assert_eq!(constant_truth(boolean), None);
        assert_eq!(constant_truth(null | object), None);
    }
}
