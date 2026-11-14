//! A method call whose callee its site's method cache remembered (`callees::Callees`, by PC) as the one function it calls,
//! planned as the plain call's chain (`call_plan`) with the method loaded into a scratch frame slot ahead of its direct
//! row: the site's cache read of the receiver (`decode::cached_read`: the value its prototype holds folded under the
//! holder's dependency, else the cache entry's slot), which the direct row then proves is the recorded callee (the slot is
//! its callee register). A `this.m()` call first copies the activation's `this` to a second scratch slot, the receiver
//! the direct row reads. The scratch slots are the registers past the function's own (`function::scratch`), and the
//! call's arguments must be a window of consecutive registers, as the direct row copies them; any other method call is
//! the step helper's (`CallForm::Stepped`).
//!
//! A load that misses, a callee other than the recorded one, and a cell the call is not admitted to take the step helper
//! behind the direct row, which makes the whole call.

use super::*;
use super::call_plan::{choose, CallForm, CallPart, CallShape, Choice, DirectFacts};
use quench_bjit::{padded_holes, HOLES};
use crate::bytecode::{MethodSite, Op};

/// The site of the method call `instruction` and the first register of the window its arguments are, where the rows make the call: a
/// `CallMethod`, or a `CallThisMethod` of the activation's own `this` (no field path), with no result flag, over arguments that are
/// consecutive registers (`direct_part` copies a window).
fn direct_site(program: &ResidualProgram, instruction: WideInstruction) -> Option<(&MethodSite, Register)> {
    let this_method = match instruction.op() {
        Op::CallMethod => false,
        Op::CallThisMethod => true,
        _ => return None,
    };
    let site = program.method_sites.get(instruction.method_site_index())?;
    let admitted = !this_method || site.receiver_path.is_none();
    let start = site.argument_start as usize;
    let list = program.method_arguments.get(start..start + usize::from(site.argument_count))?;
    let consecutive = list.windows(2).all(|pair| pair[0].checked_add(1) == Some(pair[1]));
    (instruction.result_flags() == 0 && admitted && consecutive).then(|| (site, list.first().copied().unwrap_or_default()))
}

/// Whether `method_call` makes the call `instruction` as rows where its site recorded a callee (`Vm::add_method_callees`).
pub(in crate::vm::compiled) fn makes_rows(program: &ResidualProgram, instruction: WideInstruction) -> bool {
    direct_site(program, instruction).is_some()
}

/// The method call `instruction` at `pc` as a row, the registers from `scratch` up its scratch, where its site recorded the one
/// callee `direct` names and the call is one the rows make (`direct_site`); `None` leaves the call as it is. The row is the call
/// of `CallForm::Method`, with the site's cache read (`Decoded::load`) its chain loads the callee by.
pub(super) fn method_call(known: Known, pc: u32, instruction: WideInstruction, scratch: Register, direct: Option<&DirectFacts>) -> Option<Decoded> {
    let (site, window) = direct_site(known.program, instruction)?;
    let (this, object) = match instruction.op() {
        Op::CallMethod => (instruction.register_b(), Some(instruction.register_b())),
        _ => (scratch.checked_add(1)?, None),
    };
    let read = cached_read(known, (object, site.cache, None), scratch);
    let shape = CallShape { call_pc: pc, callee: scratch, this, window, argc: site.argument_count, result: instruction.result_register(), returning: false, form: CallForm::Method };
    (choose(&shape, direct) == Choice::Direct).then_some(())?;
    Some(Decoded { a: read.a, hole: read.hole, folded: read.folded, load: read.heap, ..Decoded::step(Step::Call(shape), Some(shape.result)) })
}

/// What the chain of the method call `decoded` loads before its direct row (`MethodLoad`): the cache read into the callee slot, and the
/// activation's `this` into the receiver slot where the call is a `this.m()`.
pub(super) fn method_load(decoded: &Decoded, homes: &Homes, shape: &CallShape) -> Option<MethodLoad> {
    let heap = decoded.load.filter(|_| shape.form == CallForm::Method)?;
    let slot = |register: Register| u64::from(register) * SLOT;
    let (_, object) = operand(homes, decoded.a);
    let row = HeapRow { heap, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: heap_variant(heap, decoded.hole, Place::Mem) };
    let holes = heap_holes(decoded, homes, heap, row.entry, [slot(shape.callee), object, 0]);
    Some(MethodLoad { this: decoded.a.is_none().then(|| slot(shape.this)), read: (row, holes) })
}

/// What a method call loads ahead of its direct row: the slot the activation's `this` is copied to where the call is a `this.m()`, then
/// the row that reads the method into the callee's slot, with its holes.
pub(super) struct MethodLoad {
    this: Option<u64>,
    read: (HeapRow, [u64; HOLES]),
}

/// The rows that load a method call's callee: the copy of the activation's `this` where the call is a `this.m()`, then the read of the method.
fn load_parts(load: &MethodLoad) -> Result<Vec<CallPart>, String> {
    let part = |stencil, holes| CallPart { stencil, holes, taken: None, exits: Vec::new() };
    let copy = HeapRow { heap: Heap::This, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    let copied = load.this.map(|slot| base_rows::stencil(copy).map(|stencil| part(stencil, padded_holes([slot])))).transpose()?;
    let read = part(base_rows::stencil(load.read.0)?, load.read.1);
    Ok(copied.into_iter().chain(std::iter::once(read)).collect())
}

/// `parts` with the loads of a method call pushed, which miss into the helper row laid out behind the direct row that follows them.
pub(super) fn push_loads(parts: &mut Vec<CallPart>, load: &MethodLoad) -> Result<(), String> {
    let first = parts.len();
    parts.extend(load_parts(load)?);
    let helper_at = parts.len() + 1;
    for (index, part) in parts.iter_mut().enumerate().skip(first) {
        part.exits = vec![Exit::Ahead(helper_at - index)];
    }
    Ok(())
}
