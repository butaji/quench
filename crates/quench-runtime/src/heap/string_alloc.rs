//! String records: a one-unit ASCII string's single record, any other string's own.
use super::*;

impl Heap {
    /// Allocates a string record. Strings have no legacy-slot form, so their space holds as
    /// many records as the legacy slots do (`STRING_RECORD_CAPACITY`); exhausting it is out
    /// of memory, as exhausting the slots is.
    #[inline(always)]
    pub(crate) fn alloc_string(&mut self, string: JsString) -> Value {
        match string.single_ascii_unit() {
            None => self.alloc_string_record(string),
            Some(unit) => self.alloc_ascii_unit_string(string, unit),
        }
    }

    /// `alloc_string` of a one-unit ASCII string: the one record of that unit.
    #[inline(never)]
    fn alloc_ascii_unit_string(&mut self, string: JsString, unit: u8) -> Value {
        let strings = self.ascii_unit_strings.get_or_insert_with(|| Box::new([Value::UNDEFINED; ASCII_UNITS]));
        let cached = strings[usize::from(unit)];
        if !cached.is_undefined() {
            return cached;
        }
        let value = self.alloc_string_record(string);
        if let Some(strings) = &mut self.ascii_unit_strings {
            strings[usize::from(unit)] = value;
        }
        value
    }

    #[inline]
    fn alloc_string_record(&mut self, string: JsString) -> Value {
        #[cfg(feature = "profile-aggregate")]
        {
            let kind = CellKind::String as usize;
            // Do not materialize ropes in the profiler: production charges the flat payload
            // only when it already exists. `capacity()` flattens a rope and perturbs the
            // allocation shape the profile is meant to observe.
            let bytes = string.flat_unit_bytes();
            self.gc_profile.allocated_kinds[kind] += 1;
            *self.allocated_variants.entry("string").or_default() += 1;
            self.gc_profile.allocated_payload_bytes[kind] += bytes as u64;
            self.gc_profile.allocated_size_buckets[kind][Self::size_bucket(bytes)] += 1;
        }
        self.charge_materialized_text();
        let payload = string.flat_unit_bytes();
        let weight = 1 + payload / payload_unit();
        self.payload_bytes = self.payload_bytes.saturating_add(payload);
        self.allocated += weight;
        self.total_allocations += 1;
        match self.string_records.try_insert(string) {
            Ok(record_id) => Value::direct_string(record_id),
            Err(_) => panic!("string record space exhausted"),
        }
    }
}
