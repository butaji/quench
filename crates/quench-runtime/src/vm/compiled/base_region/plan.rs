use super::*;
use quench_bjit::numeric::supernodes::Node;
use quench_bjit::{padded_holes, HOLES};

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
        Some(Source::Constant(word) | Source::Value(word)) => (Place::Const, word),
        None => (Place::Mem, 0),
    }
}

/// Where an exit hole of a row, by exit kind, is patched to. Every exit lies in the cold area
/// after the hot rows, so a row falls through to its successor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Exit {
    /// A stub that names `pc` and enters the routine writing the live pinned registers back, then
    /// leaves at `pc`: a guard that failed before the row's effects.
    Spilling(u32),
    /// Leave at this pc; nothing is written back (the chain's spills already ran).
    At(u32),
    /// `LEAVE_STATUS`: the (status, word) the row left with.
    Status,
    /// The part this many parts after the row's own in its run: a fast arm's miss into the helper
    /// step laid out behind it, where nothing has been written back or changed.
    Ahead(usize),
}

/// A row that leaves the function at `pc`, or completes the activation with the register slot
/// at byte offset `returns`.
#[derive(Clone, Copy)]
pub(super) struct Stop {
    pub(super) pc: u32,
    pub(super) returns: Option<u64>,
}

/// Where a row branches to: a bytecode PC (a row of the region, or a leave outside it), or a part
/// of the run of rows its own instruction makes (`Planned::pre`, itself, `post`, in order), where
/// the run's length is the row that follows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Branch {
    Pc(u32),
    Part(usize),
}

/// A row placed in the program: its stencil, hole values and branch target.
pub(super) struct Planned {
    pub(super) stencil: &'static Stencil,
    /// The row's library code, which a tile of this row and its neighbour is looked up by (`tile`);
    /// none for a tile itself and for the rows no supernode names (chains, leaves).
    pub(super) node: Option<Node>,
    pub(super) holes: [u64; HOLES],
    pub(super) branch: Option<Branch>,
    /// The exits of the row by exit kind.
    pub(super) exits: Vec<Exit>,
    /// Rows that run before and after this one at the same PC (a heap row reads and writes tagged
    /// frame slots only; these move its operands and result between the slot and a pinned register).
    pub(super) pre: Vec<Planned>,
    pub(super) post: Vec<Planned>,
    /// Set for a row that leaves the function or completes it: it jumps to its leave row.
    pub(super) stop: Option<Stop>,
    /// The call record the row reads (`CallRecord`), which the code must not outlive.
    pub(super) owned: Option<Box<quench_bjit::call_record::CallRecord>>,
}

impl Planned {
    /// The run with the rows `before` laid out ahead of it at the same PC: the parts the run names by
    /// index (`Branch::Part`) are `before.len()` parts later.
    pub(super) fn after(mut self, before: Vec<Planned>) -> Self {
        let shift = before.len();
        let later = |part: &mut Planned| {
            if let Some(Branch::Part(index)) = &mut part.branch {
                *index += shift;
            }
        };
        self.pre.iter_mut().for_each(later);
        later(&mut self);
        self.post.iter_mut().for_each(later);
        self.pre.splice(0..0, before);
        self
    }

    pub(super) fn single(stencil: &'static Stencil, holes: [u64; HOLES], target: Option<u32>, exit_pc: Option<u32>) -> Self {
        Self { stencil, node: None, holes, branch: target.map(Branch::Pc), exits: exit_pc.map(Exit::Spilling).into_iter().collect(), pre: Vec::new(), post: Vec::new(), stop: None, owned: None }
    }
}

/// Whether a heap row is the variant its decode's `hole` selects: an array the entry record holds
/// is named by its record offset, else by its slot; a site read is a chain of two reads, a
/// folded read of the receiver stands on the receiver, and a global store of a value held in a frame
/// slot (any value, not a Number) takes the store gate.
fn heap_variant(heap: Heap, hole: u64, operand: Place) -> bool {
    match heap {
        Heap::Store | Heap::Index => hole != 0,
        Heap::SiteChain | Heap::ThisChain | Heap::ThisHeld => true,
        Heap::GlobalStore => operand == Place::Mem,
        _ => false,
    }
}

/// The hole values of the rows that read or store through a field cache entry, read a folded
/// prototype-held field, or take the receiver: `None` for any other row. `base` is the frame offset of
/// the object register a store writes through.
fn site_holes(decoded: &Decoded, heap: Heap, base: u64, [dst_hole, a_hole, _]: [u64; 3]) -> Option<[u64; HOLES]> {
    let upper = crate::value::NarrowLayout::upper_words() as u64;
    let folded = decoded.folded.map_or(0, Folded::word);
    let object = dst_hole | a_hole << numeric::library::SITE_OBJECT_AT;
    Some(padded_holes(match heap {
        Heap::SiteStore => [base, a_hole, decoded.hole],
        Heap::ThisStore => [0, a_hole, decoded.hole],
        Heap::ThisSite | Heap::ThisChain => [dst_hole, upper, decoded.hole],
        Heap::Site | Heap::SiteChain => [object, upper, decoded.hole],
        Heap::Held => [object, folded, decoded.hole],
        Heap::ThisHeld => [dst_hole, folded, decoded.hole],
        _ => return None,
    }))
}

/// The hole values of a heap row of `decoded`, given its holes of destination, first and second operand.
pub(super) fn heap_holes(decoded: &Decoded, homes: &Homes, heap: Heap, entry: bool, holes @ [dst_hole, a_hole, b_hole]: [u64; 3]) -> [u64; HOLES] {
    let base = if entry { decoded.hole } else { decoded.base.map_or(0, |register| u64::from(homes.orig[usize::from(register)]) * SLOT) };
    site_holes(decoded, heap, base, holes).or_else(|| global_holes(decoded, heap, holes)).unwrap_or_else(|| padded_holes(match heap {
        Heap::Store => [base, a_hole, b_hole],
        Heap::This => [dst_hole, 0, 0],
        Heap::Index if entry => [dst_hole, decoded.hole, b_hole],
        Heap::Field => [dst_hole, decoded.hole, 0],
        Heap::Capture(slot) | Heap::EnvLocal(slot) => [dst_hole, u64::from(slot), u64::from(slot) * SLOT],
        Heap::LoadLocal(slot) | Heap::LoadChecked(slot) => [dst_hole, u64::from(slot), 0],
        Heap::StoreLocal(slot) | Heap::StoreChecked(slot) => [0, a_hole, u64::from(slot)],
        Heap::TypeofIs { index, .. } => [dst_hole, a_hole, u64::from(index)],
        Heap::Identity => [0, a_hole, decoded.hole],
        Heap::TypeofString => [dst_hole, a_hole, decoded.hole],
        Heap::Instanceof => {
            let link = decoded.instanceof.map_or(0, |(_, fact)| u64::from(fact.link));
            [dst_hole | a_hole << numeric::library::SITE_OBJECT_AT, b_hole | link << numeric::library::HELD_LINK_AT, decoded.hole]
        }
        Heap::CaptureSet(slot) | Heap::EnvStore(slot) => [a_hole, u64::from(slot), u64::from(slot) * SLOT],
        Heap::Live => [0, decoded.hole, 0],
        _ => [dst_hole, a_hole, b_hole],
    }))
}

/// The plan of a heap row, given its (place, hole) of destination, first and second operand; a row
/// that defines a second register (a name call's receiver) has its definition run after it.
fn heap_plan(decoded: &Decoded, homes: &Homes, heap: Heap, pc: u32, [(dst, dst_hole), (a, a_hole), (b, b_hole)]: [(Place, u64); 3]) -> Result<Planned, String> {
    let entry = heap_variant(heap, decoded.hole, a);
    let holes = heap_holes(decoded, homes, heap, entry, [dst_hole, a_hole, b_hole]);
    // The row's stencil reads pinned operands and delivers a pinned result itself.
    let row = HeapRow { heap, dst, a, b, entry };
    let mut planned = Planned { node: Some(Node::Heap(row.code())), ..Planned::single(base_rows::stencil(row)?, holes, heap.continues_at_taken().then_some(pc + 1), Some(pc)) };
    if let Some(receiver) = decoded.also {
        planned.post.push(receiver_row(homes, receiver, pc)?);
    }
    Ok(planned)
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

/// The row of `decoded` at `pc`, which takes general semantics where `general` and the speculation of
/// its feedback `variant`; a backward jump takes the poll row the layout's ending names.
pub(super) fn plan(decoded: &Decoded, layout: &Layout, pc: u32, (general, variant): (bool, Variant), helpers: LaneHelpers) -> Result<Planned, String> {
    if decoded.step.is_some() {
        return plan_step(decoded, layout, pc, helpers);
    }
    let homes = &layout.homes;
    let (a, a_hole) = operand(homes, decoded.a);
    let (b, b_hole) = operand(homes, decoded.b);
    let (dst, dst_hole) = operand(homes, decoded.dst.map(Source::Register));
    // A double row reads an int32 constant as the double it converts to: a constant word, which no code folds.
    let double = |place, hole: u64| if place == Place::Imm { (Place::Const, f64::from(hole as u32 as i32).to_bits()) } else { (place, hole) };
    let floats = [dst, a, b].iter().any(|place| matches!(place, Place::PinF(_))) && decoded.heap.is_none();
    let ((a, a_hole), (b, b_hole)) = if floats { (double(a, a_hole), double(b, b_hole)) } else { ((a, a_hole), (b, b_hole)) };
    if let Some(heap) = decoded.heap {
        // A global store whose operand's type the slot's observation does not admit is the VM's own.
        if declines(decoded, a) {
            return plan_step(&Decoded::step(Step::CallOut, None), layout, pc, helpers);
        }
        return heap_plan(decoded, homes, heap, pc, [(dst, dst_hole), (a, a_hole), (b, b_hole)]);
    }
    let polls = decoded.target.is_some_and(|target| target <= pc) && decoded.kind == Kind::Jump;
    let kind = if polls { layout.ending.poll() } else { decoded.kind };
    let (row, stencil) = chosen_row(kind, [dst, a, b], variant, general)?;
    let interrupt = std::mem::offset_of!(super::super::CallContext, interrupt) as u64;
    let exits = polls || !matches!(kind, Kind::Jump | Kind::Move) || matches!(dst, Place::Pin(_) | Place::PinF(_) | Place::MemInt);
    // A poll row also names where the run's backward-edge counter lives (`jump_row`).
    let counter = [std::mem::offset_of!(super::super::CallContext, adapter_payload) as u64, std::mem::offset_of!(super::super::JsStepDispatch, loop_entry) as u64];
    let holes = padded_holes(if polls { [interrupt, counter[0], counter[1]] } else { [dst_hole, a_hole, b_hole] });
    Ok(Planned { node: Some(Node::Number(numeric::library::canonical(row.code()))), ..Planned::single(stencil, holes, decoded.target, exits.then_some(pc)) })
}

/// A leave row: the stub that tags the pinned registers back and returns `word`.
pub(super) fn leave_row(code: Vec<u8>) -> Stencil {
    Stencil { raw_len: code.len(), hot_len: code.len(), code, relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0 }
}
