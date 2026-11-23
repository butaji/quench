//! The callees a whole function's body calls by their entries, and which of its call sites the
//! stitcher inlines (`call_plan::inline`) instead of chaining a call.
//!
//! A call whose callee is small and monomorphic is planned as the callee's rows over registers of
//! the caller and its window (`InlineSite`), which the stitch splices in place of the call
//! (`stitch::stitch`) behind a row that proves the callee register holds the function the site
//! recorded. The callee's body is not named by the code, but its identity is: the site's
//! descriptor stays among the method's direct callees, so a collection that frees the callee
//! retires the body (`ProgramStore::retire_dead_native_calls`), and the identity a word names is
//! never a reused one.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::call_plan::inline::{Callee, Context, InlineSite};
use super::call_plan::{CallForm, CallShape, DirectFacts};
use super::call_plan::builtin::{self, BuiltinCall};
use super::super::{DirectCallee, DirectCandidates};
use quench_runtime::bytecode::Function;
use quench_runtime::compile::liveness::RegisterMasks;
use quench_runtime::heap::GlobalFact;
use std::rc::Rc;

/// A function a site may inline: its instructions, registers, the register each parameter is
/// passed to and the registers live before each instruction.
pub(crate) struct InlineBody {
    code: Vec<WideInstruction>,
    registers: u16,
    homes: Vec<(u16, Register)>,
    live: Rc<RegisterMasks>,
    /// The Number constants the callee's instructions name, by pool index: its rows fold them as the
    /// caller's rows fold the caller's (the caller's region holds only its own).
    numbers: std::collections::BTreeMap<usize, f64>,
}

impl InlineBody {
    /// The body of `function`, whose liveness is `live` and whose constant pool is `constants`; none
    /// where liveness does not cover it.
    pub(crate) fn of(function: &Function, live: Option<&Rc<RegisterMasks>>, constants: &[quench_runtime::value::Value]) -> Option<Self> {
        let home = |argument: u16| function.parameter_registers.map(|base| base + argument).or_else(|| function.promoted_register(argument));
        let homes = (0..function.params).filter_map(|argument| home(argument).map(|home| (argument, home))).collect();
        let code: Vec<WideInstruction> = function.instructions().collect();
        let numbers = code.iter().flat_map(|instruction| named_constants(*instruction))
            .filter_map(|index| Some((index, constants.get(index)?.as_number()?)))
            .collect();
        Some(Self { code, registers: function.registers, homes, live: live?.clone(), numbers })
    }
}

/// The constant-pool indexes `instruction` may name: a loaded constant and its constant operands.
fn named_constants(instruction: WideInstruction) -> impl Iterator<Item = usize> {
    use quench_runtime::bytecode::OperandKind;
    let loaded = quench_runtime::staged::Operation::<(), ()>::decode_constant_load(instruction).map(|(_, index)| index);
    let operand = |operand: quench_runtime::bytecode::Operand| (operand.kind() == Some(OperandKind::Constant)).then(|| usize::from(operand.payload()));
    let binary = super::super::meaning_compiled::is_binary_arithmetic(instruction)
        || matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. }));
    let operands = binary.then(|| [operand(instruction.operand_b()), operand(instruction.operand_c())]).into_iter().flatten().flatten();
    loaded.into_iter().chain(operands)
}

/// The callees a body calls by their entries: the descriptors its code names, which its method
/// owns for as long as the code runs, by PC which of them a call site calls, and the function
/// each runs where a site may inline it. The default has no call site: every call stays a call.
#[derive(Default)]
pub(crate) struct Callees {
    pub(crate) owned: Box<[DirectCallee]>,
    /// By PC, the owned callees its call site recorded, latest first (`DirectCandidates`): the first is
    /// the one a stitch may inline, every one a direct row of the call's chain calls.
    at: Vec<Option<std::ops::Range<usize>>>,
    bodies: Vec<Option<InlineBody>>,
    /// By PC, what holds the callee its call site recorded (`Vm::stable_callees`).
    stable: Vec<Option<Held>>,
    /// By PC, the native function the call site's callee register holds, where a field read defines it
    /// (`Vm::native_callees`), with the holder whose values the code depends on.
    natives: Vec<Option<(quench_runtime::value::Value, quench_runtime::heap::Native, u32)>>,
    /// By PC, the callee the closed world proves the call site calls (`AotFacts::call_target`), as an
    /// image's body calls it: through the site's image record (`Symbol::CallRecord`), whose run-time
    /// words the loader binds (`aot_image::bound::BoundCalls::bind`). Empty for a body of this process.
    proven: Vec<Option<DirectFacts>>,
}

/// What holds a call site's callee register, so that the register need not be checked: a Stable global cell
/// (its change retires the body), or a captured declaration whose identity the body's entry checks once
/// (`Captured`, `base_region::link::entry_rows`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Held {
    Global(GlobalFact),
    /// The depth-0 capture at `slot` holds `callee` (its word) at every entry of the body.
    Captured { slot: u16, callee: u64 },
}

impl Held {
    /// The Stable global fact, if it is one.
    pub(crate) fn global(held: Held) -> Option<GlobalFact> {
        match held {
            Held::Global(fact) => Some(fact),
            Held::Captured { .. } => None,
        }
    }
}

/// How a callee binds its `this`: what selects a direct row's receiver form (`ThisRow`), from its code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ThisForm {
    /// A sloppy function binds the global object for an `undefined` or `null` receiver.
    pub(crate) sloppy: bool,
    /// An arrow binds none (`DirectCallee::lexical`).
    pub(crate) lexical: bool,
}

impl ThisForm {
    /// The form of `callee`, which an image's row is planned with and its binding is checked against
    /// (`Vm::bind_ahead_calls`): the one derivation of both.
    pub(crate) fn of(callee: &Function) -> Self {
        Self { sloppy: !callee.strict && !callee.is_arrow, lexical: callee.is_arrow }
    }
}

/// What an image's body knows of `callee` (function `function`), the function the closed world proves a site calls: its
/// window and how it binds `this`, from its code alone. Its run-time words (identity, cell, validity, call, caller base,
/// captures, the global) are the site record's, which name no callee (`aot_image::unbound_callee`) until the loader
/// binds them, so the row refuses every call to the helper until then. The cell's facts are the ones a plain
/// activation publishes (`CellFacts::of_function`); the binding admits only a cell that holds them.
fn proven_facts(function: u32, callee: &Function) -> DirectFacts {
    let this = ThisForm::of(callee);
    DirectFacts {
        callee: crate::aot_image::unbound_callee(), cell: 0, validity: 0,
        facts: quench_runtime::staged::call::CellFacts::of_function(callee.registers, callee.params, u16::from(quench_runtime::staged::call::FILL_ALL), true),
        call: 0, caller_base: 0, function, captures: 0, env: 0, capture: 0, global_this: this.sloppy.then_some(0), lexical: this.lexical,
        registers: callee.registers, construction: false, held: false, descriptor: 0,
    }
}

/// Whether the call at `instruction` is a plain call a body's call chain makes: a call whose result
/// is its register's, or one that returns its result (`return f(x)`), which the row after it
/// completes with. The other forms record no callee (`call_plan::CallForm`).
pub(crate) fn chains(instruction: WideInstruction) -> bool {
    let plain = instruction.split_return().0;
    CallForm::of(plain) == Some(CallForm::Plain) || is_method_call(plain)
}

/// Whether `instruction` is a method call `o.m(args)`, whose callee its site's call feedback records and whose chain reads it
/// (`call_plan::Method`).
pub(crate) fn is_method_call(instruction: WideInstruction) -> bool {
    quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::MethodCall) && CallForm::of(instruction) == Some(CallForm::Stepped)
}

/// The callee the call site `instruction` makes calls, if its body's call chain makes it: the
/// latest its site reached.
fn called<'a>(instruction: WideInstruction, site: &'a DirectCandidates) -> Option<&'a DirectCallee> {
    site.first().filter(|_| chains(instruction))
}

/// Every callee the call site `instruction` reached, latest first, if its body's call chain makes it.
fn reached<'a>(instruction: WideInstruction, site: &'a DirectCandidates) -> &'a [DirectCallee] {
    if chains(instruction) { site } else { &[] }
}

impl Callees {
    /// The function each call site of `code` was recorded calling (`candidates`, by PC).
    pub(crate) fn recorded(code: &[WideInstruction], candidates: &[DirectCandidates]) -> Vec<Option<quench_runtime::value::Value>> {
        code.iter().zip(candidates).map(|(instruction, site)| called(*instruction, site).map(|callee| callee.callee)).collect()
    }

    /// The callee each call site of `code` calls (`candidates`, by PC), `body_of` the function it
    /// runs, and `stable` the Stable fact holding it by PC.
    pub(crate) fn of(code: &[WideInstruction], candidates: &[DirectCandidates], body_of: &dyn Fn(&DirectCallee) -> Option<InlineBody>, stable: Vec<Option<Held>>,
        natives: Vec<Option<(quench_runtime::value::Value, quench_runtime::heap::Native, u32)>>) -> Self {
        let mut owned = Vec::new();
        let at = code.iter().zip(candidates).map(|(instruction, site)| {
            let callees = reached(*instruction, site);
            let start = owned.len();
            owned.extend(callees.iter().cloned());
            (!callees.is_empty()).then_some(start..owned.len())
        }).collect();
        let bodies = owned.iter().map(body_of).collect();
        Self { owned: owned.into_boxed_slice(), at, bodies, stable, natives, proven: Vec::new() }
    }

    /// The callees of an image's body (`aot_image::base`): each call site of `code`, the code of
    /// `function`, whose callee `facts` prove, calls it through its image record (`proven`).
    pub(crate) fn proven(program: &quench_runtime::bytecode::ResidualProgram, function: u32, code: &[WideInstruction],
        facts: &quench_runtime::compile::aot_facts::AotFacts) -> Self {
        let facts = facts.function_facts(function);
        let proven = code.iter().enumerate().map(|(pc, instruction)| {
            let callee = facts.call_target(pc as u32).filter(|_| chains(*instruction))?;
            program.function_get(callee as usize).map(|body| proven_facts(callee, body))
        }).collect();
        Self { proven, ..Self::default() }
    }

    /// The builtin each call site runs on its row, by PC: a site whose callee is a native the b-jit runs on one.
    pub(crate) fn builtins(&self) -> Vec<Option<BuiltinCall>> {
        self.natives.iter().map(|native| {
            let (value, native, _) = (*native)?;
            Some(BuiltinCall { identity: value.to_bits(), row: builtin::of(native)? })
        }).collect()
    }

    /// The holders the builtins' guards depend on: a change of a holder's values retires the body (`Body::holders`).
    pub(crate) fn native_holders(&self) -> Vec<u32> {
        self.natives.iter().flatten().filter(|(_, native, _)| builtin::of(*native).is_some()).map(|(_, _, holder)| *holder).collect()
    }

    /// Whether every call of `code` is a direct one (`LaneMethod::closed`).
    pub(crate) fn closed(&self, code: &[WideInstruction]) -> bool {
        code.iter().zip(&self.at).all(|(instruction, callee)| !crate::compiled::meaning_compiled::is_call(*instruction) || (callee.is_some() && !is_method_call(*instruction)))
    }

    /// What each call site recorded of every callee it reached, latest first, by PC: the targets of its
    /// chain's direct rows. The first of each is the callee a stitch may inline.
    ///
    /// # Safety
    /// Each owned callee's `cell` is the address of its function's live entry cell.
    pub(crate) unsafe fn targets(&self) -> Vec<Vec<DirectFacts>> {
        if !self.proven.is_empty() { return self.proven.iter().map(|facts| facts.iter().copied().collect()).collect(); }
        // SAFETY: the caller's contract.
        self.at.iter().enumerate().map(|(pc, callees)| callees.clone().into_iter().flatten().map(|at| unsafe { DirectFacts::of(&self.owned[at], self.held(pc, at)) }).collect()).collect()
    }

    /// Whether the callee `index` of the site at `pc` is the one its callee register holds by a Stable fact: the latest
    /// callee the site recorded, whose global the fact names (`stable`).
    fn held(&self, pc: usize, index: usize) -> bool {
        self.stable.get(pc).is_some_and(Option::is_some) && self.at[pc].as_ref().is_some_and(|at| at.start == index)
    }

    /// The Stable facts the body's direct rows rely on (`held`): a body subscribes to them, so their change retires it,
    /// which is what lets a row skip the callee register's identity check.
    pub(crate) fn held_facts(&self) -> impl Iterator<Item = GlobalFact> + '_ {
        self.stable.iter().flatten().filter_map(|held| Held::global(*held))
    }

    /// The captured identities the body's direct rows rely on (`Held::Captured`) at the call sites in `pcs` (the
    /// span its code covers): its entry checks each once. A site outside it is no row of the body's: a loop region
    /// of a function that calls a captured callee before its loop (`c(row); for (..) {}`) entered with no captures
    /// word bound (it calls nothing, `osr_lane::bind_captures`) and its entry test read `undefined` as the view.
    pub(crate) fn captured_identities(&self, pcs: std::ops::RangeInclusive<usize>) -> impl Iterator<Item = (u16, u64)> + '_ {
        self.stable.iter().enumerate().filter(move |(pc, _)| pcs.contains(pc)).filter_map(|(_, held)| held.as_ref()).filter_map(|held| match *held {
            Held::Captured { slot, callee } => Some((slot, callee)),
            Held::Global(_) => None,
        })
    }

    /// The call `shape` planned as its callee's rows (`targets`: what each site recorded), in
    /// `context`, which it spends from; none where the callee stays a call.
    pub(crate) fn inlined_call(&self, targets: &[Vec<DirectFacts>], region: &LoopRegion, context: &mut Context, shape: &CallShape) -> Option<InlineSite> {
        // A method call's callee is read into a scratch register, not the callee register an inlined site proves.
        if shape.method.is_some() {
            return None;
        }
        let at = self.at.get(shape.call_pc as usize)?.as_ref()?.start;
        let (callee, body) = (&self.owned[at], self.bodies[at].as_ref()?);
        let numbers = |index: usize| body.numbers.get(&index).copied().or_else(|| region.number_constant(index));
        let inlinable = Callee { function: callee.target.function, identity: callee.callee, code: &body.code, registers: body.registers, homes: &body.homes, numbers: &numbers, live: &body.live, stable: self.stable[shape.call_pc as usize].and_then(Held::global), captures: (callee.capture_values.addr(), callee.capture_len) };
        let mut trial = context.clone();
        let site = InlineSite::plan(shape, targets[shape.call_pc as usize].first()?, &inlinable, &mut trial)
            .inspect_err(|why| super::note(|_, rejects| *rejects.entry(format!("inline refused: {why:?}")).or_default() += 1))
            .ok()?;
        *context = trial;
        note_inlined();
        Some(site)
    }
}

/// The hole of a direct row that holds its record's address (`call_plan::direct_part`).
const RECORD_HOLE: u8 = 0;

/// An image's direct rows (`LaneHelpers::Ahead`) name their record by the site's `symbol`, which the
/// loader places and binds (`aot_image::live`): the record the plan owns is the template the image
/// carries (`aot_image::CallSite`), its address no hole of the code.
pub(crate) fn name_records(parts: &mut [super::call_plan::CallPart], records: &[Box<quench_bjit::call_record::CallRecord>], symbol: crate::aot_image::Symbol) {
    let addresses: Vec<u64> = records.iter().map(|record| std::ptr::from_ref(&**record).addr() as u64).collect();
    for part in parts.iter_mut().filter(|part| addresses.contains(&part.holes[usize::from(RECORD_HOLE)])) {
        part.holes[usize::from(RECORD_HOLE)] = 0;
        part.symbols = [Some((RECORD_HOLE, symbol)), None];
    }
}
