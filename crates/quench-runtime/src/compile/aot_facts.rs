//! Closed-world facts for AOT: what holds of the program alone when no `eval`, `Function` or
//! dynamic import can add code (`SourceDemands`). Each fact is a dependency in the one
//! dependency system (`code_dependencies`), never a per-use check, and is read by every tier
//! that lowers the program (lane bodies, BaseJIT bodies), so no layer re-derives it.
//!
//! - Call targets: a call whose callee register holds a closure, or a binding written exactly
//!   once with one (`census`), names that function (direct call, inlinable).
//! - Number representations: registers that hold only Numbers at every read and write, as
//!   `Int32` or `Float64` (`flow`), so lane bodies keep them unboxed.
//!
//! The analyses run over the bytecode (`flow`, `effects`) and reuse its dataflow driver
//! (`dataflow.rs`), its successor table and its register uses. Owner: AOT facts.

mod blocks;
mod census;
mod effects;
mod flow;
mod gate;
mod scope;
mod value;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::bytecode::ResidualProgram;
use crate::typed_ir::Representation;
use census::Census;
use flow::FunctionFlow;
use scope::Scope;

/// The facts of one program.
#[derive(Default)]
pub struct AotFacts {
    /// `(function, pc)` of a plain call → the function it always calls, or, for a method read
    /// (`guessed_calls`), the one closure every store of its name writes.
    pub(crate) call_targets: BTreeMap<(u32, u32), u32>,
    /// The `call_targets` that are a method read's guess, not a proof: the receiver may hold another
    /// value of that name (a builtin's, a closure made again, a store through a computed key). Only a
    /// row that guards the callee it reaches takes one (`FunctionFacts::call_target`); one that drops
    /// the guard takes the proven ones only (`FunctionFacts::proven_call_target`).
    pub(crate) guessed_calls: std::collections::BTreeSet<(u32, u32)>,
    /// `(function, pc)` of a construction (`new`) → the function it always constructs.
    pub(crate) construct_targets: BTreeMap<(u32, u32), u32>,
    /// Function id → register → the Number representation it always holds.
    pub(crate) numbers: BTreeMap<u32, Box<[Option<Representation>]>>,
    /// Per function, the names its `prototype` is given, in code order (`prototype_fields`).
    pub(crate) prototype_fields: BTreeMap<u32, Vec<u32>>,
}

impl AotFacts {
    /// The names stored into `function`'s `prototype`, each once, in the order the program's code
    /// first stores them (`AotFacts::prototype_fields`): a guess of its prototype's layout after
    /// `constructor`, which a loader checks against the object it finds (`aot_image::symbols`).
    pub fn prototype_fields(&self, function: u32) -> &[u32] {
        self.prototype_fields.get(&function).map_or(&[], |fields| fields.as_slice())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.call_targets.is_empty() && self.construct_targets.is_empty() && self.numbers.is_empty()
    }

    /// The functions some construction (`new`) always constructs.
    pub fn constructed_functions(&self) -> std::collections::BTreeSet<u32> {
        self.construct_targets.values().copied().collect()
    }

    /// The facts of function `function`, for its lowering.
    pub fn function_facts(&self, function: u32) -> FunctionFacts<'_> {
        FunctionFacts { facts: self, function }
    }
}

/// The facts of one function.
pub struct FunctionFacts<'a> {
    facts: &'a AotFacts,
    function: u32,
}

impl<'a> FunctionFacts<'a> {
    /// The function the plain call at `pc` calls: proven, or a method read's guess (`AotFacts::guessed_calls`).
    /// A row taking it guards the callee it reaches.
    pub fn call_target(&self, pc: u32) -> Option<u32> {
        self.facts.call_targets.get(&(self.function, pc)).copied()
    }

    /// The function the plain call at `pc` always calls, proven: a row may drop its callee guard for it.
    pub fn proven_call_target(&self, pc: u32) -> Option<u32> {
        self.call_target(pc).filter(|_| !self.facts.guessed_calls.contains(&(self.function, pc)))
    }

    /// The function the construction (`new`) at `pc` always constructs.
    pub fn construct_target(&self, pc: u32) -> Option<u32> {
        self.facts.construct_targets.get(&(self.function, pc)).copied()
    }

    /// Per register, the Number representation it always holds.
    pub fn numbers(&self) -> Option<&'a [Option<Representation>]> {
        self.facts.numbers.get(&self.function).map(|numbers| &**numbers)
    }
}

/// The facts of `program`, or none where the program can add code at run time.
pub fn analyze(program: &ResidualProgram) -> AotFacts {
    let world = gate::world(program);
    if world == gate::World::Open {
        return AotFacts::default();
    }
    let scope = Scope::of(program);
    let flows: Vec<FunctionFlow> = (0..program.function_count())
        .map(|id| program.with_record(id, |function| flow::analyze_function(id as u32, function, &scope)))
        .collect();
    let mut census = Census::of(&flows);
    if world == gate::World::Locals { census.open_to_global_code(); }
    let mut facts = AotFacts::default();
    for (id, flow) in flows.iter().enumerate() {
        let id = id as u32;
        for call in &flow.calls {
            let Some(target) = census.target(call.callee) else { continue };
            let guessed = census.guessed(call.callee);
            if call.construct {
                if !guessed { facts.construct_targets.insert((id, call.pc), target); }
            } else {
                facts.call_targets.insert((id, call.pc), target);
                if guessed { facts.guessed_calls.insert((id, call.pc)); }
            }
        }
        for &(base, atom) in &flow.prototype_writes {
            let Some(owner) = census.prototype_owner(base) else { continue };
            let fields = facts.prototype_fields.entry(owner).or_default();
            if !fields.contains(&atom) { fields.push(atom); }
        }
        if flow.numbers.iter().any(Option::is_some) {
            facts.numbers.insert(id, flow.numbers.clone());
        }
    }
    facts
}
