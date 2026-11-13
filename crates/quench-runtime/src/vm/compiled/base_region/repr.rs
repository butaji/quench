use super::*;
use crate::vm::dispatch_fast::operand_feedback::OperandClass;

/// How the values of a register are held, from the narrowest to the widest: a raw int32, a
/// Number as `f64`, a tagged word. A register takes the widest of what its writers produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Repr {
    Unseen,
    Int,
    Double,
    Tagged,
}

pub(super) type Reprs = BTreeMap<Register, Repr>;

pub(super) fn operand_repr(reprs: &Reprs, source: Option<Source>) -> Repr {
    match source {
        Some(Source::Register(register)) => reprs.get(&register).copied().unwrap_or(Repr::Tagged),
        Some(Source::Constant(word)) if word >> HALF == INT_HIGH => Repr::Int,
        Some(Source::Constant(_)) => Repr::Double,
        None => Repr::Unseen,
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
fn heap_repr(numbers: bool, widened: bool) -> Repr {
    match (numbers, widened) {
        (false, _) => Repr::Tagged,
        (true, false) => Repr::Int,
        (true, true) => Repr::Double,
    }
}

/// The representation a row's result needs: a division, a double operand or a `widened` row
/// (its int32 speculation failed) gives a double; a bitwise row an int32 unless widened.
pub(super) fn result_repr(row: &Decoded, reprs: &Reprs, widened: bool, numbers: bool) -> Repr {
    if row.heap.is_some() {
        return heap_repr(numbers, widened);
    }
    let operands = operand_repr(reprs, row.a).max(operand_repr(reprs, row.b));
    match row.kind {
        _ if operands == Repr::Tagged => Repr::Tagged,
        Kind::Move | Kind::Const => operands,
        Kind::Div => Repr::Double,
        Kind::Add | Kind::Sub | Kind::Mul => if widened || operands == Repr::Double { Repr::Double } else { Repr::Int },
        _ if widened => Repr::Tagged,
        _ => Repr::Int,
    }
}

/// Whether the row at `pc` is widened: named by a leave, recorded as declining its int32 step
/// (`kinds`), or a loaded value among widened ones.
fn row_widened(widened: &[usize], pc: usize, with_loads: bool, kinds: Option<&OperandKinds>) -> bool {
    with_loads || widened.contains(&pc) || kinds.is_some_and(|kinds| !selected(kinds.class()).speculates())
}

/// Whether a loaded value (a heap row) is among the widened PCs.
fn loads_widened(decoded: &[Decoded], header: u32, widened: &[usize]) -> bool {
    decoded.iter().enumerate().any(|(k, row)| row.heap.is_some() && widened.contains(&(header as usize + k)))
}

/// The representation of every register the region names, to a fixed point: where the
/// compile began (`seen`, the live registers' sample; dead at the header: none yet), then each
/// writer's result, which the sites' feedback (`kinds`, per row) widens where it recorded a
/// declined int32 step.
pub(super) fn representations(region: &LoopRegion, decoded: &[Decoded], widened: &[usize], seen: [&[Register]; SEEN], kinds: &[OperandKinds], webs: &Webs) -> Reprs {
    let header = region.header;
    // One loaded value that left for its int32 speculation widens every loaded value at once: the
    // next compile need not discover them one leave at a time.
    let loads_widened = loads_widened(decoded, header, widened);
    let mut reprs = Reprs::new();
    let entered = webs.entry_live(region);
    for row in decoded {
        for web in row.registers() {
            let register = webs.orig[usize::from(web)];
            let start = match (entered.contains(&usize::from(web)), seen[0].contains(&register), seen[1].contains(&register)) {
                (false, ..) => Repr::Unseen,
                (_, true, _) => Repr::Int,
                (_, _, true) => Repr::Double,
                _ => Repr::Tagged,
            };
            reprs.insert(web, start);
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (k, row) in decoded.iter().enumerate() {
            let Some(dst) = row.dst else { continue };
            let result = result_repr(row, &reprs, row_widened(widened, header as usize + k, loads_widened && row.heap.is_some(), kinds.get(k)), region.number_registers().contains(&webs.orig[usize::from(dst)]));
            if reprs[&dst] < result {
                reprs.insert(dst, result);
                changed = true;
            }
        }
    }
    reprs
}

/// Whether the row takes general (tagged word) semantics: its result or an operand of a
/// relation is no int32.
pub(super) fn is_general(row: &Decoded, reprs: &Reprs) -> bool {
    match row.dst {
        Some(dst) => reprs[&dst] != Repr::Int,
        None => operand_repr(reprs, row.a).max(operand_repr(reprs, row.b)) >= Repr::Double,
    }
}

/// The registers of `repr`, the most named first.
pub(super) fn ranked(decoded: &[Decoded], reprs: &Reprs, repr: Repr) -> Vec<Register> {
    let mut uses: BTreeMap<Register, usize> = BTreeMap::new();
    for row in decoded {
        for register in row.registers() {
            *uses.entry(register).or_default() += 1;
        }
    }
    let mut ranked: Vec<_> = uses.into_iter().filter(|(register, _)| reprs[register] == repr).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.into_iter().map(|(register, _)| register).collect()
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

    #[test]
    fn only_the_int32_row_speculates() {
        assert!(Variant::Int32.speculates());
        assert!(![Variant::Double, Variant::Concat, Variant::Generic].iter().any(|variant| variant.speculates()));
    }
}
