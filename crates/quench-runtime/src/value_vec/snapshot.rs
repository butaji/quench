//! The arena as a snapshot keeps it: written and read in the order `write_snapshot` fixes.
use super::*;

impl ValueArena {
    /// The arena as a snapshot keeps it (`crate::snapshot`): storage positions name places in
    /// `values`, so the vector and its free ranges are written as they are.
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        // Slot owners and representations are derived where they are kept (`Heap::track_slot_representations`).
        let Self { values: _, free, released_values, shape_lengths, shape_chains: _, chain_owners: _, owner_types: _, slot_readers: _, shape_readers: _, tracking: _, dictionaries, free_dictionaries, next_dictionary, nursery: _, narrowed_words: _, settled_values: _, widened_shapes: _, reservations: _, reservation_cycle: _, owner_epoch: _, int_to_number: _ } = self;
        writer.values(&self.values);
        for bucket in 0..BUCKETS {
            writer.u32s(&self.free_starts(bucket));
        }
        let _ = free;
        writer.len(*released_values);
        writer.u32s(shape_lengths);
        writer.len(dictionaries.len());
        for (id, entries) in dictionaries {
            writer.u32(*id);
            writer.len(entries.len());
            for (key, value) in entries {
                writer.u32(*key);
                writer.value(*value);
            }
        }
        writer.u32s(free_dictionaries);
        writer.u32(*next_dictionary);
    }

    pub(crate) fn read_snapshot(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        let values = reader.values()?;
        let mut free: [Vec<u32>; BUCKETS] = [const { Vec::new() }; BUCKETS];
        for bucket in &mut free {
            *bucket = reader.u32s()?;
        }
        let released_values = reader.len()?;
        let mut values: PropertyBuffer = values.into();
        let mut lists = [FreeRuns::EMPTY; BUCKETS];
        for (list, starts) in lists.iter_mut().zip(&free) {
            // Pushed last to first, so the list pops in the order it was written.
            for &start in starts.iter().rev() {
                if start as usize >= values.len() {
                    return Err(crate::snapshot::SnapshotError("free run past the arena".into()));
                }
                values[start as usize] = Value::from_bits(u64::from(list.head));
                *list = FreeRuns { head: start, len: list.len + 1, unreleased: list.unreleased + 1 };
            }
        }
        let shape_lengths = reader.u32s()?;
        let dictionaries = crate::snapshot::read_list(reader, |reader| {
            let id = reader.u32()?;
            let entries = crate::snapshot::read_list(reader, |reader| Ok((reader.u32()?, reader.value()?)))?;
            Ok((id, entries.into_iter().collect()))
        })?
        .into_iter()
        .collect();
        Ok(Self {
            values,
            free: lists,
            released_values,
            shape_lengths,
            dictionaries,
            free_dictionaries: reader.u32s()?,
            next_dictionary: reader.u32()?,
            ..Self::default()
        })
    }
}
