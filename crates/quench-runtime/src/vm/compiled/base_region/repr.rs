use super::*;
use crate::typed_ir::{Representation, Type};
use crate::vm::dispatch_fast::operand_feedback::{OperandClass, OperandKinds};

/// The representation of each web, by its number: `None` is unseen (a web no writer reaches), else
/// how its values are held. A register takes the widest of what its writers produce.
pub(super) type Reprs = Vec<Option<Representation>>;

pub(super) fn operand_repr(reprs: &[Option<Representation>], source: Option<Source>) -> Option<Representation> {
    match source {
        Some(Source::Register(register)) => reprs.get(usize::from(register)).copied().unwrap_or(Some(Representation::Tagged)),
        Some(Source::Constant(word)) if word >> HALF == INT_HIGH => Some(Representation::Int32),
        Some(Source::Constant(_)) => Some(Representation::Float64),
        Some(Source::Value(_)) => Some(Representation::Tagged),
        None => None,
    }
}

/// The variant of row a site gets, chosen from its operand feedback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Variant {
    /// The int32-speculating row: guards int32 operands, exits at the PC on any other.
    Int32,
    /// The double row: takes Numbers, ints converted; exits at the PC on any other operand.
    Double,
    /// The string-concatenation row, whose both operands are guarded as strings.
    Concat,
    /// The generic row, whose guard exits at the PC for anything but Numbers.
    Generic,
}

impl Variant {
    pub(super) fn speculates(self) -> bool {
        self == Self::Int32
    }
}

/// The variant of each feedback class: a site never declined takes the int32 row it was
/// compiled with, the lattice's higher classes the wider rows.
const SELECTION: [(OperandClass, Variant); 4] = [
    (OperandClass::Unseen, Variant::Int32),
    (OperandClass::Int32, Variant::Int32),
    (OperandClass::Number, Variant::Double),
    (OperandClass::Any, Variant::Generic),
];

/// The kinds whose generic site becomes its concatenation variant once it has met a string.
const STRING_VARIANTS: [(Kind, Variant); 1] = [(Kind::Add, Variant::Concat)];

fn selected(class: OperandClass) -> Variant {
    SELECTION.iter().find(|(candidate, _)| *candidate == class).map_or(Variant::Generic, |(_, variant)| *variant)
}

/// The variant of the row of `kind` at a site with `kinds`.
pub(super) fn variant_of(kind: Kind, kinds: Option<&OperandKinds>) -> Variant {
    let Some(kinds) = kinds else { return Variant::Int32 };
    match selected(kinds.class()) {
        Variant::Generic if kinds.has_string() => STRING_VARIANTS.iter().find(|(candidate, _)| *candidate == kind).map_or(Variant::Generic, |(_, variant)| *variant),
        variant => variant,
    }
}

/// The representation of a heap row's result: a tagged word unless it reads a Number, then an
/// int32 (a double once widened).
fn heap_repr(numbers: bool, widened: bool) -> Option<Representation> {
    match (numbers, widened) {
        (false, _) => Some(Representation::Tagged),
        (true, false) => Some(Representation::Int32),
        (true, true) => Some(Representation::Float64),
    }
}

/// The representation a row's result needs: a step's result is whatever the interpreter's
/// operation wrote, a tagged word, as is a logical negation's boolean; a Number negation gives a
/// double (an int32 zero negates to `-0`); a division, a double operand or a `widened` row (its int32
/// speculation failed) gives a double; a bitwise row an int32 unless widened.
pub(super) fn result_repr(row: &Decoded, reprs: &[Option<Representation>], widened: bool, numbers: bool) -> Option<Representation> {
    if row.step.is_some() {
        return Some(Representation::Tagged);
    }
    if let Some(heap) = row.heap {
        return if heap.yields_tagged() { Some(Representation::Tagged) } else if heap.yields_double() { Some(Representation::Float64) } else { heap_repr(numbers || read_numbers(row), widened || read_doubles(row)) };
    }
    kind_repr(row.kind, operand_repr(reprs, row.a).max(operand_repr(reprs, row.b)), widened)
}

/// Whether a field read's cache site observed only Numbers in its own slot (`own_slot_type`): its
/// row delivers a Number, unboxed where the destination is pinned, and leaves on any other word.
fn read_numbers(row: &Decoded) -> bool {
    matches!(row.heap, Some(Heap::Site | Heap::ThisSite)) && row.slot.is_some_and(|ty| ty.is_subtype(crate::typed_ir::Type::NUMBER))
}

/// Whether that slot also observed a non-int32 Number, so the read delivers a double.
fn read_doubles(row: &Decoded) -> bool {
    read_numbers(row) && row.slot.is_some_and(|ty| !ty.is_subtype(crate::typed_ir::Type::INT32))
}

/// The representation a non-heap row of `kind` writes, given the greatest representation of its `operands`.
fn kind_repr(kind: Kind, operands: Option<Representation>, widened: bool) -> Option<Representation> {
    match kind {
        Kind::Not => Some(Representation::Tagged),
        _ if operands == Some(Representation::Tagged) => Some(Representation::Tagged),
        Kind::Neg | Kind::Sqrt | Kind::Floor | Kind::Ceil | Kind::Trunc | Kind::Abs | Kind::Round => Some(Representation::Float64),
        Kind::Move | Kind::Const => operands,
        Kind::Div => Some(Representation::Float64),
        Kind::Add | Kind::Sub | Kind::Mul | Kind::Rem => if widened || operands == Some(Representation::Float64) { Some(Representation::Float64) } else { Some(Representation::Int32) },
        _ if widened => Some(Representation::Tagged),
        _ => Some(Representation::Int32),
    }
}

/// Whether the row at `pc` is widened: named by a leave, recorded as declining its int32 step
/// (`kinds`, its site's, where it is a caller instruction), or a loaded value among widened ones.
fn row_widened(widened: &[usize], pc: usize, with_loads: bool, kinds: Option<&OperandKinds>) -> bool {
    with_loads || widened.contains(&pc) || kinds.is_some_and(|kinds| !selected(kinds.class()).speculates())
}

/// Whether a loaded value (a heap row) is among the widened PCs.
fn loads_widened(decoded: &[Decoded], header: u32, widened: &[usize]) -> bool {
    decoded.iter().enumerate().any(|(k, row)| row.heap.is_some() && widened.contains(&(header as usize + k)))
}

/// Where a web starts: unseen where the region is not entered with it, else the narrowest representation
/// that holds the type the compile observed of its register (`seen`), tagged where none does.
fn entry_repr(entered: bool, register: Register, seen: &[Type]) -> Option<Representation> {
    if !entered {
        return None;
    }
    let observed = seen.get(usize::from(register)).copied().unwrap_or(Type::ANY);
    Some(if Representation::Int32.accepts(observed) {
        Representation::Int32
    } else if Representation::Float64.accepts(observed) {
        Representation::Float64
    } else {
        Representation::Tagged
    })
}

/// The representation of every web the region names, to a fixed point: where the compile
/// began (`seen`, the entry's observed types; dead at the header: none yet), then each writer's result.
pub(super) fn representations(span: &Span, decoded: &[Decoded], (widened, kinds): (&[usize], &[Option<OperandKinds>]), seen: &[Type], webs: &Webs) -> Reprs {
    let header = span.header;
    // One loaded value that left for its int32 speculation widens every loaded value at once: the
    // next compile need not discover them one leave at a time.
    let loads_widened = loads_widened(decoded, header, widened);
    let entered = webs.entry_live(span);
    let mut reprs: Reprs = webs.orig.iter().enumerate().map(|(web, register)| entry_repr(entered.contains(&web), *register, seen)).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (k, row) in decoded.iter().enumerate() {
            if let Some(also) = row.also.filter(|also| reprs[usize::from(*also)] < Some(Representation::Tagged)) {
                reprs[usize::from(also)] = Some(Representation::Tagged);
                changed = true;
            }
            let Some(dst) = row.dst else { continue };
            let numbers = span.number_registers().contains(&webs.orig[usize::from(dst)]) || row.global.is_some_and(Access::numeric);
            let result = result_repr(row, &reprs, row_widened(widened, header as usize + k, loads_widened && row.heap.is_some(), kinds.get(k).and_then(Option::as_ref)), numbers);
            if reprs[usize::from(dst)] < result {
                reprs[usize::from(dst)] = result;
                changed = true;
            }
        }
    }
    reprs
}

/// Keeps each site read's slot-type proof (`Decoded::typed`) only where a row drops a check on it: an
/// inline-cache read of a receiver every path proved (`Heap::IcKnownGet`) into a pinned double, in code that
/// makes no call or helper step.
///
/// INVARIANT (one fact store): a check the b-jit drops is backed by a fact-store proof and the dependency
/// that keeps it true. Here the proof is the slot's observed type, a Number type (`NativeFieldLoad::ty`, the
/// type `Heap::slot_type` maintains on every store), and the dependency is the same one the u-jit's code holds
/// for it (`SlotDependency`, subscribed through `Body::slot_types` as `StableFacts::slots`): a store that widens
/// the slot retires the code before its next entry. Inside one run nothing the rows do widens it (an in-place
/// store writes only what the slot's type holds, or leaves through the store gate); only a call or a helper
/// step could, so code that makes one keeps the tested read.
pub(super) fn keep_typed_reads(homes: &Homes, decoded: &mut [Decoded]) {
    let calls = decoded.iter().any(Decoded::makes_calls);
    for row in decoded.iter_mut() {
        let double = row.dst.is_some_and(|dst| matches!(homes.place(dst), Place::PinF(_)));
        let known = row.heap == Some(Heap::Site) && row.ic.is_some() && row.proven;
        if calls || !known || !double {
            row.typed = None;
        }
    }
}

/// Whether the row takes general (tagged word) semantics: its result or an operand of a
/// relation is no int32.
pub(super) fn is_general(row: &Decoded, reprs: &[Option<Representation>]) -> bool {
    match row.dst {
        Some(dst) => reprs[usize::from(dst)] != Some(Representation::Int32),
        None => operand_repr(reprs, row.a).max(operand_repr(reprs, row.b)) >= Some(Representation::Float64),
    }
}

/// The webs of `repr`, the most named first.
pub(super) fn ranked(decoded: &[Decoded], reprs: &[Option<Representation>], repr: Option<Representation>) -> Vec<Register> {
    let mut uses = vec![0usize; reprs.len()];
    for row in decoded {
        for web in row.registers() {
            uses[usize::from(web)] += 1;
        }
    }
    let mut ranked: Vec<(Register, usize)> = (0..reprs.len()).filter(|web| uses[*web] > 0 && reprs[*web] == repr).map(|web| (web as Register, uses[web])).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.into_iter().map(|(web, _)| web).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INT: OperandKinds = OperandKinds::INT32;

    fn variant(kind: Kind, kinds: OperandKinds) -> Variant {
        variant_of(kind, Some(&kinds))
    }

    #[test]
    fn a_site_that_never_declined_takes_the_int32_row() {
        assert_eq!(variant(Kind::Add, OperandKinds::NONE), Variant::Int32);
        assert_eq!(variant(Kind::Add, INT), Variant::Int32);
        assert_eq!(variant_of(Kind::Add, None), Variant::Int32);
    }

    #[test]
    fn a_number_site_takes_the_double_row() {
        assert_eq!(variant(Kind::Mul, INT.union(OperandKinds::DOUBLE)), Variant::Double);
        assert_eq!(variant(Kind::Add, INT.union(OperandKinds::WIDENED)), Variant::Double);
    }

    #[test]
    fn a_string_on_add_takes_the_concatenation_row() {
        assert_eq!(variant(Kind::Add, OperandKinds::STRING), Variant::Concat);
        assert_eq!(variant(Kind::Add, INT.union(OperandKinds::STRING)), Variant::Concat);
    }

    #[test]
    fn other_kinds_take_the_generic_row() {
        assert_eq!(variant(Kind::Sub, OperandKinds::STRING), Variant::Generic);
        assert_eq!(variant(Kind::Add, OperandKinds::OTHER), Variant::Generic);
        assert_eq!(variant(Kind::Lt, OperandKinds::BIGINT), Variant::Generic);
    }

    /// A proven site read into a pinned double keeps its slot-type proof (the dependency the code holds and
    /// the row that drops the tag test); a read into a frame slot, an unproven receiver, or code that makes a
    /// helper step does not.
    #[test]
    fn a_typed_read_keeps_its_proof_only_where_a_check_drops() {
        let dependency = crate::code_dependencies::SlotDependency { shape: 7, slot: 2, ty: Type::NUMBER };
        let read = |dst: Register, proven: bool| Decoded { ic: Some(7), proven, typed: Some(dependency), ..Decoded::heap(Heap::Site, Some(dst), Some(Source::Register(2)), None) };
        // Web 0 holds a pinned double, web 1 a frame slot; web 2 is the receiver.
        let homes = Homes::of(&[], &[0], vec![0, 1, 2], &[Vec::new(), Vec::new(), Vec::new()]);
        let mut rows = [read(0, true), read(1, true), read(0, false)];
        keep_typed_reads(&homes, &mut rows);
        assert_eq!(rows.iter().map(|row| row.typed.is_some()).collect::<Vec<_>>(), [true, false, false]);
        let mut calling = [read(0, true), Decoded::step(Step::Helper, None)];
        keep_typed_reads(&homes, &mut calling);
        assert!(calling[0].typed.is_none(), "a helper step may widen the slot");
    }

    #[test]
    fn only_the_int32_row_speculates() {
        assert!(Variant::Int32.speculates());
        assert!(![Variant::Double, Variant::Concat, Variant::Generic].iter().any(|variant| variant.speculates()));
    }
}
