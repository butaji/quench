use super::*;
use quench_bjit::numeric::supernodes::Node;
use quench_bjit::HOLES;

/// Where the registers of the region live: pinned (int32 or double), int slots, or tagged
/// frame slots.
pub(super) struct Homes {
    pub(super) pins: Vec<Pin>,
    pub(super) floats: Vec<Pin>,
    pub(super) slots: Vec<Register>,
    /// The register of the activation each web belongs to, whose frame slot a web uses.
    pub(super) orig: Vec<Register>,
}

impl Homes {
    /// The homes of the ranked int and double webs: a pin of each kind goes to the most named
    /// webs that do not clash with another web holding it, so webs with disjoint lifetimes share
    /// a pin; an int web with no pin takes an int slot.
    pub(super) fn of(ints: &[Register], doubles: &[Register], orig: Vec<Register>, clash: &[Vec<usize>]) -> Self {
        let (pins, slots) = colored(ints, PINNED, clash);
        let (floats, _) = colored(doubles, PINNED_F, clash);
        Self { pins, floats, slots, orig }
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

/// Gives each web of `ranked`, most named first, the lowest of `limit` pins no clashing web
/// holds; the webs left without one are returned.
fn colored(ranked: &[Register], limit: usize, clash: &[Vec<usize>]) -> (Vec<Pin>, Vec<Register>) {
    let (mut taken, mut left): (Vec<Pin>, Vec<Register>) = (Vec::new(), Vec::new());
    for &web in ranked {
        let used: Vec<usize> = taken.iter().filter(|(_, other)| clash[usize::from(web)].contains(&usize::from(*other))).map(|(pin, _)| *pin).collect();
        match (0..limit).find(|pin| !used.contains(pin)) {
            Some(pin) => taken.push((pin, web)),
            None => left.push(web),
        }
    }
    (taken, left)
}

/// The place and hole of an operand: an int32 constant is an immediate.
pub(super) fn operand(homes: &Homes, source: Option<Source>) -> (Place, u64) {
    match source {
        Some(Source::Register(register)) => (homes.place(register), u64::from(homes.orig[usize::from(register)]) * SLOT),
        Some(Source::Constant(word)) if word >> HALF == INT_HIGH => (Place::Imm, word & u64::from(u32::MAX)),
        Some(Source::Constant(word)) => (Place::Const, word),
        None => (Place::Mem, 0),
    }
}

/// A row placed in the program: its stencil, hole values and branch target.
pub(super) struct Planned {
    pub(super) stencil: &'static Stencil,
    /// The row's library code, which a tile of this row and its neighbour is looked up by; none
    /// for a tile itself.
    pub(super) node: Option<Node>,
    pub(super) holes: [u64; HOLES],
    pub(super) target: Option<u32>,
    /// The PC a guard of the row exits at.
    pub(super) exit_pc: Option<u32>,
    /// Rows that run before and after this one at the same PC (a heap row reads and writes tagged
    /// frame slots only; these move its operands and result between the slot and a pinned register).
    pub(super) pre: Vec<Planned>,
    pub(super) post: Vec<Planned>,
}

impl Planned {
    fn single(stencil: &'static Stencil, node: Node, holes: [u64; HOLES], target: Option<u32>, exit_pc: Option<u32>) -> Self {
        Self { stencil, node: Some(node), holes, target, exit_pc, pre: Vec::new(), post: Vec::new() }
    }
}

/// The plan of a heap row, given its (place, hole) of destination, first and second operand.
fn heap_plan(decoded: &Decoded, homes: &Homes, heap: Heap, pc: u32, [(dst, dst_hole), (a, a_hole), (b, b_hole)]: [(Place, u64); 3]) -> Result<Planned, String> {
    // An array the entry record holds is named by its record offset (`decoded.hole`), else by its slot.
    let entry = decoded.hole != 0 && matches!(heap, Heap::Store | Heap::Index);
    let base = if entry { decoded.hole } else { decoded.base.map_or(0, |register| u64::from(homes.orig[usize::from(register)]) * SLOT) };
    let holes = match heap {
        Heap::Store => [base, a_hole, b_hole],
        Heap::Index if entry => [dst_hole, decoded.hole, b_hole],
        Heap::Field => [dst_hole, decoded.hole, 0],
        Heap::Capture(slot) => [dst_hole, u64::from(slot), u64::from(slot) * SLOT],
        _ => [dst_hole, a_hole, b_hole],
    };
    // The row's stencil reads pinned operands and delivers a pinned result itself.
    let row = HeapRow { heap, dst, a, b, entry };
    Ok(Planned::single(base_rows::stencil(row)?, Node::Heap(row.code()), holes, None, Some(pc)))
}

/// Whether a row of `variant` speculates int32 operands, given whether the representations make
/// it `general`: the double variant never does, the others as the representations say. A
/// `Variant::Concat` site would call the runtime (`lane_body_string_concat`) through the helper row
/// (`call_plan`), whose payload only a region that crosses with the activation's dispatch has;
/// until then it takes the generic row, whose tag guard exits at the PC for a string operand and
/// writes nothing.
fn speculates(variant: Variant, general: bool) -> bool {
    variant != Variant::Double && !general
}

/// The row of the variant's speculation or, where the library holds none, of the speculation the
/// representations give, with its stencil.
fn chosen_row(kind: Kind, [dst, a, b]: [Place; 3], variant: Variant, general: bool) -> Result<(numeric::Row, &'static Stencil), String> {
    let row = |speculate| {
        let row = numeric::Row { kind, dst, a, b, speculate };
        numeric::stencil(row).map(|stencil| (row, stencil))
    };
    row(speculates(variant, general)).or_else(|_| row(!general))
}

pub(super) fn plan(decoded: &Decoded, homes: &Homes, pc: u32, general: bool, variant: Variant) -> Result<Planned, String> {
    let (a, a_hole) = operand(homes, decoded.a);
    let (b, b_hole) = operand(homes, decoded.b);
    let (dst, dst_hole) = operand(homes, decoded.dst.map(Source::Register));
    // A double row reads an int32 constant as the double it converts to: a constant word, which no code folds.
    let double = |place, hole: u64| if place == Place::Imm { (Place::Const, f64::from(hole as u32 as i32).to_bits()) } else { (place, hole) };
    let floats = [dst, a, b].iter().any(|place| matches!(place, Place::PinF(_))) && decoded.heap.is_none();
    let ((a, a_hole), (b, b_hole)) = if floats { (double(a, a_hole), double(b, b_hole)) } else { ((a, a_hole), (b, b_hole)) };
    if let Some(heap) = decoded.heap {
        return heap_plan(decoded, homes, heap, pc, [(dst, dst_hole), (a, a_hole), (b, b_hole)]);
    }
    let polls = decoded.target.is_some_and(|target| target <= pc) && decoded.kind == Kind::Jump;
    let kind = if polls { Kind::JumpPoll } else { decoded.kind };
    let (row, stencil) = chosen_row(kind, [dst, a, b], variant, general)?;
    let interrupt = std::mem::offset_of!(super::super::CallContext, interrupt) as u64;
    let exits = polls || !matches!(kind, Kind::Jump | Kind::Move) || matches!(dst, Place::Pin(_) | Place::PinF(_) | Place::MemInt);
    // A poll row also names where the run's backward-edge counter lives (`jump_row`).
    let counter = [std::mem::offset_of!(super::super::CallContext, adapter_payload) as u64, std::mem::offset_of!(super::super::JsStepDispatch, loop_entry) as u64];
    let holes = if polls { [interrupt, counter[0], counter[1]] } else { [dst_hole, a_hole, b_hole] };
    Ok(Planned::single(stencil, Node::Number(numeric::library::canonical(row.code())), holes, decoded.target, exits.then_some(pc)))
}

/// A leave row: the stub that tags the pinned registers back and returns `word`.
pub(super) fn leave_row(code: Vec<u8>) -> Stencil {
    Stencil { raw_len: code.len(), code, relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0 }
}
