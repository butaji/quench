use super::*;
use quench_fastjit::HOLES;

/// Where the registers of the region live: pinned (int32 or double), int slots, or tagged
/// frame slots.
pub(super) struct Homes {
    pub(super) pins: Vec<Pin>,
    pub(super) floats: Vec<Pin>,
    pub(super) slots: Vec<Register>,
}

impl Homes {
    pub(super) fn of(ints: Vec<Register>, doubles: Vec<Register>) -> Self {
        let pinned = |registers: &[Register], count| registers.iter().take(count).copied().enumerate().collect();
        Self { pins: pinned(&ints, PINNED), floats: pinned(&doubles, PINNED_F), slots: ints.into_iter().skip(PINNED).collect() }
    }

    pub(super) fn place(&self, register: Register) -> Place {
        let find = |pins: &[Pin]| pins.iter().find(|(_, pinned)| *pinned == register).map(|(pin, _)| *pin as u8);
        if let Some(pin) = find(&self.pins) {
            return Place::Pin(pin);
        }
        if let Some(pin) = find(&self.floats) {
            return Place::PinF(pin);
        }
        if self.slots.contains(&register) { Place::MemInt } else { Place::Mem }
    }
}

/// The place and hole of an operand: an int32 constant is an immediate.
pub(super) fn operand(homes: &Homes, source: Option<Source>) -> (Place, u64) {
    match source {
        Some(Source::Register(register)) => (homes.place(register), u64::from(register) * SLOT),
        Some(Source::Constant(word)) if word >> HALF == INT_HIGH => (Place::Imm, word & u64::from(u32::MAX)),
        Some(Source::Constant(word)) => (Place::Const, word),
        None => (Place::Mem, 0),
    }
}

/// A row placed in the program: its stencil, hole values and branch target.
pub(super) struct Planned {
    pub(super) stencil: &'static Stencil,
    pub(super) holes: [u64; HOLES],
    pub(super) target: Option<u32>,
    /// The PC a guard of the row exits at.
    pub(super) exit_pc: Option<u32>,
}

/// The plan of a heap row, given its (place, hole) of destination, first and second operand.
fn heap_plan(decoded: &Decoded, heap: Heap, pc: u32, [(dst, dst_hole), (a, a_hole), (b, b_hole)]: [(Place, u64); 3]) -> Result<Planned, String> {
    let base = decoded.base.map_or(0, |register| u64::from(register) * SLOT);
    let holes = match heap {
        Heap::Store => [base, a_hole, b_hole],
        Heap::Field => [dst_hole, decoded.hole, 0],
        _ => [dst_hole, a_hole, b_hole],
    };
    Ok(Planned { stencil: fast_rows::stencil(HeapRow { heap, dst, a, b })?, holes, target: None, exit_pc: Some(pc) })
}

pub(super) fn plan(decoded: &Decoded, homes: &Homes, pc: u32, general: bool) -> Result<Planned, String> {
    let (a, a_hole) = operand(homes, decoded.a);
    let (b, b_hole) = operand(homes, decoded.b);
    let (dst, dst_hole) = operand(homes, decoded.dst.map(Source::Register));
    // A double row reads an int32 constant as the double it converts to: a constant word, which no code folds.
    let double = |place, hole: u64| if place == Place::Imm { (Place::Const, f64::from(hole as u32 as i32).to_bits()) } else { (place, hole) };
    let floats = [dst, a, b].iter().any(|place| matches!(place, Place::PinF(_))) && decoded.heap.is_none();
    let ((a, a_hole), (b, b_hole)) = if floats { (double(a, a_hole), double(b, b_hole)) } else { ((a, a_hole), (b, b_hole)) };
    if let Some(heap) = decoded.heap {
        return heap_plan(decoded, heap, pc, [(dst, dst_hole), (a, a_hole), (b, b_hole)]);
    }
    let polls = decoded.target.is_some_and(|target| target <= pc) && decoded.kind == Kind::Jump;
    let kind = if polls { Kind::JumpPoll } else { decoded.kind };
    let stencil = numeric::stencil(numeric::Row { kind, dst, a, b, speculate: !general })?;
    let interrupt = std::mem::offset_of!(super::super::CallContext, interrupt) as u64;
    let exits = polls || !matches!(kind, Kind::Jump | Kind::Move) || matches!(dst, Place::Pin(_) | Place::PinF(_) | Place::MemInt);
    Ok(Planned {
        stencil,
        holes: [if polls { interrupt } else { dst_hole }, a_hole, b_hole],
        target: decoded.target,
        exit_pc: exits.then_some(pc),
    })
}

/// A leave row: the stub that tags the pinned registers back and returns `word`.
pub(super) fn leave_row(code: Vec<u8>) -> Stencil {
    Stencil { raw_len: code.len(), code, relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0 }
}
