#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_bjit::numeric::supernodes::Node;
use quench_bjit::{padded_holes, HOLES};
use super::super::LaneHelper;
use super::call_plan::ChainRead;

/// Where the registers of the region live: pinned (int32 or double), int slots, or tagged
/// frame slots.
pub(crate) struct Homes {
    pub(crate) pins: Vec<Pin>,
    pub(crate) floats: Vec<Pin>,
    pub(crate) slots: Vec<Register>,
    /// The register of the activation each web belongs to, whose frame slot a web uses.
    pub(crate) orig: Vec<Register>,
}

impl Homes {
    /// The homes of the ranked int and double webs: a pin of each kind goes to the most named
    /// webs that do not clash with another web holding it, so webs with disjoint lifetimes share
    /// a pin; an int web with no pin takes an int slot.
    pub(crate) fn of(ints: &[Register], doubles: &[Register], orig: Vec<Register>, clash: &[Vec<usize>]) -> Self {
        let (pins, slots) = colored(ints, PINNED, clash);
        let (floats, _) = colored(doubles, PINNED_F, clash);
        Self { pins, floats, slots, orig }
    }

    pub(crate) fn place(&self, register: Register) -> Place {
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
pub(crate) fn operand(homes: &Homes, source: Option<Source>) -> (Place, u64) {
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
pub(crate) enum Exit {
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
pub(crate) struct Stop {
    pub(crate) pc: u32,
    pub(crate) returns: Option<u64>,
}

/// Where a row branches to: a bytecode PC (a row of the region, or a leave outside it), or a part
/// of the run of rows its own instruction makes (`Planned::pre`, itself, `post`, in order), where
/// the run's length is the row that follows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Branch {
    Pc(u32),
    Part(usize),
}

/// A row placed in the program: its stencil, hole values and branch target.
pub(crate) struct Planned {
    pub(crate) stencil: &'static Stencil,
    /// The row's library code, which a tile of this row and its neighbour is looked up by (`tile`);
    /// none for a tile itself and for the rows no supernode names (chains, leaves).
    pub(crate) node: Option<Node>,
    pub(crate) holes: [u64; HOLES],
    pub(crate) branch: Option<Branch>,
    /// The exits of the row by exit kind.
    pub(crate) exits: Vec<Exit>,
    /// Rows that run before and after this one at the same PC (a heap row reads and writes tagged
    /// frame slots only; these move its operands and result between the slot and a pinned register).
    pub(crate) pre: Vec<Planned>,
    pub(crate) post: Vec<Planned>,
    /// Set for a row that leaves the function or completes it: it jumps to its leave row.
    pub(crate) stop: Option<Stop>,
    /// The call record the row reads (`CallRecord`), which the code must not outlive.
    pub(crate) owned: Vec<Box<quench_bjit::call_record::CallRecord>>,
    /// Reached only through a fast row's miss (`steps::behind`): laid out after the hot rows.
    pub(crate) cold: bool,
    /// The holes that name a symbol the loader resolves, each with its symbol (`CallPart::symbols`).
    pub(crate) symbolic: Symbolic,
}

/// The most holes of one row that name symbols: the generic row's semantic entry and operand record.
pub(crate) const SYMBOLIC_HOLES: usize = 2;

/// The holes of a row whose run-time address is a symbol the loader resolves, each with its symbol.
pub(crate) type Symbolic = [Option<(u8, crate::aot_image::Symbol)>; SYMBOLIC_HOLES];

/// A row that names no symbol.
pub(crate) const NO_SYMBOLS: Symbolic = [None; SYMBOLIC_HOLES];

impl Planned {
    /// The run with the rows `before` laid out ahead of it at the same PC: the parts the run names by
    /// index (`Branch::Part`) are `before.len()` parts later.
    pub(crate) fn after(mut self, before: Vec<Planned>) -> Self {
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

    /// The run with `next`'s parts laid out after its own at the same PC: `next`'s own parts (`Branch::Part`) shift past them.
    pub(crate) fn then_run(&mut self, mut next: Planned) {
        let shift = self.pre.len() + 1 + self.post.len();
        let (pre, post) = (std::mem::take(&mut next.pre), std::mem::take(&mut next.post));
        self.owned.append(&mut next.owned);
        self.post.extend(pre.into_iter().chain(std::iter::once(next)).chain(post).map(|mut part| {
            if let Some(Branch::Part(index)) = &mut part.branch {
                *index += shift;
            }
            part
        }));
    }

    pub(crate) fn single(stencil: &'static Stencil, holes: [u64; HOLES], target: Option<u32>, exit_pc: Option<u32>) -> Self {
        Self { stencil, node: None, holes, branch: target.map(Branch::Pc), exits: exit_pc.map(Exit::Spilling).into_iter().collect(), pre: Vec::new(), post: Vec::new(), stop: None, owned: Vec::new(), cold: false, symbolic: NO_SYMBOLS }
    }
}

/// Whether a heap row is the variant its decode's `hole` selects: an array the entry record holds
/// is named by its record offset, else by its slot; a site read is a chain of two reads, a
/// folded read of the receiver stands on the receiver. A store's variant is its gate (`Decoded::gated`).
fn heap_variant(heap: Heap, hole: u64) -> bool {
    match heap {
        Heap::Store | Heap::Index => hole != 0,
        Heap::TypedStore(_) | Heap::TypedIndex(_) | Heap::TypedLength => true,
        Heap::SiteChain | Heap::ThisChain | Heap::ThisHeld => true,
        _ => false,
    }
}

/// The hole values of the rows that read or store through a field cache entry, read a folded
/// prototype-held field, or take the receiver: `None` for any other row. `base` is the frame offset of
/// the object register a store writes through.
fn site_holes(decoded: &Decoded, heap: Heap, base: u64, [dst_hole, a_hole, _]: [u64; 3]) -> Option<[u64; HOLES]> {
    let upper = quench_runtime::value::NarrowLayout::upper_words() as u64;
    let folded = decoded.folded.map_or(0, Folded::word);
    let object = dst_hole | a_hole << numeric::site_bits::SITE_OBJECT_AT;
    Some(padded_holes(match heap {
        Heap::IcGet | Heap::IcKnownGet => { let ic = decoded.ic.unwrap_or_default(); [object, upper, ic & u64::from(u32::MAX), ic >> u32::BITS, 0] }
        Heap::IcSet { .. } | Heap::IcKnownSet { admitted: false, .. } => { let ic = decoded.ic.unwrap_or_default(); [base, a_hole, u64::from(decoded.atom.unwrap_or_default()), ic & u64::from(u32::MAX), ic >> u32::BITS] }
        // An admitted store takes no atom test, so its shape is hole 2 (`slot_store::own_flat_known`) and the offset hole 3.
        Heap::IcKnownSet { admitted: true, .. } => { let ic = decoded.ic.unwrap_or_default(); [base, a_hole, ic & u64::from(u32::MAX), ic >> u32::BITS, 0] }
        // The flat rows read no shape: the offset follows the atom where the store tests it (`IcKnownSetFlat`), else leads.
        Heap::IcKnownGetFlat => { let ic = decoded.ic.unwrap_or_default(); [object, upper, 0, ic >> u32::BITS, 0] }
        Heap::IcKnownSetFlat { admitted: false, .. } => { let ic = decoded.ic.unwrap_or_default(); [base, a_hole, u64::from(decoded.atom.unwrap_or_default()), ic >> u32::BITS, 0] }
        Heap::IcKnownSetFlat { admitted: true, .. } => { let ic = decoded.ic.unwrap_or_default(); [base, a_hole, ic >> u32::BITS, 0, 0] }
        Heap::SiteStore => [base, a_hole, decoded.hole, decoded.admitted_tags(), 0],
        Heap::ThisStore => [0, a_hole, decoded.hole, decoded.admitted_tags(), 0],
        Heap::ThisSite | Heap::ThisChain => [dst_hole, upper, decoded.hole, 0, 0],
        Heap::Site | Heap::SiteChain => [object, upper, decoded.hole, 0, 0],
        Heap::Held => [object, folded, decoded.hole, 0, 0],
        Heap::ThisHeld => [dst_hole, folded, decoded.hole, 0, 0],
        _ => return None,
    }))
}

/// The hole values of a heap row of `decoded`, given its holes of destination, first and second operand.
pub(crate) fn heap_holes(decoded: &Decoded, homes: &Homes, heap: Heap, entry: bool, holes @ [dst_hole, a_hole, b_hole]: [u64; 3]) -> [u64; HOLES] {
    let base = if entry { decoded.hole } else { decoded.base.map_or(0, |register| u64::from(homes.orig[usize::from(register)]) * SLOT) };
    site_holes(decoded, heap, base, holes).or_else(|| global_holes(decoded, heap, holes)).or_else(|| slot_holes(heap, dst_hole, a_hole)).or_else(|| test_holes(decoded, heap, dst_hole, a_hole)).unwrap_or_else(|| padded_holes(match heap {
        Heap::Store | Heap::TypedStore(_) => [base, a_hole, b_hole],
        Heap::This => [dst_hole, 0, 0],
        Heap::Index | Heap::TypedIndex(_) if entry => [dst_hole, decoded.hole, b_hole],
        // A narrow element decodes through the upper-word table (`elements::record_element`).
        Heap::Index => return padded_holes([dst_hole, a_hole, b_hole, 0, quench_runtime::value::NarrowLayout::upper_words() as u64]),
        Heap::Field | Heap::TypedLength => [dst_hole, decoded.hole, 0],
        Heap::Instanceof => {
            let link = decoded.instanceof.map_or(0, |(_, fact)| u64::from(fact.link));
            [dst_hole | a_hole << numeric::site_bits::SITE_OBJECT_AT, b_hole | link << numeric::site_bits::HELD_LINK_AT, decoded.hole]
        }
        _ => [dst_hole, a_hole, b_hole],
    }))
}

/// The holes of a heap row that tests a value: its type, identity or liveness.
fn test_holes(decoded: &Decoded, heap: Heap, dst_hole: u64, a_hole: u64) -> Option<[u64; HOLES]> {
    Some(padded_holes(match heap {
        Heap::TypeofIs { index, .. } => [dst_hole, a_hole, u64::from(index)],
        Heap::Identity => [0, a_hole, decoded.hole],
        Heap::TypeofString => [dst_hole, a_hole, decoded.hole],
        Heap::Live => [0, decoded.hole, 0],
        _ => return None,
    }))
}

/// The holes of a heap row addressed by a frame or environment slot.
fn slot_holes(heap: Heap, dst_hole: u64, a_hole: u64) -> Option<[u64; HOLES]> {
    Some(padded_holes(match heap {
        Heap::Capture(slot) | Heap::EnvLocal(slot) => [dst_hole, u64::from(slot), u64::from(slot) * SLOT],
        Heap::LoadLocal(slot) | Heap::LoadChecked(slot) => [dst_hole, u64::from(slot), 0],
        Heap::StoreLocal(slot) | Heap::StoreChecked(slot) => [0, a_hole, u64::from(slot)],
        Heap::CaptureSet(slot) | Heap::EnvStore(slot) | Heap::EnvDefine(slot) => [a_hole, u64::from(slot), u64::from(slot) * SLOT],
        _ => return None,
    }))
}

/// The plan of a heap row, given its (place, hole) of destination, first and second operand; a row
/// that defines a second register (a name call's receiver) has its definition run after it.
fn heap_plan(decoded: &Decoded, homes: &Homes, heap: Heap, pc: u32, [(dst, dst_hole), (a, a_hole), (b, b_hole)]: [(Place, u64); 3]) -> Result<Planned, String> {
    let entry = if matches!(heap, Heap::GlobalStore | Heap::SiteStore | Heap::ThisStore) { decoded.gated(a) } else { heap_variant(heap, decoded.hole) };
    let holes = heap_holes(decoded, homes, heap, entry, [dst_hole, a_hole, b_hole]);
    // The row's stencil reads pinned operands and delivers a pinned result itself.
    let row = HeapRow { heap, dst, a, b, entry };
    let mut planned = Planned { node: Some(Node::Heap(row.code())), ..Planned::single(base_rows::stencil(row)?, holes, heap.continues_at_taken().then_some(pc + 1), Some(pc)) };
    if let Some(receiver) = decoded.also {
        planned.post.push(receiver_row(homes, receiver, pc)?);
    }
    ic_plan(decoded, homes, heap, pc, [(dst, dst_hole), (a, a_hole)], planned)
}

/// A site read's or store's run with its inline-cache row (`Decoded::ic`) ahead of it: the row's
/// hit, the shape it holds, continues at the next instruction; a miss, or a destination that
/// cannot hold the word, falls to the site row behind it, which the entry tables answer, and then
/// to the proof that the receiver is a direct object (`proofs`), which the receiver stays while its register
/// holds it. A receiver every path proved takes the row that only compares its shape (`Decoded::proven`).
fn ic_plan(decoded: &Decoded, homes: &Homes, heap: Heap, pc: u32, [(dst, dst_hole), (a, a_hole)]: [(Place, u64); 2], mut site: Planned) -> Result<Planned, String> {
    let Some(_) = decoded.ic.filter(|_| matches!(heap, Heap::Site | Heap::SiteStore)) else { return Ok(site) };
    let receiver = if heap == Heap::Site { a_hole } else { decoded.base.map_or(0, |register| u64::from(homes.orig[usize::from(register)]) * SLOT) };
    let proof = (!decoded.completes).then(|| proof_row(receiver, pc)).transpose()?;
    if !decoded.takes_ic_row() {
        // No inline-cache row: the site row runs, then the proof, which holds the receiver's shape for the rows after it.
        if let Some(proof) = proof {
            site.node = None;
            site.post.push(proof);
        }
        return Ok(site);
    }
    // A typed object read's destination is proved a direct object on the site row's path as well (`proofs::typed_object`):
    // the fast row delivers a word the slot's dependency holds to be one, but the site row delivers any word, so the
    // proof leaves unless it is a direct object. The destination is a frame slot (`repr::keep_typed_reads`).
    let delivered = (heap == Heap::Site && decoded.typed.is_some_and(|dependency| dependency.ty == quench_runtime::typed_ir::Type::DIRECT_OBJECT_FAMILY))
        .then(|| proof_row(dst_hole, pc)).transpose()?;
    let int = decoded.slot.is_some_and(|ty| ty.is_subtype(quench_runtime::typed_ir::Type::INT32));
    let ic = match (heap, decoded.proven, decoded.flat) {
        (Heap::Site, false, _) => Heap::IcGet,
        (Heap::Site, true, false) => Heap::IcKnownGet,
        (Heap::Site, true, true) => Heap::IcKnownGetFlat,
        (_, false, _) => Heap::IcSet { int },
        (_, true, false) => Heap::IcKnownSet { int, admitted: decoded.admitted },
        (_, true, true) => Heap::IcKnownSetFlat { int, admitted: decoded.admitted },
    };
    // A known get whose slot the fact store proved a Number (`Decoded::typed`, kept by `repr::keep_typed_reads`)
    // takes the row variant that delivers to its pinned double untested: its entry flag cleared. An object slot
    // has no such variant: its destination is a direct object by the dependency alone (`proofs::typed_object`).
    let untested = ic == Heap::IcKnownGet && decoded.typed.is_some_and(|dependency| dependency.ty.is_subtype(quench_runtime::typed_ir::Type::NUMBER)) && matches!(dst, Place::PinF(_));
    let row = HeapRow { heap: ic, dst, a, b: Place::Mem, entry: !untested };
    let holes = heap_holes(decoded, homes, ic, false, [dst_hole, a_hole, 0]);
    let mut fast = Planned::single(base_rows::stencil(row)?, holes, Some(pc + 1), None);
    fast.exits = vec![Exit::Ahead(1)];
    site.post.extend(proof);
    site.post.extend(delivered);
    Ok(super::steps::behind(fast, site))
}

/// The row that leaves at the next instruction unless the object register at frame offset `receiver` is
/// a direct object (`Heap::IcProof`).
fn proof_row(receiver: u64, pc: u32) -> Result<Planned, String> {
    let row = HeapRow { heap: Heap::IcProof, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    Ok(Planned::single(base_rows::stencil(row)?, padded_holes([receiver]), None, Some(pc + 1)))
}

/// Whether a row of `variant` speculates int32 operands, given whether the representations make
/// it `general`: the double variant never does, the others as the representations say. A
/// `Variant::Concat` site would call the runtime (`body_string_concat`) through the helper row
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
    // A no-overflow row the library holds no places for (a double operand's register) is the checked row, which
    // takes the same int32 operands and tests the overflow the proof did not need.
    let checked = |speculate| {
        let row = numeric::Row { kind: super::exact::checked(kind), dst, a, b, speculate };
        numeric::stencil(row).map(|stencil| (row, stencil))
    };
    row(speculates(variant, general)).or_else(|_| row(!general)).or_else(|_| checked(speculates(variant, general)))
}

/// The prototype-held entries of a method site (`Known::reads`): each load that one holder value a `folded_read` takes,
/// as a Held row over the receiver (`Heap::Held`: the receiver's exact shape and prototype link, then the value the holder
/// held at compile time, into `scratch`). The holder is listed in `Layout::holders`, so a change of its values retires the
/// body; a call's direct row compares the value by identity. Empty where the receiver is not framed, or the site records none.
pub(crate) fn chain_reads(layout: &Layout, receiver: Register, cache: u16, scratch: Register) -> Vec<ChainRead> {
    let Some(reads) = layout.reads else { return Vec::new() };
    if layout.homes.place(receiver) != Place::Mem {
        return Vec::new();
    }
    let row = HeapRow { heap: Heap::Held, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    reads(cache).into_iter().filter_map(|load| {
        let (holder, inherited) = (load.holder?, load.inherited?);
        let ([(link, value)], Some(chain)) = (inherited.values(), inherited.chains().next()) else { return None };
        if load.function_shape {
            return None;
        }
        let folded = Folded { shape: load.shape, link: *link, holder, depth: chain.depth };
        let object = u64::from(scratch) * SLOT | u64::from(receiver) * SLOT << numeric::site_bits::SITE_OBJECT_AT;
        let holes = padded_holes([object, folded.word(), value.to_bits(), 0, 0]);
        Some(ChainRead { callee: value.to_bits(), holder, stencil: base_rows::stencil(row).ok()?, holes })
    }).take(crate::compiled::tier_policy::POLYMORPHISM).collect()
}

/// The site read of a method call's callee (`call_plan::Method`): the field cache entry `cache` answers for the
/// receiver register `receiver`, written to the frame slot of `scratch`, where the call's direct row checks it. A miss
/// leaves with nothing written (`Exit::At` the call), so the interpreter runs the call instruction again.
pub(crate) fn method_read(receiver: Register, cache: u16, scratch: Register) -> Result<(&'static Stencil, [u64; HOLES]), String> {
    let row = HeapRow { heap: Heap::Site, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    let (dst_hole, a_hole) = (u64::from(scratch) * SLOT, u64::from(receiver) * SLOT);
    let object = dst_hole | a_hole << numeric::site_bits::SITE_OBJECT_AT;
    let upper = quench_runtime::value::NarrowLayout::upper_words() as u64;
    let entry = super::decode::site_read_row(receiver, cache, scratch).hole;
    Ok((base_rows::stencil(row)?, padded_holes([object, upper, entry, 0, 0])))
}

/// The row of `decoded` at `pc`, which takes general semantics where `general` and the speculation of
/// its feedback `variant`; a backward jump takes the poll row the layout's ending names.
pub(crate) fn plan(decoded: &Decoded, layout: &Layout, pc: u32, (general, variant): (bool, Variant), helpers: LaneHelpers) -> Result<Planned, String> {
    planned_row(decoded, layout, pc, (general, variant), helpers)
}

/// `plan` before an image body's misses are taken to the step helper.
fn planned_row(decoded: &Decoded, layout: &Layout, pc: u32, (general, variant): (bool, Variant), helpers: LaneHelpers) -> Result<Planned, String> {
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
        // A store whose operand's type its slot's observation does not admit is the VM's own.
        if let Some(step) = decoded.declines(a) {
            return plan_step(&Decoded::step(step, None), layout, pc, helpers);
        }
        let planned = heap_plan(decoded, homes, heap, pc, [(dst, dst_hole), (a, a_hole), (b, b_hole)])?;
        return Ok(if decoded.recheck { planned.after(vec![stores_guard(pc)?]) } else { planned });
    }
    // A frame slot the int32 proof holds reads as the int32 its low bits hold, with no tag test (`proven_int_operands`).
    // A kind whose library holds no MemInt row for that operand keeps the tested read.
    let proven_a = if a == Place::Mem && decoded.int_a { Place::MemInt } else { a };
    let proven_b = if b == Place::Mem && decoded.int_b { Place::MemInt } else { b };
    if (proven_a, proven_b) != (a, b)
        && let Ok(planned) = number_plan(decoded, layout, pc, (general, variant, helpers), [(dst, dst_hole), (proven_a, a_hole), (proven_b, b_hole)]) {
        return Ok(planned);
    }
    number_plan(decoded, layout, pc, (general, variant, helpers), [(dst, dst_hole), (a, a_hole), (b, b_hole)])
}

/// The run's stores test before a store the proofs re-admit (`Decoded::recheck`), a mid-body guard: a miss leaves at the
/// store's PC with nothing written, as a failed store guard does (`Exit::Spilling`), and a hit falls into the store.
fn stores_guard(pc: u32) -> Result<Planned, String> {
    let row = HeapRow { heap: Heap::Stores, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    Ok(Planned::single(base_rows::stencil(row)?, padded_holes([0, 0, 0]), None, Some(pc)))
}

/// The plan of a number row: a jump that polls the interrupt on a backward edge, or the chosen arithmetic row.
fn number_plan(decoded: &Decoded, layout: &Layout, pc: u32, (general, variant, helpers): (bool, Variant, LaneHelpers), [(dst, dst_hole), (a, a_hole), (b, b_hole)]: [(Place, u64); 3]) -> Result<Planned, String> {
    let polls = decoded.target.is_some_and(|target| target <= pc) && decoded.kind == Kind::Jump;
    let kind = if polls { layout.ending.poll() } else { decoded.kind };
    let ((kind, [dst, a, b]), holes_in) = folded(kind, [(dst, dst_hole), (a, a_hole), (b, b_hole)]);
    let [dst_hole, a_hole, b_hole] = holes_in;
    let (row, stencil) = chosen_row(kind, [dst, a, b], variant, general)?;
    let interrupt = std::mem::offset_of!(super::super::CallContext, head.interrupt) as u64;
    let exits = polls || !matches!(kind, Kind::Jump | Kind::Move) || holds_number(dst);
    // A poll row also names where the run's backward-edge counter lives (`jump_row`).
    let counter = [std::mem::offset_of!(super::super::CallContext, head.adapter_payload) as u64, std::mem::offset_of!(super::super::JsStepDispatch, head.loop_entry) as u64];
    // A poll reads the process-wide attention byte first (hole 3), so its common case is one byte load.
    let attention = quench_runtime::execution_interrupt::attention_address();
    // A case slot's maximum rides in hole 3 (`Decoded::hole`); no other numeric row names a fourth hole.
    let bound = if decoded.kind == Kind::IntCase { decoded.hole } else { 0 };
    let holes = padded_holes(if polls { [interrupt, counter[0], counter[1], attention] } else { [dst_hole, a_hole, b_hole, bound] });
    let planned = Planned { node: Some(Node::Number(numeric::library::canonical(row.code()))), ..Planned::single(stencil, holes, decoded.target, exits.then_some(pc)) };
    if decoded.generic && variant == Variant::Concat && decoded.kind == Kind::Add {
        return concat_run(decoded, layout, pc, planned, helpers);
    }
    Ok(planned)
}

/// The run of a string `+` site (`Variant::Concat`) at `pc`: the numeric row `fast` is the fast arm, which takes Number
/// operands and writes nothing otherwise, and its miss enters the generic step helper laid out behind it, which the
/// interpreter's `+` makes (the concatenation included). The region keeps running where a leave would end it each
/// iteration; the helper is off the loop's steady path (`steps::behind`).
fn concat_run(decoded: &Decoded, layout: &Layout, pc: u32, fast: Planned, helpers: LaneHelpers) -> Result<Planned, String> {
    // Two register operands: the call-out of the operation (`CallOut::Binary`, `Vm::binary`, the concatenation) is the miss.
    // A constant operand has no register call-out, so the generic step makes the operation.
    let registers = matches!((decoded.a, decoded.b), (Some(Source::Register(_)), Some(Source::Register(_))));
    let helper = if registers { LaneHelper::VmStep } else { LaneHelper::Step };
    let slow = steps::plan_helper(layout, pc, decoded, (helper, helpers))?;
    let behind_parts = 1 + slow.pre.len() + slow.post.len();
    let run = Planned { node: None, exits: vec![Exit::Ahead(1)], branch: decoded.completes.then_some(Branch::Part(1 + behind_parts)), ..fast };
    Ok(steps::behind(run, slow))
}

/// A number row partially evaluated on its immediate operand, the way the C compiler treats the same
/// expression: a commutative operation takes its immediate second (the form a row encodes into its
/// instruction), and an operation whose immediate is its neutral element (`x | 0`, `x + 0`) over an
/// int32 operand into an int32 home or a frame slot (which takes the int word) is the move of that operand. Only where
/// the library has the folded row: otherwise the row as decoded.
fn folded(kind: Kind, [dst, a, b]: [(Place, u64); 3]) -> ((Kind, [Place; 3]), [u64; 3]) {
    let int32 = |place: Place| matches!(place, Place::Pin(_) | Place::MemInt);
    let (a, b) = if kind.commutes() && a.0 == Place::Imm && b.0 != Place::Imm { (b, a) } else { (a, b) };
    let neutral = b.0 == Place::Imm && kind.neutral() == Some(b.1 as u32 as i32);
    let moved = (Kind::Move, [dst.0, a.0, Place::Mem]);
    let home = int32(dst.0) || dst.0 == Place::Mem;
    if neutral && int32(a.0) && home && numeric::stencil(numeric::Row { kind: Kind::Move, dst: dst.0, a: a.0, b: Place::Mem, speculate: true }).is_ok() {
        return (moved, [dst.1, a.1, 0]);
    }
    ((kind, [dst.0, a.0, b.0]), [dst.1, a.1, b.1])
}

/// Whether `place` holds a number the row reads or writes in a register or as an int32 in memory, not a tagged word.
pub(crate) fn holds_number(place: Place) -> bool {
    matches!(place, Place::Pin(_) | Place::PinF(_) | Place::MemInt)
}

/// A leave row: the stub that tags the pinned registers back and returns `word`.
pub(crate) fn leave_row(code: Vec<u8>) -> Stencil {
    Stencil { raw_len: code.len(), hot_len: code.len(), code, relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0 }
}
