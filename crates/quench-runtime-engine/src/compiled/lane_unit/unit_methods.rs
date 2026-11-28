use super::*;

impl Unit {
    /// The effect row of the whole unit: the join of each instruction's row (`performed`), where an instruction
    /// names no operation its row is ANY.
    pub(crate) fn effects(&self) -> quench_runtime::staged::Effects {
        self.instructions.iter().fold(quench_runtime::staged::Effects::NONE, |row, instruction| {
            let effects = crate::compiled::performed(instruction).map_or(quench_runtime::staged::Effects::ANY, |operation| operation.effects());
            row.join(effects)
        })
    }

    /// `frame` and the frames around it, outermost first: the activations a leave inside it
    /// makes.
    pub(crate) fn chain(&self, frame: usize) -> Vec<usize> {
        let mut chain = vec![frame];
        while let Some(parent) = self.frames[*chain.last().expect("a chain has a frame")].parent {
            chain.push(parent);
        }
        chain.reverse();
        chain
    }

    /// Maximum simultaneous inlined activations, derived from the parent graph.
    pub(crate) fn recovery_depth(&self) -> usize {
        (0..self.frames.len()).map(|index| self.chain(index).len()).max().unwrap_or(0)
    }

    /// Project the original functions' recovery liveness onto unit registers. A suspended
    /// frame retains both its call inputs and continuation state, and so do the frames
    /// around it; callee facts include implicit observations such as legacy f.arguments.
    /// The unit's own CFG then refines the projection, so uses the splice introduces (the
    /// result move after a returning operation, for example) stay live. `None` for a unit
    /// wider than one register set, whose lowering projects each frame's own masks per
    /// query instead (`Lowering::is_live`).
    pub(crate) fn register_masks<'a>(
        &self, program: &'a quench_runtime::ResidualProgram, caller: u32,
    ) -> Option<std::borrow::Cow<'a, quench_runtime::compile::liveness::RegisterMasks>> {
        use quench_runtime::compile::liveness::{ROOT_MAP_REGISTERS, RegisterMasks, RegisterSet, register_bit};
        let caller_masks = program.register_root_masks(caller).filter(|masks| !masks.is_windowed())?;
        if self.frames.is_empty() && self.origins.iter().enumerate().all(|(pc, origin)|
            *origin == Origin::Caller(pc as u32)) {
            return Some(std::borrow::Cow::Borrowed(caller_masks));
        }
        if self.registers > ROOT_MAP_REGISTERS { return None; }
        // Frames follow their parents (`Emitter::emit`).
        let mut suspended: Vec<RegisterSet> = Vec::with_capacity(self.frames.len());
        let mut callees = Vec::with_capacity(self.frames.len());
        for frame in &self.frames {
            let (parent, outer) = match frame.parent {
                None => (caller_masks, RegisterSet::default()),
                Some(parent) => (callees[parent], suspended[parent]),
            };
            let call = frame.call as usize;
            let around = parent.get(call)? | (parent.get(call + 1).unwrap_or_default() & !frame.result.map_or_else(RegisterSet::default, |result| register_bit(result - frame.parent_base(&self.frames))));
            suspended.push(outer | around.checked_shl(u32::from(frame.parent_base(&self.frames)))?);
            callees.push(program.register_root_masks(frame.target.function).filter(|masks| !masks.is_windowed())?);
        }
        let mut masks = self.origins.iter().map(|origin| match *origin {
            Origin::Caller(pc) => caller_masks.get(pc as usize),
            Origin::Inlined { frame, pc } => Some(suspended[frame]
                | callees[frame].get(pc as usize)?.checked_shl(u32::from(self.frames[frame].base))?),
        }).collect::<Option<Vec<_>>>()?;
        masks.push(caller_masks.get(self.end as usize)?);
        let mut transfers: Vec<_> = self.instructions.iter().map(|instruction|
            quench_runtime::compile::liveness::transfer(*instruction, program,
                &program.field_sites, &program.superinstructions)).collect();
        // A marker defines its callee's registers: call admission binds every one.
        for (frame, pc) in self.frames.iter().zip(&self.markers) {
            transfers[*pc].1 = (0..frame.registers).fold(RegisterSet::default(), |mask, register| mask | register_bit(frame.base + register));
        }
        // A returning instruction continues at the function's terminal `Return` where it has
        // one, whose register then holds its value; otherwise it completes the activation.
        let terminal = self.instructions.len().checked_sub(1)
            .filter(|terminal| matches!(Operation::<u64, ()>::of(self.instructions[*terminal]), Some(Operation::Return { .. })));
        let mut starts = Vec::with_capacity(self.instructions.len() + 1);
        let mut successors = Vec::new();
        for (pc, instruction) in self.instructions.iter().enumerate() {
            starts.push(successors.len());
            if instruction.returns_from_frame() {
                if let Some(terminal) = terminal {
                    transfers[pc].1 |= register_bit(self.instructions[terminal].register_a());
                    successors.push(terminal);
                }
                continue;
            }
            if let Some(frame) = self.markers.iter().position(|marker| *marker == pc) {
                successors.extend(self.frames[frame].alternative);
            }
            // The one successor table: a branch table continues at every one of its entries.
            quench_runtime::bytecode::for_each_instruction_successor(pc, *instruction, |successor| successors.push(successor));
        }
        starts.push(successors.len());
        let edges = vec![quench_runtime::bytecode::ControlEdge::Normal; successors.len()];
        let table = quench_runtime::bytecode::Successors::from_parts(&starts, &successors, &edges);
        let live = quench_runtime::compile::liveness::analyze_graph(&transfers, &table, |pc| masks[pc]);
        Some(std::borrow::Cow::Owned(RegisterMasks::new(live)))
    }
}

