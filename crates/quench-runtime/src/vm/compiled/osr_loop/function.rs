use super::{LoopRegion, MAX_REGION_INSTRUCTIONS, Register, Value, WideInstruction};
use std::collections::{BTreeMap, BTreeSet};

impl LoopRegion {
    /// The span of a whole function, for the rows of its BaseJIT body: every instruction, from PC 0
    /// to the last, with the Number constants of the pool and the function's liveness. It names no
    /// registers, arrays, fields or captures: the rows prove each Number operand where they read
    /// it, and a function with no liveness (`None`) or too long to analyze has no span.
    pub(in crate::vm) fn of_function(
        instructions: Vec<WideInstruction>,
        constants: &[Value],
        live: Option<&std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    ) -> Option<Self> {
        let backedge = u32::try_from(instructions.len().checked_sub(1)?).ok()?;
        let live = live?.clone();
        (instructions.len() <= MAX_REGION_INSTRUCTIONS).then(|| Self {
            header: 0,
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
