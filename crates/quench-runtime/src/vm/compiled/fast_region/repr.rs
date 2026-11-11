use super::*;

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

/// The representation of every register the region names, to a fixed point: where the
/// compile began (`ints`, `doubles`; dead at the header: none yet), then each writer's result.
pub(super) fn representations(region: &LoopRegion, decoded: &[Decoded], widened: &[usize], seen: [&[Register]; SEEN]) -> Reprs {
    let header = region.header;
    let mut reprs = Reprs::new();
    for row in decoded {
        for register in row.registers() {
            let start = match (region.is_live(header, register), seen[0].contains(&register), seen[1].contains(&register)) {
                (false, ..) => Repr::Unseen,
                (_, true, _) => Repr::Int,
                (_, _, true) => Repr::Double,
                _ => Repr::Tagged,
            };
            reprs.insert(register, start);
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (k, row) in decoded.iter().enumerate() {
            let Some(dst) = row.dst else { continue };
            let result = result_repr(row, &reprs, widened.contains(&(header as usize + k)), region.number_registers().contains(&dst));
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
