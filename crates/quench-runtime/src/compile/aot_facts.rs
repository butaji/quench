//! Closed-world facts for AOT: what holds of the program alone when no `eval`, `Function` or
//! dynamic import can add code (`SourceDemands`). Each fact is a dependency in the one
//! dependency system (`code_dependencies`), never a per-use check, and is read by every tier
//! that lowers the program (lane bodies, BaseJIT bodies), so no layer re-derives it.
//!
//! - Call targets: a call whose callee register holds a closure, or a binding written exactly
//!   once with one (`census`), names that function (direct call, inlinable).
//! - Constructor shapes: the fields a constructor's straight-line prologue stores on `this`,
//!   in order (`shapes`), for the shape a `new` of it allocates.
//! - Number representations: registers that hold only Numbers at every read and write, as
//!   `Int32` or `Float64` (`flow`), so lane bodies keep them unboxed.
//!
//! The analyses run over the bytecode (`flow`, `effects`) and reuse its dataflow driver
//! (`dataflow.rs`), its successor table and its register uses. Owner: AOT facts.
#![allow(dead_code)] // read by the AOT lowering, which is not wired to these facts yet

mod blocks;
mod census;
mod effects;
mod flow;
mod gate;
mod scope;
mod shapes;
mod value;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use crate::bytecode::ResidualProgram;
use crate::typed_ir::Representation;
use census::Census;
use flow::FunctionFlow;
use scope::Scope;

/// The facts of one program.
#[derive(Default)]
pub struct AotFacts {
    /// `(function, pc)` of a plain call → the function it always calls.
    pub(crate) call_targets: BTreeMap<(u32, u32), u32>,
    /// `(function, pc)` of a construction (`new`) → the function it always constructs.
    pub(crate) construct_targets: BTreeMap<(u32, u32), u32>,
    /// Function id of a constructor → the fields its `new` stores on the object, in order (atom numbers).
    pub(crate) constructor_shapes: BTreeMap<u32, Box<[u32]>>,
    /// Function id → register → the Number representation it always holds.
    pub(crate) numbers: BTreeMap<u32, Box<[Option<Representation>]>>,
}

impl AotFacts {
    pub(crate) fn is_empty(&self) -> bool {
        self.call_targets.is_empty() && self.construct_targets.is_empty() && self.constructor_shapes.is_empty() && self.numbers.is_empty()
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
    /// The function the plain call at `pc` always calls.
    pub fn call_target(&self, pc: u32) -> Option<u32> {
        self.facts.call_targets.get(&(self.function, pc)).copied()
    }

    /// The function the construction (`new`) at `pc` always constructs.
    pub fn construct_target(&self, pc: u32) -> Option<u32> {
        self.facts.construct_targets.get(&(self.function, pc)).copied()
    }

    /// Per register, the Number representation it always holds.
    pub fn numbers(&self) -> Option<&'a [Option<Representation>]> {
        self.facts.numbers.get(&self.function).map(|numbers| &**numbers)
    }

    /// The fields a `new` of this function stores on its object, in order (atom numbers).
    pub(crate) fn constructor_shape(&self) -> Option<&'a [u32]> {
        self.facts.constructor_shapes.get(&self.function).map(|fields| &**fields)
    }
}

/// The facts of `program`, or none where the program can add code at run time.
pub fn analyze(program: &ResidualProgram) -> AotFacts {
    if !gate::closed_world(program) {
        return AotFacts::default();
    }
    let scope = Scope::of(program);
    let flows: Vec<FunctionFlow> = (0..program.function_count())
        .map(|id| program.with_record(id, |function| flow::analyze_function(id as u32, function, &scope)))
        .collect();
    let census = Census::of(&flows);
    let mut facts = AotFacts::default();
    let mut constructed = BTreeSet::new();
    for (id, flow) in flows.iter().enumerate() {
        let id = id as u32;
        for call in &flow.calls {
            let Some(target) = census.target(call.callee) else { continue };
            if call.construct {
                constructed.insert(target);
                facts.construct_targets.insert((id, call.pc), target);
            } else {
                facts.call_targets.insert((id, call.pc), target);
            }
        }
        if flow.numbers.iter().any(Option::is_some) {
            facts.numbers.insert(id, flow.numbers.clone());
        }
    }
    for target in constructed {
        let fields = flows.get(target as usize).and_then(|flow| flow.shape.as_deref());
        if let Some(fields) = fields.filter(|fields| !fields.is_empty()) {
            facts.constructor_shapes.insert(target, Box::from(fields));
        }
    }
    facts
}
