#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{LoopRegion, MAX_REGION_INSTRUCTIONS, Register, Value, WideInstruction};
use std::collections::{BTreeMap, BTreeSet};

#[quench_macros::ext(LoopRegionOps4)]
impl LoopRegion {
    /// The span of a whole function, for the rows of its BaseJIT body: every instruction, from PC 0
    /// to the last, with the Number constants of the pool and the function's liveness. It names no
    /// registers, arrays, fields or captures: the rows prove each Number operand where they read
    /// it, and a function with no liveness (`None`) or too long to analyze has no span.
    pub(crate) fn of_function(
        instructions: Vec<WideInstruction>,
        constants: &[Value],
        live: Option<&std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    ) -> Option<Self> {
        Self::span(instructions, constants, live, 0)
    }

    /// The role-free span of `instructions` (the code from `header` to a backedge, or a whole
    /// function at 0): what a tier that proves its operands row by row stitches, with no
    /// Cranelift role analysis.
    pub(crate) fn span(
        instructions: Vec<WideInstruction>,
        constants: &[Value],
        live: Option<&std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
        header: u32,
    ) -> Option<Self> {
        if instructions.len() > MAX_REGION_INSTRUCTIONS { return None; }
        Self::unbounded_span(instructions, constants, live, header)
    }

    /// The span of a whole function for an ahead image's base body (`base_body::image_body`): as
    /// `of_function`, with no instruction budget. The budget bounds the JIT's compile time and code
    /// size where it runs; an ahead compile pays neither at run time, and every function needs its
    /// base body, which polls termination at backward jumps as any body does.
    pub(crate) fn of_whole_function(
        instructions: Vec<WideInstruction>,
        constants: &[Value],
        live: Option<&std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    ) -> Option<Self> {
        Self::unbounded_span(instructions, constants, live, 0)
    }

    fn unbounded_span(
        instructions: Vec<WideInstruction>,
        constants: &[Value],
        live: Option<&std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
        header: u32,
    ) -> Option<Self> {
        let backedge = header.checked_add(u32::try_from(instructions.len().checked_sub(1)?).ok()?)?;
        let live = live?.clone();
        Some(Self {
            header,
            backedge,
            numbers: Vec::<Register>::new(),
            tagged: Vec::new(),
            arrays: Vec::new(),
            captures: Vec::new(),
            stored_captures: Vec::new(),
            fields: Vec::new(),
            field_reads: BTreeMap::new(),
            dynamic_field_reads: BTreeMap::new(),
            dynamic_arrays: BTreeSet::new(),
            field_stores: BTreeMap::new(),
            dynamic_field_stores: BTreeMap::new(),
            field_store_sites: Vec::new(),
            held: BTreeMap::new(),
            identity_branches: BTreeMap::new(),
            instructions,
            constants: constants.iter().enumerate().filter_map(|(index, value)| Some((index, value.as_number()?))).collect(),
            live: Some(live),
            inlining: None,
        })
    }
}
