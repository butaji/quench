//! Calls to small leaf functions are inlined into the caller's operator
//! stream as it is lowered. A call becomes: the arguments stored into fresh
//! caller locals, the callee's other locals reset to their defaults, and the
//! callee body as a block of the callee's result type, in which `return`
//! branches to that block. Wasm's structured control keeps every relative
//! depth inside the body valid, because the block stands where the callee's
//! function label stood. Traps, memory and global accesses keep their meaning
//! in the caller's instance; only the activation (and so the call depth that
//! exhausts the stack) disappears.
//!
//! A function that calls may be copied only into its own call sites: a
//! recursion unfolds by one level per round, the copy's own calls staying
//! calls, so the recursion still runs and still exhausts the stack, only at
//! a greater depth (the limit is the implementation's to choose).
//!
//! No round holds operators. Each round inlines the bodies as the previous
//! round left them, so a body's form after `k` rounds is a stream: the form
//! after `k - 1` rounds with each of round `k - 1`'s leaf calls replaced by
//! that leaf's own form after `k - 1` rounds. What lowering and the next
//! round need to know of a form (its locals, first writes, accesses, length
//! and calls) follows from the previous form and the copies one pass over it
//! makes, so only that pass reads it, and every read of a body or of a leaf
//! it copies comes from the frontend's source again.

use wasmparser::{BinaryReaderError, BlockType, Operator, ValType};

use super::{WasmMemory, WasmSignature, WasmSignatures, WasmType};
use crate::Diagnostic;

/// Operators a leaf callee may have to be copied into each call site when
/// the interpreter runs the module: a call costs it a frame and a dispatch,
/// so copying a body pays up to this size. The budget bounds code growth per
/// call site; larger callees keep their call.
const INTERPRETED_LEAF_OPERATORS: usize = 512;

/// Operators up to which a module lowered for compiled code inlines leaves as the
/// interpreter does. A compiled call is cheap, but its activation still costs a frame, a
/// stack check, an interrupt check and a status test, and a copy lets Cranelift optimize the
/// leaf with its caller's values. Wasm compiles eagerly, so every copy is compiled whether it
/// runs or not; past this size the copies' compile time outweighs them on a module's first
/// runs, and larger modules keep every call.
const COMPILED_INLINING_MODULE_OPERATORS: usize = 32_768;

#[cfg(test)]
thread_local! {
    /// Set by tests of compiled calls themselves: compiled lowering then keeps every call.
    pub(crate) static COMPILED_CALLS_KEPT_FOR_TEST: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// The leaf budget for a module lowered for `mode`, whose operators `module_operators`
/// counts (asked only for compiled code).
pub(super) fn leaf_budget(
    mode: crate::ExecutionMode,
    module_operators: impl FnOnce() -> usize,
) -> usize {
    match mode {
        crate::ExecutionMode::Interpreted => INTERPRETED_LEAF_OPERATORS,
        #[cfg(test)]
        crate::ExecutionMode::Compiled
            if COMPILED_CALLS_KEPT_FOR_TEST.with(std::cell::Cell::get) =>
        {
            0
        }
        crate::ExecutionMode::Compiled
            if module_operators() <= COMPILED_INLINING_MODULE_OPERATORS =>
        {
            INTERPRETED_LEAF_OPERATORS
        }
        crate::ExecutionMode::Compiled => 0,
    }
}

/// Inlining rounds. A function that only called leaves is call-free after a
/// round and may be inlined in the next, so code grows by at most this many
/// nested budgets per call site.
const INLINE_ROUNDS: usize = 2;

/// A frontend operator stream that can be read again for every pass.
pub(super) trait Operators<'a>:
    Clone + IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>
{
}

impl<'a, T> Operators<'a> for T where
    T: Clone + IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>
{
}

/// A defined function as the frontend gave it: its declared locals and its operators.
pub(super) struct Source<I> {
    pub(super) locals: Vec<ValType>,
    pub(super) operators: I,
}

/// What receives a body's operators, in order.
pub(super) type Sink<'s, 'a> = dyn FnMut(Operator<'a>) -> Result<(), Diagnostic> + 's;

/// Why a stream ended early: its receiver needs no more operators, or reading failed.
enum Halt {
    Enough,
    Failed(Diagnostic),
}

impl From<Diagnostic> for Halt {
    fn from(diagnostic: Diagnostic) -> Self {
        Self::Failed(diagnostic)
    }
}

/// A receiver inside the plan, which may stop its stream.
type Receiver<'s, 'a> = dyn FnMut(Operator<'a>) -> Result<(), Halt> + 's;

/// How `operator` changes the structured nesting it leaves for the operators after it.
fn nesting_after(nesting: usize, operator: &Operator<'_>) -> usize {
    match operator {
        Operator::Block { .. }
        | Operator::Loop { .. }
        | Operator::If { .. }
        | Operator::Try { .. }
        | Operator::TryTable { .. } => nesting + 1,
        Operator::End | Operator::Delegate { .. } => nesting.saturating_sub(1),
        _ => nesting,
    }
}

/// Declared locals whose first textual use is a `local.set`/`local.tee` at
/// function-body nesting: straight-line body code runs in order, so every read
/// of such a local follows that write. Observes a body's operators in order.
pub(super) struct FirstUse {
    params: u16,
    assigned: Vec<bool>,
    seen: Vec<bool>,
    nesting: usize,
}

impl FirstUse {
    pub(super) fn new(params: u16, declared: usize) -> Self {
        Self {
            params,
            assigned: vec![false; declared],
            seen: vec![false; declared],
            nesting: 0,
        }
    }

    pub(super) fn observe(&mut self, operator: &Operator<'_>) {
        if let Operator::LocalGet { local_index }
        | Operator::LocalSet { local_index }
        | Operator::LocalTee { local_index } = operator
            && let Some(index) = (*local_index as usize).checked_sub(usize::from(self.params))
            && index < self.seen.len()
            && !self.seen[index]
        {
            self.seen[index] = true;
            self.assigned[index] =
                self.nesting == 0 && !matches!(operator, Operator::LocalGet { .. });
        }
        self.nesting = nesting_after(self.nesting, operator);
    }

    pub(super) fn finish(self) -> Vec<bool> {
        self.assigned
    }
}

/// A body's form after some rounds, as lowering and the next round need it.
pub(super) struct Form {
    /// The locals after the parameters.
    pub(super) locals: Vec<ValType>,
    /// How many locals stood when each round started (the first count is the
    /// declared locals). A round's copies add their locals after these. The
    /// counts end with the last round that changes the body.
    counts: Vec<usize>,
    /// Per local, whether its first use is a write at function-body nesting
    /// (`FirstUse`).
    pub(super) assigned_first: Vec<bool>,
    /// The Memory32 bindings the body accesses directly, by memory index.
    pub(super) memories: Vec<bool>,
    /// The globals the body reads or writes, ascending.
    pub(super) globals: Vec<u32>,
    length: usize,
    calls: bool,
    disqualified: bool,
}

impl Form {
    /// The rounds that change the body.
    fn rounds(&self) -> usize {
        self.counts.len() - 1
    }
}

/// A callee that can replace its call sites in its round, in its form as the round starts.
struct Leaf {
    params: Vec<ValType>,
    result: Option<ValType>,
    form: Form,
}

impl Leaf {
    /// Operators a copy of the leaf adds in place of its call: the parameter
    /// stores, the resets of locals not assigned first, the block, and the
    /// body (one operator per body operator, the final `end` closing the block).
    fn copy_length(&self) -> usize {
        let resets = self
            .form
            .assigned_first
            .iter()
            .filter(|assigned| !**assigned)
            .count();
        self.params.len() + 2 * resets + 1 + self.form.length
    }
}

fn numeric_value_type(ty: &WasmType) -> Option<ValType> {
    Some(match ty {
        WasmType::I32 => ValType::I32,
        WasmType::I64 => ValType::I64,
        WasmType::F32 => ValType::F32,
        WasmType::F64 => ValType::F64,
        _ => return None,
    })
}

fn numeric(ty: &ValType) -> bool {
    matches!(
        ty,
        ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64
    )
}

/// Operators that keep a body from being copied: tail calls, which would
/// replace the caller's activation, and exception handling, whose handler
/// ranges stay per function.
fn disqualifies(operator: &Operator<'_>) -> bool {
    matches!(
        operator,
        Operator::ReturnCall { .. }
            | Operator::ReturnCallIndirect { .. }
            | Operator::ReturnCallRef { .. }
            | Operator::Try { .. }
            | Operator::Catch { .. }
            | Operator::CatchAll
            | Operator::Delegate { .. }
            | Operator::Rethrow { .. }
            | Operator::Throw { .. }
            | Operator::ThrowRef
            | Operator::TryTable { .. }
    )
}

fn calls(operator: &Operator<'_>) -> bool {
    matches!(
        operator,
        Operator::Call { .. } | Operator::CallIndirect { .. } | Operator::CallRef { .. }
    )
}

fn default_value<'a>(ty: ValType) -> Operator<'a> {
    match ty {
        ValType::I32 => Operator::I32Const { value: 0 },
        ValType::I64 => Operator::I64Const { value: 0 },
        ValType::F32 => Operator::F32Const {
            value: wasmparser::Ieee32::from(0.0),
        },
        ValType::F64 => Operator::F64Const {
            value: wasmparser::Ieee64::from(0.0),
        },
        ValType::V128 | ValType::Ref(_) => unreachable!("leaf locals are numeric"),
    }
}

/// Adds the ascending indices of `from` to the ascending `into`.
fn union(into: &mut Vec<u32>, from: &[u32]) {
    if from.iter().all(|index| into.binary_search(index).is_ok()) {
        return;
    }
    into.extend_from_slice(from);
    into.sort_unstable();
    into.dedup();
}

/// The defined functions' signatures and where they start in the function index space. The
/// plan keeps its own copy: lowering adds block types to the module's signature pool.
struct Callees {
    imports: u32,
    defined: Vec<Option<WasmSignature>>,
}

impl Callees {
    fn new(signatures: &WasmSignatures) -> Self {
        Self {
            imports: (signatures.function_count() - signatures.defined_count()) as u32,
            defined: (0..signatures.defined_count() as u32)
                .map(|index| signatures.defined_signature(index).cloned())
                .collect(),
        }
    }

    fn defined_index(&self, function: u32) -> Option<usize> {
        let index = function.checked_sub(self.imports)? as usize;
        (index < self.defined.len()).then_some(index)
    }

    fn defined_signature(&self, index: usize) -> Option<&WasmSignature> {
        self.defined.get(index)?.as_ref()
    }
}

/// The module state a form's accesses are counted against.
pub(super) struct Bindings<'m> {
    pub(super) memories: &'m [WasmMemory],
    pub(super) globals: usize,
}

/// One inlining round's leaves, by defined index.
struct Round {
    leaves: Vec<Option<Leaf>>,
}

/// A module's inlining: the leaves of each round. A body that is not a leaf
/// in the first round never becomes one (inlining only adds operators and
/// numeric locals), so a later round considers only the previous one's leaves.
pub(super) struct LeafPlan<'m, 'a, I> {
    name: &'m str,
    sources: &'m [Source<I>],
    bindings: Bindings<'m>,
    signatures: Callees,
    rounds: Vec<Round>,
    marker: std::marker::PhantomData<Operator<'a>>,
}

impl<'m, 'a, I: Operators<'a>> LeafPlan<'m, 'a, I> {
    pub(super) fn new(
        name: &'m str,
        sources: &'m [Source<I>],
        bindings: Bindings<'m>,
        signatures: &WasmSignatures,
        budget: usize,
    ) -> Result<Self, Diagnostic> {
        let mut plan = Self {
            name,
            sources,
            bindings,
            signatures: Callees::new(signatures),
            rounds: Vec::new(),
            marker: std::marker::PhantomData,
        };
        for round in 0..INLINE_ROUNDS {
            let leaves = (0..sources.len())
                .map(|index| match plan.rounds.last() {
                    Some(previous) if previous.leaves[index].is_none() => Ok(None),
                    _ => plan.leaf(round, index, budget),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if leaves.iter().all(Option::is_none) {
                break;
            }
            plan.rounds.push(Round { leaves });
        }
        Ok(plan)
    }

    fn error(&self, message: &str) -> Diagnostic {
        Diagnostic::unsupported(self.name, message)
    }

    /// The defined function at `index` as a leaf of `round`, in its form when the round
    /// starts.
    fn leaf(&self, round: usize, index: usize, budget: usize) -> Result<Option<Leaf>, Diagnostic> {
        let Some(signature) = self.signatures.defined_signature(index) else {
            return Ok(None);
        };
        let params = signature
            .params
            .iter()
            .map(numeric_value_type)
            .collect::<Option<Vec<_>>>();
        let result = match signature.results.as_slice() {
            [] => Some(None),
            [result] => numeric_value_type(result).map(Some),
            _ => None,
        };
        let (Some(params), Some(result)) = (params, result) else {
            return Ok(None);
        };
        let form = self.form(round, index, Some(budget))?;
        if form.length > budget || form.disqualified || !form.locals.iter().all(numeric) {
            return Ok(None);
        }
        Ok(Some(Leaf {
            params,
            result,
            form,
        }))
    }

    /// The leaf `operator` calls from `caller` in `round`, if it is a call to one.
    fn leaf_call(
        &self,
        operator: &Operator<'_>,
        caller: usize,
        round: usize,
    ) -> Option<(usize, &Leaf)> {
        let Operator::Call { function_index } = operator else {
            return None;
        };
        let callee = self.signatures.defined_index(*function_index)?;
        let leaf = self.rounds[round].leaves.get(callee)?.as_ref()?;
        // A call-free body may enter any other function; a calling body only
        // its own call sites.
        ((callee == caller) == leaf.form.calls).then_some((callee, leaf))
    }

    /// The body at defined `index` after `rounds` rounds. A `budget` lets a body found
    /// longer than it stop at its first round (its form is then incomplete past the
    /// length that exceeds it).
    fn form(&self, rounds: usize, index: usize, budget: Option<usize>) -> Result<Form, Diagnostic> {
        let params = self
            .signatures
            .defined_signature(index)
            .ok_or_else(|| self.error("missing Wasm function signature"))?
            .params
            .len();
        let params = u16::try_from(params).map_err(|_| self.error("too many Wasm parameters"))?;
        let declared = self.sources[index].locals.clone();
        let mut form = Form {
            counts: vec![declared.len()],
            assigned_first: Vec::new(),
            memories: Vec::new(),
            globals: Vec::new(),
            locals: declared,
            length: 0,
            calls: false,
            disqualified: false,
        };
        // The first pass reads the body as declared; each pass then derives the next form
        // from the copies its round makes.
        let mut read = Some(Read {
            first_use: FirstUse::new(params, form.locals.len()),
            memories: vec![false; self.bindings.memories.len()],
            ..Read::default()
        });
        for round in 0..rounds.max(1) {
            let applies = round < rounds;
            let next = self.rounds.get(round + 1).filter(|_| round + 1 < rounds);
            let mut copied = Copies::default();
            let mut nesting = 0_usize;
            let flow = self.stream(round, index, &form, &mut |operator| {
                if let Some(read) = read.as_mut() {
                    read.observe(&self.bindings, &operator);
                    // A body past the budget, or one it may not copy, is no leaf: the rest
                    // of it decides nothing for a leaf check.
                    if budget.is_some_and(|budget| read.length > budget || read.disqualified) {
                        return Err(Halt::Enough);
                    }
                }
                if applies {
                    match self.leaf_call(&operator, index, round) {
                        Some((callee, leaf)) => copied.copy(callee == index, leaf, nesting == 0),
                        None => {
                            copied.kept_calls |= calls(&operator);
                            // A kept call can be inlined by the next round only if it
                            // enters one of that round's leaves.
                            copied.later |= matches!(operator, Operator::Call { function_index }
                                if next.is_some_and(|next| self.signatures.defined_index(function_index)
                                    .is_some_and(|callee| next.leaves[callee].is_some())));
                        }
                    }
                }
                nesting = nesting_after(nesting, &operator);
                Ok(())
            });
            if let Some(read) = read.take() {
                form.assigned_first = read.first_use.finish();
                form.memories = read.memories;
                form.globals = read.globals;
                form.length = read.length;
                form.calls = read.calls;
                form.disqualified = read.disqualified;
            }
            match flow {
                Ok(()) => {}
                Err(Halt::Enough) => return Ok(form),
                Err(Halt::Failed(diagnostic)) => return Err(diagnostic),
            }
            if !applies || !(copied.any || copied.later) {
                break;
            }
            // Copies of other functions are call-free; a copy of the body itself
            // brings its own calls along.
            form.calls = copied.kept_calls || (copied.itself && form.calls);
            form.length += copied.length;
            form.locals.extend(copied.locals);
            form.counts.push(form.locals.len());
            form.assigned_first.extend(copied.assigned_first);
            for (memory, accessed) in copied.memories.iter().enumerate() {
                form.memories[memory] |= *accessed;
            }
            union(&mut form.globals, &copied.globals);
            if !copied.later {
                break;
            }
        }
        Ok(form)
    }

    /// The body at defined `index` as it is lowered, after every round.
    pub(super) fn lowered(&self, index: usize) -> Result<Form, Diagnostic> {
        self.form(self.rounds.len(), index, None)
    }

    /// Streams the body at defined `index` as it is lowered, given its `lowered` form.
    pub(super) fn lower(
        &self,
        index: usize,
        form: &Form,
        sink: &mut Sink<'_, 'a>,
    ) -> Result<(), Diagnostic> {
        match self.stream(self.rounds.len(), index, form, &mut |operator| {
            sink(operator).map_err(Halt::Failed)
        }) {
            Ok(()) | Err(Halt::Enough) => Ok(()),
            Err(Halt::Failed(diagnostic)) => Err(diagnostic),
        }
    }

    /// Streams the body at defined `index` after `rounds` rounds into `sink`, given its
    /// form through at least that many rounds.
    fn stream(
        &self,
        rounds: usize,
        index: usize,
        form: &Form,
        sink: &mut Receiver<'_, 'a>,
    ) -> Result<(), Halt> {
        let Some(round) = rounds.min(form.rounds()).checked_sub(1) else {
            for operator in self.sources[index].operators.clone() {
                sink(operator.map_err(|error| self.error(&error.to_string()))?)?;
            }
            return Ok(());
        };
        let too_many_locals = || self.error("too many Wasm locals");
        let params = self
            .signatures
            .defined_signature(index)
            .ok_or_else(|| self.error("missing Wasm function signature"))?
            .params
            .len();
        // The round's first copied local follows the locals that stood when it started.
        let mut next = params + form.counts[round];
        self.stream(round, index, form, &mut |operator| {
            let Some((callee, leaf)) = self.leaf_call(&operator, index, round) else {
                return sink(operator);
            };
            let base = u32::try_from(next).map_err(|_| too_many_locals())?;
            next += leaf.params.len() + leaf.form.locals.len();
            let parameters = u32::try_from(leaf.params.len()).map_err(|_| too_many_locals())?;
            for parameter in (0..parameters).rev() {
                sink(Operator::LocalSet {
                    local_index: base + parameter,
                })?;
            }
            for (offset, ty) in leaf.form.locals.iter().enumerate() {
                if leaf.form.assigned_first[offset] {
                    continue;
                }
                sink(default_value(*ty))?;
                sink(Operator::LocalSet {
                    local_index: base
                        + parameters
                        + u32::try_from(offset).map_err(|_| too_many_locals())?,
                })?;
            }
            sink(Operator::Block {
                blockty: leaf.result.map_or(BlockType::Empty, BlockType::Type),
            })?;
            let mut depth = 0_u32;
            self.stream(round, callee, &leaf.form, &mut |operator| {
                sink(match operator {
                    Operator::LocalGet { local_index } => Operator::LocalGet {
                        local_index: base + local_index,
                    },
                    Operator::LocalSet { local_index } => Operator::LocalSet {
                        local_index: base + local_index,
                    },
                    Operator::LocalTee { local_index } => Operator::LocalTee {
                        local_index: base + local_index,
                    },
                    Operator::Return => Operator::Br {
                        relative_depth: depth,
                    },
                    operator @ (Operator::Block { .. }
                    | Operator::Loop { .. }
                    | Operator::If { .. }) => {
                        depth += 1;
                        operator
                    }
                    Operator::End => {
                        // The body's final `end` closes the inline block.
                        depth = depth.saturating_sub(1);
                        Operator::End
                    }
                    operator => operator,
                })
            })
        })
    }
}

/// What the first pass over a body finds in it as declared.
struct Read {
    length: usize,
    calls: bool,
    disqualified: bool,
    first_use: FirstUse,
    memories: Vec<bool>,
    globals: Vec<u32>,
}

impl Default for Read {
    fn default() -> Self {
        Self {
            length: 0,
            calls: false,
            disqualified: false,
            first_use: FirstUse::new(0, 0),
            memories: Vec::new(),
            globals: Vec::new(),
        }
    }
}

impl Read {
    fn observe(&mut self, bindings: &Bindings<'_>, operator: &Operator<'_>) {
        self.length += 1;
        self.calls |= calls(operator);
        self.disqualified |= disqualifies(operator);
        self.first_use.observe(operator);
        if let Some(memory) = super::memory::direct_access_memory(bindings.memories, operator) {
            self.memories[memory] = true;
        }
        if let Operator::GlobalGet { global_index } | Operator::GlobalSet { global_index } =
            operator
            && (*global_index as usize) < bindings.globals
            && let Err(position) = self.globals.binary_search(global_index)
        {
            self.globals.insert(position, *global_index);
        }
    }
}

/// What one round's pass over a body copies.
#[derive(Default)]
struct Copies {
    /// The round copied a leaf, the body itself among them.
    any: bool,
    itself: bool,
    /// Operators, locals (with their first writes) and accesses the copies add.
    length: usize,
    locals: Vec<ValType>,
    assigned_first: Vec<bool>,
    memories: Vec<bool>,
    globals: Vec<u32>,
    /// A call the round keeps, and whether a later round may copy into the body.
    kept_calls: bool,
    later: bool,
}

impl Copies {
    /// A copy of `leaf` at a call site, at function-body nesting when `top`: the copy's
    /// parameter stores and resets are the first uses of its locals, and a local the leaf
    /// assigns first is first used inside the copy's block.
    fn copy(&mut self, itself: bool, leaf: &Leaf, top: bool) {
        self.any = true;
        self.itself |= itself;
        // Only a body's own copy brings calls along: a leaf copied elsewhere is call-free.
        self.later |= itself;
        self.length += leaf.copy_length() - 1;
        self.locals
            .extend(leaf.params.iter().chain(&leaf.form.locals).copied());
        self.assigned_first
            .extend(std::iter::repeat_n(top, leaf.params.len()));
        self.assigned_first.extend(
            leaf.form
                .assigned_first
                .iter()
                .map(|assigned| !assigned && top),
        );
        if self.memories.len() < leaf.form.memories.len() {
            self.memories.resize(leaf.form.memories.len(), false);
        }
        for (memory, accessed) in leaf.form.memories.iter().enumerate() {
            self.memories[memory] |= *accessed;
        }
        union(&mut self.globals, &leaf.form.globals);
    }
}
