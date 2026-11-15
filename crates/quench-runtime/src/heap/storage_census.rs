//! `QUENCH_TRACE=storage-census`: per container, the live 8-byte values and how many have a
//! narrow form (`Value::to_narrow`), the bytes that 4-byte storage would save, and the record
//! counts beside them, after a full collection. One line, in KB where sized.
use super::*;

const BYTES_PER_KIB: usize = 1024;
/// The bytes a narrow value saves: the wide value's bytes less the narrow one's.
const NARROW_SAVING_BYTES: usize = std::mem::size_of::<u64>() - std::mem::size_of::<u32>();
/// The words a map entry holds: its key and its value.
const MAP_ENTRY_WORDS: usize = 2;
/// The page size `/proc/self/statm` counts resident pages in, in KiB.
const KIB_PER_PAGE: usize = 4;
/// The census reasons a property vector's run is kept wide for: not all narrow, narrow but of a
/// class no smaller, or narrow of the same class.
const REASON_WIDE: usize = 0;
const REASON_ELIGIBLE: usize = 1;
const REASON_SAME_CLASS: usize = 2;

/// Resident set size of the process, in KiB.
pub(super) fn resident_set_kib() -> usize {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|statm| statm.split_whitespace().nth(1)?.parse::<usize>().ok())
        .map_or(0, |pages| pages * KIB_PER_PAGE)
}

#[derive(Default)]
struct Tally {
    containers: usize,
    values: usize,
    narrow: usize,
    words: usize,
}

impl Tally {
    fn take(&mut self, values: impl Iterator<Item = Value>) {
        self.containers += 1;
        for value in values {
            self.values += 1;
            if value.to_narrow().is_some() {
                self.narrow += 1;
            }
        }
    }

    fn line(&self, name: &str) -> String {
        format!("{name}={}/{}n/{}w/{}c save={}KB", self.values, self.narrow, self.words, self.containers, self.narrow_savings() / BYTES_PER_KIB)
    }

    fn narrow_savings(&self) -> usize {
        self.narrow * NARROW_SAVING_BYTES
    }
}

/// What a census gathers from the heap's storage.
#[derive(Default)]
struct Census {
    own: Tally,
    elements: Tally,
    runs_narrow: usize,
    reasons: [(usize, usize); 3],
    objects: usize,
    arrays: usize,
    functions: usize,
    cells: usize,
    maps: Tally,
    sets: Tally,
    environments: Tally,
    map_bytes: usize,
}

impl Heap {
    pub(super) fn trace_storage_census(&self) {
        let mut census = Census::default();
        self.census_records(&mut census);
        self.census_cells(&mut census);
        let (strings, string_bytes) = self.census_strings();
        let kinds = self.census_kinds();
        let properties = &self.properties;
        let rss_kb = resident_set_kib();
        eprintln!("storage census strings={strings} string_bytes={}KB slot_bytes={} cells[{kinds}]", string_bytes / BYTES_PER_KIB, std::mem::size_of::<Slot>());
        eprintln!("storage census reasons: wide={:?} eligible={:?} same_class={:?}", census.reasons[REASON_WIDE], census.reasons[REASON_ELIGIBLE], census.reasons[REASON_SAME_CLASS]);
        let total = census.own.narrow_savings() + census.elements.narrow_savings() + census.maps.narrow_savings() + census.sets.narrow_savings();
        eprintln!(
            "storage census: rss_kb={rss_kb} records objects={} arrays={} functions={} cells={} arena_words={} {} narrow_runs={} {} {} {} envs={}/{}c map_set_payload={}KB save_total={}KB",
            census.objects,
            census.arrays,
            census.functions,
            census.cells,
            properties.resident_words(),
            census.own.line("own"),
            census.runs_narrow,
            census.elements.line("elements"),
            census.maps.line("map"),
            census.sets.line("set"),
            census.environments.values,
            census.environments.containers,
            census.map_bytes / BYTES_PER_KIB,
            total / BYTES_PER_KIB,
        );
    }

    /// The object, array and function records, and the values of their properties and elements.
    fn census_records(&self, census: &mut Census) {
        let properties = &self.properties;
        self.object_records.for_each_live(|_, object| {
            census.objects += 1;
            census_object(properties, object, census);
        });
        self.array_records.for_each_live(|_, array| {
            census.arrays += 1;
            census_object(properties, array, census);
            let backing = array.elements();
            census.elements.words += backing.capacity();
            census.elements.take(backing.as_slice().iter().copied());
        });
        self.function_records.for_each_live(|_, function| {
            census.functions += 1;
            census_object(properties, &function.object, census);
        });
    }

    /// The maps, sets, and environments among the cells.
    fn census_cells(&self, census: &mut Census) {
        for slot in self.slots.iter() {
            let Some(cell) = slot.cell.as_ref() else { continue };
            census.cells += 1;
            match cell {
                Cell::Map { entries, .. } => {
                    census.map_bytes += entries.payload_bytes();
                    census.maps.words += MAP_ENTRY_WORDS * entries.capacity();
                    census.maps.take(entries.iter().flat_map(|(key, value)| [key, value]));
                }
                Cell::Set { entries, .. } => {
                    census.map_bytes += entries.payload_bytes();
                    census.sets.words += entries.capacity();
                    census.sets.take(entries.iter());
                }
                Cell::Environment { slots, .. } => {
                    census.environments.containers += 1;
                    census.environments.values += slots.len();
                    census.environments.words += slots.len();
                }
                _ => {}
            }
        }
    }

    /// The string records, and their unit storage (shared storage counted once).
    fn census_strings(&self) -> (usize, usize) {
        let (mut strings, mut string_bytes) = (0usize, 0usize);
        let mut seen = rustc_hash::FxHashSet::default();
        self.string_records.for_each_live(|_, string| {
            strings += 1;
            let ((unit_address, unit_bytes), host) = string.memory_parts();
            if unit_bytes > 0 && seen.insert(unit_address) {
                string_bytes += unit_bytes;
            }
            if let Some((address, bytes)) = host
                && seen.insert(address)
            {
                string_bytes += bytes;
            }
        });
        (strings, string_bytes)
    }

    /// The cells by kind, each with its count and the payload it owns beyond its slot.
    fn census_kinds(&self) -> String {
        let mut kinds: std::collections::BTreeMap<&'static str, (usize, usize)> = Default::default();
        for slot in self.slots.iter() {
            let Some(cell) = slot.cell.as_ref() else { continue };
            let kind = match cell {
                Cell::Object(_) => "object",
                Cell::Array { .. } => "array",
                Cell::Function(_) => "function",
                Cell::Environment { .. } => "env",
                Cell::Map { .. } => "map",
                Cell::Set { .. } => "set",
                Cell::Iterator(_) => "iter",
                Cell::Proxy { .. } => "proxy",
                Cell::WeakMap { .. } => "weakmap",
                Cell::AccessorPair { .. } => "accessor",
                Cell::BindingReference { .. } => "binding",
                _ => "other",
            };
            let entry = kinds.entry(kind).or_default();
            entry.0 += 1;
            entry.1 += cell.payload_bytes();
        }
        kinds.iter().map(|(kind, (count, bytes))| format!("{kind}:{count}/{}KB", bytes / BYTES_PER_KIB)).collect::<Vec<_>>().join(",")
    }
}

/// One object's property vector, counted by the reason its run is kept wide (or not counted
/// where it is already narrow, or inline).
fn census_object(properties: &crate::value_vec::ValueArena, object: &Object, census: &mut Census) {
    let vector = object.properties();
    if vector.is_dictionary() {
        return;
    }
    let len = properties.len(vector);
    if !vector.has_inline_property_storage() {
        census.own.words += properties.capacity(vector);
    }
    if vector.is_narrow() || vector.has_inline_property_storage() {
        // Already 4 bytes a value, or in the record's own word: nothing more to save.
        census.runs_narrow += usize::from(vector.is_narrow());
        census.own.containers += 1;
        census.own.values += len;
        return;
    }
    let all_narrow = (0..len).all(|slot| properties.get(vector, slot).is_some_and(|v| v.to_narrow().is_some()));
    let reason = if !all_narrow {
        REASON_WIDE
    } else if properties.narrow_class_smaller(len) {
        REASON_ELIGIBLE
    } else {
        REASON_SAME_CLASS
    };
    census.reasons[reason].0 += 1;
    census.reasons[reason].1 += len;
    census.own.take((0..len).filter_map(|slot| properties.get(vector, slot)));
}
