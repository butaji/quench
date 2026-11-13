//! A map from small dense ids (atoms, registers) to values, kept as one flat vector indexed by
//! the id: a lookup is an index, never a hash or a tree walk.

/// Values by dense `u32` id; an id with no entry holds `None`.
#[derive(Clone, Debug)]
pub(crate) struct DenseMap<V> {
    slots: Vec<Option<V>>,
    #[cfg(feature = "aot")]
    len: usize,
}

impl<V> Default for DenseMap<V> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            #[cfg(feature = "aot")]
            len: 0,
        }
    }
}

impl<V> DenseMap<V> {
    pub(crate) fn get(&self, id: &u32) -> Option<&V> {
        self.slots.get(*id as usize)?.as_ref()
    }
}

#[cfg(feature = "aot")]
impl<V> DenseMap<V> {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    fn slot(&mut self, id: u32) -> &mut Option<V> {
        let index = id as usize;
        if index >= self.slots.len() {
            self.slots.resize_with(index + 1, || None);
        }
        &mut self.slots[index]
    }

    pub(crate) fn insert(&mut self, id: u32, value: V) {
        let absent = self.slot(id).replace(value).is_none();
        self.len += usize::from(absent);
    }

    /// The ids with an entry, ascending.
    pub(crate) fn keys(&self) -> impl Iterator<Item = u32> + '_ {
        self.slots.iter().enumerate().filter(|(_, slot)| slot.is_some()).map(|(id, _)| id as u32)
    }
}
