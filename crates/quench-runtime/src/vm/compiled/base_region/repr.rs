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

/// The representation of each web, by its number.
pub(super) type Reprs = Vec<Repr>;

pub(super) fn operand_repr(reprs: &[Repr], source: Option<Source>) -> Repr {
    match source {
        Some(Source::Register(register)) => reprs.get(usize::from(register)).copied().unwrap_or(Repr::Tagged),
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
pub(super) fn result_repr(row: &Decoded, reprs: &[Repr], widened: bool, numbers: bool) -> Repr {
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

/// Whether the row at `pc` is widened: named by a leave, or a loaded value among widened ones.
fn row_widened(widened: &[usize], pc: usize, with_loads: bool) -> bool {
    with_loads || widened.contains(&pc)
}

/// Whether a loaded value (a heap row) is among the widened PCs.
fn loads_widened(decoded: &[Decoded], header: u32, widened: &[usize]) -> bool {
    decoded.iter().enumerate().any(|(k, row)| row.heap.is_some() && widened.contains(&(header as usize + k)))
}

/// Where a web starts: unseen where the region is not entered with it, else what the compile
/// observed of its register (`seen`: the ints, then the doubles).
fn entry_repr(entered: bool, register: Register, seen: [&[Register]; SEEN]) -> Repr {
    match (entered, seen[0].contains(&register), seen[1].contains(&register)) {
        (false, ..) => Repr::Unseen,
        (_, true, _) => Repr::Int,
        (_, _, true) => Repr::Double,
        _ => Repr::Tagged,
    }
}

/// The representation of every web the region names, to a fixed point: where the compile
/// began (`ints`, `doubles`; dead at the header: none yet), then each writer's result.
pub(super) fn representations(region: &LoopRegion, decoded: &[Decoded], widened: &[usize], seen: [&[Register]; SEEN], webs: &Webs) -> Reprs {
    let header = region.header;
    // One loaded value that left for its int32 speculation widens every loaded value at once: the
    // next compile need not discover them one leave at a time.
    let loads_widened = loads_widened(decoded, header, widened);
    let entered = webs.entry_live(region);
    let mut reprs: Reprs = webs.orig.iter().enumerate().map(|(web, register)| entry_repr(entered.contains(&web), *register, seen)).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (k, row) in decoded.iter().enumerate() {
            let Some(dst) = row.dst else { continue };
            let result = result_repr(row, &reprs, row_widened(widened, header as usize + k, loads_widened && row.heap.is_some()), region.number_registers().contains(&webs.orig[usize::from(dst)]));
            if reprs[usize::from(dst)] < result {
                reprs[usize::from(dst)] = result;
                changed = true;
            }
        }
    }
    reprs
}

/// Whether the row takes general (tagged word) semantics: its result or an operand of a
/// relation is no int32.
pub(super) fn is_general(row: &Decoded, reprs: &[Repr]) -> bool {
    match row.dst {
        Some(dst) => reprs[usize::from(dst)] != Repr::Int,
        None => operand_repr(reprs, row.a).max(operand_repr(reprs, row.b)) >= Repr::Double,
    }
}

/// The webs of `repr`, the most named first.
pub(super) fn ranked(decoded: &[Decoded], reprs: &[Repr], repr: Repr) -> Vec<Register> {
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
