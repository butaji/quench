//! String records: a one-unit ASCII string's single record, any other string's own.
use super::*;

impl Heap {
    /// Allocates a string record. Strings have no legacy-slot form, so their space holds as
    /// many records as the legacy slots do (`STRING_RECORD_CAPACITY`); exhausting it is out
    /// of memory, as exhausting the slots is.
    #[inline(always)]
    pub fn alloc_string(&mut self, string: JsString) -> Value {
        match string.single_ascii_unit() {
            None => self.alloc_string_record(string),
            Some(unit) => self.alloc_ascii_unit_string(string, unit),
        }
    }

    /// `alloc_string` of the string of `units` (`JsString::from_unit_run`): a one-unit ASCII
    /// run that has its record is answered without building the string.
    #[inline]
    pub(crate) fn alloc_unit_run(&mut self, units: crate::vm::wtf16::Units<'_>) -> Value {
        if let crate::vm::wtf16::Units::Ascii(&[unit]) = units
            && let Some(strings) = &self.ascii_unit_strings
            && !strings[usize::from(unit)].is_undefined()
        {
            return strings[usize::from(unit)];
        }
        self.alloc_string(JsString::from_unit_run(units))
    }

    /// `alloc_unit_run` of the ASCII run `bytes`: a short run of two units or more is written
    /// inline into its record, charged as `alloc_string_record` charges it, with none of the
    /// general string's representation dispatch (an exec's captures: ~230 Ir each before).
    #[inline]
    pub(crate) fn alloc_ascii_run(&mut self, bytes: &[u8]) -> Value {
        if bytes.len() == 1 || bytes.len() > crate::vm::wtf16::SHORT_ASCII_CAPACITY || cfg!(feature = "profile-aggregate") {
            return self.alloc_unit_run(crate::vm::wtf16::Units::Ascii(bytes));
        }
        self.charge_materialized_text();
        // A short string's payload is its length (`JsString::flat_unit_bytes`), as its sweep
        // releases it.
        let payload = bytes.len();
        self.payload_bytes = self.payload_bytes.saturating_add(payload);
        self.allocated += 1 + payload / payload_unit();
        match self.string_records.try_insert(JsString::short_ascii(bytes)) {
            Ok(record_id) => Value::direct_string(record_id),
            Err(_) => panic!("string record space exhausted"),
        }
    }

    /// `alloc_ascii_run` of the `len` ASCII bytes that `fill` writes, given the flat ASCII bytes
    /// of `sources` read in place, with no copy of any source. A short string is filled on the
    /// stack and written into its inline record; a longer one is filled straight into its flat
    /// payload, which becomes the record's. `None` when a source is not flat ASCII, before any
    /// record is made.
    #[inline]
    pub(crate) fn alloc_ascii_derived<const N: usize>(
        &mut self,
        sources: [Value; N],
        len: usize,
        fill: impl FnOnce(&mut [u8], [&[u8]; N]),
    ) -> Option<Value> {
        debug_assert!(len <= crate::vm::wtf16::MAX_STRING_UNITS);
        if len <= crate::vm::wtf16::SHORT_ASCII_CAPACITY {
            let mut inline = [0u8; crate::vm::wtf16::SHORT_ASCII_CAPACITY];
            self.with_ascii_sources(sources, |bytes| fill(&mut inline[..len], bytes))?;
            return Some(self.alloc_ascii_run(&inline[..len]));
        }
        let string = self.with_ascii_sources(sources, |bytes| {
            JsString::from_ascii_zeroed_with(len, |out| fill(out, bytes))
        })?;
        Some(self.alloc_string_record(string))
    }

    /// `build` run on the flat ASCII bytes of `sources`, each read in place from its record or
    /// small string. The heap is only read while `build` runs: it makes no record, so nothing
    /// moves or is freed under the bytes. `None`, before `build` runs, when a source is not flat
    /// ASCII.
    pub(crate) fn with_ascii_sources<const N: usize, R>(&self, sources: [Value; N], build: impl FnOnce([&[u8]; N]) -> R) -> Option<R> {
        let texts = sources.map(|source| self.flat_text(source));
        let mut bytes: [&[u8]; N] = [&[]; N];
        for (slot, text) in bytes.iter_mut().zip(&texts) {
            let crate::vm::wtf16::Units::Ascii(units) = text.as_ref()?.units() else {
                return None;
            };
            *slot = units;
        }
        Some(build(bytes))
    }

    /// The string of the one Latin-1 code unit `unit` past ASCII, one record per unit while
    /// no collection empties the table (`ascii_unit_strings`); `None` for any other unit.
    pub(crate) fn latin1_unit_string(&mut self, unit: u16) -> Option<Value> {
        let index = usize::from(unit);
        if !(ASCII_UNITS..UNIT_STRINGS).contains(&index) {
            return None;
        }
        if let Some(strings) = &self.ascii_unit_strings
            && !strings[index].is_undefined()
        {
            return Some(strings[index]);
        }
        let value = self.alloc_string_record(JsString::from_units(&[unit]));
        self.ascii_unit_strings.get_or_insert_with(|| Box::new([Value::UNDEFINED; UNIT_STRINGS]))[index] = value;
        Some(value)
    }

    /// `alloc_string` of a one-unit ASCII string: the one record of that unit.
    #[inline(never)]
    fn alloc_ascii_unit_string(&mut self, string: JsString, unit: u8) -> Value {
        let strings = self.ascii_unit_strings.get_or_insert_with(|| Box::new([Value::UNDEFINED; UNIT_STRINGS]));
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
            self.gc_profile.allocated_size_buckets[kind][crate::profile::size_bucket(bytes)] += 1;
        }
        self.charge_materialized_text();
        let payload = string.flat_unit_bytes();
        let weight = 1 + payload / payload_unit();
        self.payload_bytes = self.payload_bytes.saturating_add(payload);
        self.allocated += weight;
        match self.string_records.try_insert(string) {
            Ok(record_id) => Value::direct_string(record_id),
            Err(_) => panic!("string record space exhausted"),
        }
    }
}
