use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn mapped_argument_load(
        &self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        fallback: Value,
    ) -> Value {
        let function = &p.functions[self.frames[frame].function as usize];
        let Some(argument_slot) = mapped_arguments_slot(function, slot) else {
            return fallback;
        };
        let arguments = if self.local_slot_is_environment_owned(p, frame, argument_slot) {
            self.heap
                .environment_slot(self.frames[frame].env, argument_slot)
                .unwrap_or(Value::UNDEFINED)
        } else {
            self.frames[frame]
                .locals
                .get(argument_slot)
                .copied()
                .unwrap_or(Value::UNDEFINED)
        };
        let Some(index) = self.mapped_argument_index(arguments, slot) else {
            return fallback;
        };
        match self.heap.get(arguments) {
            Some(cell @ Cell::Array { .. }) => cell
                .array_elements()
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted())
                .unwrap_or(Value::UNDEFINED),
            _ => fallback,
        }
    }

    pub(super) fn mapped_argument_store(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        value: Value,
    ) {
        let function = &p.functions[self.frames[frame].function as usize];
        let Some(argument_slot) = mapped_arguments_slot(function, slot) else {
            return;
        };
        let arguments = if self.local_slot_is_environment_owned(p, frame, argument_slot) {
            self.heap
                .environment_slot(self.frames[frame].env, argument_slot)
                .unwrap_or(Value::UNDEFINED)
        } else {
            self.frames[frame]
                .locals
                .get(argument_slot)
                .copied()
                .unwrap_or(Value::UNDEFINED)
        };
        self.store_mapped_argument(arguments, slot, value);
    }

    pub(super) fn store_environment_mapped_argument(
        &mut self,
        p: &ResidualProgram,
        function: u32,
        environment: Value,
        slot: usize,
        value: Value,
    ) {
        let function = &p.functions[function as usize];
        let Some(argument_slot) = mapped_arguments_slot(function, slot) else {
            return;
        };
        if !function.local_slot_uses_environment(argument_slot) {
            return;
        }
        let arguments = self
            .heap
            .environment_slot(environment, argument_slot)
            .unwrap_or(Value::UNDEFINED);
        self.store_mapped_argument(arguments, slot, value);
    }

    fn mapped_argument_index(&self, arguments: Value, slot: usize) -> Option<usize> {
        self.object_data(arguments)
            .and_then(Object::arguments_map)
            .and_then(|mapping| {
                mapping
                    .iter()
                    .position(|mapped| *mapped != u16::MAX && usize::from(*mapped) == slot)
            })
    }

    fn store_mapped_argument(&mut self, arguments: Value, slot: usize, value: Value) {
        let Some(index) = self.mapped_argument_index(arguments, slot) else {
            return;
        };
        let _ = self.set_array_element(arguments, index, value);
    }

    pub(super) fn sync_mapped_argument(&mut self, object: Value, index: usize, value: Value) {
        let Some(slot) = self
            .object_data(object)
            .and_then(Object::arguments_map)
            .and_then(|mapping| mapping.get(index).copied())
            .filter(|slot| *slot != u16::MAX)
            .map(usize::from)
        else {
            return;
        };
        // The mapping belongs to the one activation whose `arguments` binding holds this object.
        // Writes almost always come from that activation or its callees, so search inward-out.
        let Some(owner) = self
            .frames
            .iter()
            .rposition(|frame| self.frame_owns_arguments(frame, object))
        else {
            return;
        };
        if self.frames[owner].captured {
            if let Some(target) = self.heap.environment_slot_mut(self.frames[owner].env, slot) {
                *target = value;
            }
        } else if let Some(target) = self.frames[owner].locals.get_mut(slot) {
            *target = value;
        }
    }

    fn frame_owns_arguments(&self, frame: &Frame, object: Value) -> bool {
        let Some(function) = self
            .programs
            .residual(frame.program)
            .and_then(|program| program.functions.get(frame.function as usize))
        else {
            return false;
        };
        let Some(slot) = function.arguments_slot.map(usize::from) else {
            return false;
        };
        if frame.captured {
            function.local_slot_uses_environment(slot)
                && self.heap.environment_slot(frame.env, slot) == Some(object)
        } else {
            frame.locals.get(slot) == Some(&object)
        }
    }

    pub(super) fn unmap_argument_index(&mut self, object: Value, index: usize) {
        if let Some(mapping) = self
            .object_data_mut(object)
            .and_then(Object::arguments_map_mut)
            && let Some(slot) = mapping.get_mut(index)
        {
            *slot = u16::MAX;
        }
    }
}

fn mapped_arguments_slot(function: &crate::bytecode::Function, parameter: usize) -> Option<usize> {
    let slot = function.arguments_slot?;
    (function.arguments_are_mapped() && parameter < usize::from(function.params))
        .then_some(usize::from(slot))
}
