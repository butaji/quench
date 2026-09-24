use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn field_cache_owner(&self, object: Value, cache: FieldCache) -> Option<Value> {
        let mut owner = object;
        for depth in 0..=cache.depth {
            let current = self.object_data(owner)?;
            if depth == cache.depth {
                return (owner == cache.owner && current.shape() == cache.owner_shape)
                    .then_some(owner);
            }
            if self
                .shape_slot(current.shape(), cache.atom)
                .is_some_and(|slot| self.heap.property_get(current, slot).is_some())
            {
                return None;
            }
            owner = current.proto;
            if owner.is_null() {
                return None;
            }
        }
        None
    }
}
