//! A map from the PCs of one loop to values, flat over its instructions.

/// Values by the PCs of one loop (`header + index`), flat over the loop's instructions.
pub(in crate::compiled::osr_loop) struct PcMap<T> {
    header: u32,
    slots: Vec<Option<T>>,
}

impl<T> PcMap<T> {
    pub(in crate::compiled::osr_loop) fn new(header: u32, len: usize) -> Self {
        Self { header, slots: std::iter::repeat_with(|| None).take(len).collect() }
    }

    pub(in crate::compiled::osr_loop) fn insert(&mut self, pc: u32, value: T) {
        self.slots[(pc - self.header) as usize] = Some(value);
    }

    pub(in crate::compiled::osr_loop) fn get(&self, pc: u32) -> Option<&T> {
        self.slots.get(pc.checked_sub(self.header)? as usize)?.as_ref()
    }

    pub(in crate::compiled::osr_loop) fn contains(&self, pc: u32) -> bool {
        self.get(pc).is_some()
    }

    pub(in crate::compiled::osr_loop) fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    /// The entries in PC order.
    pub(in crate::compiled::osr_loop) fn iter(&self) -> impl Iterator<Item = (u32, &T)> + '_ {
        self.slots.iter().enumerate().filter_map(|(index, slot)| Some((self.header + index as u32, slot.as_ref()?)))
    }

    /// The entries in PC order, by value.
    pub(in crate::compiled::osr_loop) fn into_entries(self) -> impl Iterator<Item = (u32, T)> {
        let header = self.header;
        self.slots.into_iter().enumerate().filter_map(move |(index, slot)| Some((header + index as u32, slot?)))
    }
}
