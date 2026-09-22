use super::*;
use crate::bytecode::MAPPED_ARGUMENTS_BIT;

impl<H: Host> Vm<H> {
    pub(super) fn mapped_argument_load(
        &self,
        p: &ResidualProgram,
        frame: usize,
        slot: usize,
        fallback: Value,
    ) -> Value {
        let function = &p.functions[self.frames[frame].function as usize];
        let Some(encoded) = function.arguments_slot else {
            return fallback;
        };
        if encoded & MAPPED_ARGUMENTS_BIT == 0 || slot >= usize::from(function.params) {
            return fallback;
        }
        let argument_slot = usize::from(encoded & !MAPPED_ARGUMENTS_BIT);
        let arguments = if self.frames[frame].captured {
            match self.heap.get(self.frames[frame].env) {
                Some(Cell::Environment { slots, .. }) => slots
                    .get(argument_slot)
                    .copied()
                    .unwrap_or(Value::UNDEFINED),
                _ => Value::UNDEFINED,
            }
        } else {
            self.frames[frame]
                .locals
                .get(argument_slot)
                .copied()
                .unwrap_or(Value::UNDEFINED)
        };
        let Some(index) = self
            .object_data(arguments)
            .and_then(|object| object.arguments_map.as_ref())
            .and_then(|mapping| {
                mapping
                    .iter()
                    .position(|mapped| *mapped != u16::MAX && usize::from(*mapped) == slot)
            })
        else {
            return fallback;
        };
        match self.heap.get(arguments) {
            Some(Cell::Array { elements, .. }) => elements
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
        let Some(encoded) = function.arguments_slot else {
            return;
        };
        if encoded & MAPPED_ARGUMENTS_BIT == 0 || slot >= usize::from(function.params) {
            return;
        }
        let argument_slot = usize::from(encoded & !MAPPED_ARGUMENTS_BIT);
        let arguments = if self.frames[frame].captured {
            match self.heap.get(self.frames[frame].env) {
                Some(Cell::Environment { slots, .. }) => slots
                    .get(argument_slot)
                    .copied()
                    .unwrap_or(Value::UNDEFINED),
                _ => Value::UNDEFINED,
            }
        } else {
            self.frames[frame]
                .locals
                .get(argument_slot)
                .copied()
                .unwrap_or(Value::UNDEFINED)
        };
        let Some(index) = self
            .object_data(arguments)
            .and_then(|object| object.arguments_map.as_ref())
            .and_then(|mapping| {
                mapping
                    .iter()
                    .position(|mapped| *mapped != u16::MAX && usize::from(*mapped) == slot)
            })
        else {
            return;
        };
        let _ = self.set_array_element(arguments, index, value);
    }

    pub(super) fn sync_mapped_argument(&mut self, object: Value, index: usize, value: Value) {
        let mut updates = Vec::new();
        for (frame_index, frame) in self.frames.iter().enumerate() {
            let has_arguments = if frame.captured {
                matches!(self.heap.get(frame.env), Some(Cell::Environment { slots, .. }) if slots.contains(&object))
            } else {
                frame.locals.contains(&object)
            };
            if has_arguments
                && let Some(slot) = self
                    .object_data(object)
                    .and_then(|object| object.arguments_map.as_ref())
                    .and_then(|mapping| mapping.get(index).copied())
                    .filter(|slot| *slot != u16::MAX)
            {
                updates.push((frame_index, usize::from(slot)));
            }
        }
        for (frame_index, slot) in updates {
            if self.frames[frame_index].captured {
                if let Some(Cell::Environment { slots, .. }) =
                    self.heap.get_mut(self.frames[frame_index].env)
                    && let Some(target) = slots.get_mut(slot)
                {
                    *target = value;
                }
            } else if let Some(target) = self.frames[frame_index].locals.get_mut(slot) {
                *target = value;
            }
        }
    }

    pub(super) fn unmap_argument_index(&mut self, object: Value, index: usize) {
        if let Some(mapping) = self
            .object_data_mut(object)
            .and_then(|object| object.arguments_map.as_mut())
            && let Some(slot) = mapping.get_mut(index)
        {
            *slot = u16::MAX;
        }
    }
}
