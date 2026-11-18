//! The heap side of JSON.parse: values built as the parse goes, objects taking the layout
//! their keys lead to, and the cache of layouts and strings later parses reuse.

use super::*;

/// A layout a parse met: the shape objects whose keys led here take, and the edge its latest
/// object went on along, which the next object of the layout is expected to take (V8's
/// expected transition, `JsonParser::ScanJsonPropertyKey`).
struct JsonLayout {
    shape: u32,
    expected: u32,
}

/// A key added to a layout: the bytes of the key's text when it is ASCII and needs no
/// escape (`key_bytes`), which the source is compared with in place of scanning its key, its
/// atom, and the layout the add leads to.
struct JsonLayoutEdge {
    key: Range<u32>,
    atom: Atom,
    to: u32,
}

/// Where an object's keys have led so far: its layout, `JSON_UNPLANNED` once a key repeats;
/// the edge its latest key took; and the context its first key is expected from
/// (`JsonHeapBuilder::first_keys`).
#[derive(Clone, Copy)]
pub(super) struct JsonObjectLayout {
    layout: u32,
    last: u32,
    context: u32,
}

/// The layout of an object whose keys repeat one, which therefore needs a property store
/// per key rather than one shape (`JsonHeapBuilder::object`); and an edge not taken yet.
const JSON_UNPLANNED: u32 = u32::MAX;
/// The layout of an object before its first key.
const JSON_EMPTY_LAYOUT: u32 = 0;
/// The context of an object that is no member's value, nor within one.
const JSON_ROOT_CONTEXT: u32 = 0;

/// Builds heap values as the parse goes. The heap does not collect inside a native call, so
/// values gathered for an array or object stay valid until their container holds them, and
/// the shapes and string values below stay valid for the parse.
pub(super) struct JsonHeapBuilder<'v, H: Host> {
    vm: &'v mut Vm<H>,
    array_prototype: Value,
    object_prototype: Value,
    /// The first property store that failed; the parse finishes and then reports it.
    pub(super) error: Option<JsError>,
    /// What earlier parses learned, taken from the VM for this parse.
    cache: JsonParseCache,
}

/// What JSON parses learn and later parses reuse: the layouts their objects took, their keys'
/// atoms and their strings, as V8 keeps a JSON key list's map transitions. A collection
/// renumbers shapes and frees the values, so these hold only within one collection epoch of
/// one realm; atoms outlive collections, so `ascii_keys` stays.
#[derive(Default)]
pub(crate) struct JsonParseCache {
    /// The collection epoch and object prototype the layouts and strings belong to.
    owner: Option<(u64, Value)>,
    /// The layouts parses met, `JSON_EMPTY_LAYOUT` first, and the keys added to them.
    layouts: Vec<JsonLayout>,
    edges: Vec<JsonLayoutEdge>,
    /// The first edge the latest object of each context took: objects that are the value
    /// of one member, directly or within arrays, mostly share a layout, as V8's parse expects
    /// of an array's elements. The context of an object under a member is one past the
    /// edge of that member's key; `JSON_ROOT_CONTEXT` precedes them.
    first_keys: Vec<u32>,
    /// The edge of each (layout, key) add, for a key other than the one expected.
    edge_index: rustc_hash::FxHashMap<(u32, Atom), u32>,
    /// The ASCII text of each edge's key.
    key_bytes: Vec<u8>,
    /// The atom of each ASCII key parses met outside the expected edges: a JSON text
    /// repeats a few keys, which this hashes a word at a time instead of interning unit by
    /// unit (`intern_unit_run_atom`). Atoms outlive every collection.
    ascii_keys: rustc_hash::FxHashMap<Box<[u8]>, Atom>,
    /// The string value made of each ASCII run of the text parses met, with the run's
    /// bytes in `string_bytes`: a string has no identity, so a run the text repeats takes
    /// the value made once, as V8 internalizes a JSON text's repeated strings. The table
    /// holds copies of the distinct runs, so the builder borrows nothing of the text.
    strings: hashbrown::HashTable<(Range<u32>, Value)>,
    string_bytes: Vec<u8>,
}

/// Bytes of keys and strings past which the cache is dropped rather than kept: a large text
/// (a data literal) would otherwise stay resident through its strings.
const JSON_CACHE_BYTES: usize = 1 << 16;

impl JsonParseCache {
    /// This cache for a parse in `epoch` of the realm of `object_prototype`, whose empty
    /// object takes `empty_shape`: emptied of other epochs' layouts and strings.
    fn prepared(mut self, epoch: u64, object_prototype: Value, empty_shape: u32) -> Self {
        if self.owner == Some((epoch, object_prototype)) {
            return self;
        }
        self.owner = Some((epoch, object_prototype));
        self.layouts.clear();
        self.layouts.push(JsonLayout { shape: empty_shape, expected: JSON_UNPLANNED });
        self.edges.clear();
        self.first_keys.clear();
        self.first_keys.push(JSON_UNPLANNED);
        self.edge_index.clear();
        self.key_bytes.clear();
        self.strings.clear();
        self.string_bytes.clear();
        self
    }

    fn small(&self) -> bool {
        self.key_bytes.len() + self.string_bytes.len() <= JSON_CACHE_BYTES && self.edges.len() <= JSON_CACHE_BYTES / 16
    }
}

pub(super) enum JsonBuildError {
    Parse(JsonParseError),
    Runtime(JsError),
}

/// A hash of ASCII run `bytes` from its length and its first and last words: runs the
/// table holds are compared whole, so the hash need not read every byte.
#[inline(always)]
fn ascii_run_hash(bytes: &[u8]) -> u64 {
    let ends = if bytes.len() < WORD_BYTES {
        short_run_ends(bytes)
    } else {
        let word = |at: usize| u64::from_le_bytes(bytes[at..at + WORD_BYTES].try_into().expect("a word"));
        (word(0), word(bytes.len() - WORD_BYTES))
    };
    rustc_hash::FxBuildHasher.hash_one((bytes.len(), ends))
}

/// The bytes of a run shorter than a word, as two values that together with its length
/// determine it, read without a loop (as wyhash reads short keys): its first and last four
/// bytes, which overlap below eight, else its first, middle and last bytes.
#[inline(always)]
pub(super) fn short_run_ends(bytes: &[u8]) -> (u64, u64) {
    const HALF_WORD: usize = WORD_BYTES / 2;
    let length = bytes.len();
    let half = |at: usize| u64::from(u32::from_le_bytes(bytes[at..at + HALF_WORD].try_into().expect("half a word")));
    if length >= HALF_WORD {
        (half(0), half(length - HALF_WORD))
    } else if length > 0 {
        let byte = |at: usize| u64::from(bytes[at]);
        (byte(0) | byte(length / 2) << u8::BITS, byte(length - 1))
    } else {
        (0, 0)
    }
}

impl<'v, H: Host> JsonHeapBuilder<'v, H> {
    pub(super) fn new(vm: &'v mut Vm<H>) -> Self {
        let globals = vm.realm.globals;
        let object_prototype = vm.realm_object_prototype(globals);
        let empty_shape = Vm::<H>::empty_object(object_prototype).shape();
        Self {
            array_prototype: vm.array_prototype_for_realm(globals),
            object_prototype,
            error: None,
            cache: std::mem::take(&mut vm.json_parse_cache).prepared(vm.heap.collection_epoch(), object_prototype, empty_shape),
            vm,
        }
    }

    pub(super) fn finish(mut self, parsed: Result<Value, JsonParseError>) -> Result<Value, JsonBuildError> {
        let value = parsed.map_err(JsonBuildError::Parse)?;
        match self.error.take() {
            Some(error) => Err(JsonBuildError::Runtime(error)),
            None => Ok(value),
        }
    }

    pub(super) fn materialize(&mut self, value: &JsonValue) -> Result<Value, JsError> {
        let _stack = self.vm.enter_stack()?;
        Ok(match value {
            JsonValue::Null => self.literal(JsonLiteral::Null),
            JsonValue::Bool(true) => self.literal(JsonLiteral::True),
            JsonValue::Bool(false) => self.literal(JsonLiteral::False),
            JsonValue::Number(value) => self.number(*value),
            JsonValue::String(value) => self.vm.heap.alloc_string(value.clone()),
            JsonValue::Source(value, _) => self.materialize(value)?,
            JsonValue::Array(values) => {
                let mut values = values
                    .iter()
                    .map(|value| self.materialize(value))
                    .collect::<Result<Vec<_>, _>>()?;
                self.array(values.drain(..))
            }
            JsonValue::Object(entries) => {
                let mut layout = self.layout(None);
                let mut keys = Vec::with_capacity(entries.len());
                let mut values = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    let key = self.vm.intern_js_atom(key);
                    self.advance(&mut layout, key);
                    keys.push(key);
                    values.push(self.materialize(value)?);
                }
                self.object(layout, keys.drain(..), values.drain(..))
            }
        })
    }

    /// The atom of key `units`.
    fn atom(&mut self, units: Units<'_>) -> Atom {
        let Units::Ascii(bytes) = units else {
            return self.vm.intern_unit_run_atom(units);
        };
        if let Some(&atom) = self.cache.ascii_keys.get(bytes) {
            return atom;
        }
        let atom = self.vm.intern_unit_run_atom(units);
        self.cache.ascii_keys.insert(bytes.into(), atom);
        atom
    }

    /// The edge `object` expects its next key to take.
    #[inline(always)]
    fn expected_edge(&self, object: &JsonObjectLayout) -> u32 {
        match object.layout {
            JSON_EMPTY_LAYOUT => self.cache.first_keys[object.context as usize],
            layout => self.cache.layouts[layout as usize].expected,
        }
    }

    /// Moves `object` on along `edge`, which its layout now expects next.
    #[inline(always)]
    fn take(&mut self, object: &mut JsonObjectLayout, edge: u32) {
        match object.layout {
            JSON_EMPTY_LAYOUT => self.cache.first_keys[object.context as usize] = edge,
            layout => self.cache.layouts[layout as usize].expected = edge,
        }
        object.layout = self.cache.edges[edge as usize].to;
        object.last = edge;
    }

    /// Moves `object` on by an add of `atom`: along the edge this parse made for it, else
    /// along a new one. A key the layout holds already leaves the object unplanned.
    fn advance(&mut self, object: &mut JsonObjectLayout, atom: Atom) {
        let layout = object.layout;
        if layout == JSON_UNPLANNED {
            return;
        }
        if let Some(&edge) = self.cache.edge_index.get(&(layout, atom)) {
            return self.take(object, edge);
        }
        let shape = self.cache.layouts[layout as usize].shape;
        if self.vm.property_shape_slot(shape, PropertyKey::string(atom)).is_some() {
            object.layout = JSON_UNPLANNED;
            return;
        }
        let next = self.vm.transition_shape(shape, atom);
        let to = if self.vm.shape_is_dictionary(next) {
            JSON_UNPLANNED
        } else {
            self.cache.layouts.push(JsonLayout { shape: next, expected: JSON_UNPLANNED });
            (self.cache.layouts.len() - 1) as u32
        };
        // A key that is not plain ASCII has no source text to match: its range is out of
        // bounds.
        let text = self.vm.atom_name(atom);
        let key = if text.bytes().all(|byte| byte.is_ascii() && !STRING_RUN_ENDS[usize::from(byte)]) {
            let start = self.cache.key_bytes.len() as u32;
            self.cache.key_bytes.extend_from_slice(text.as_bytes());
            start..self.cache.key_bytes.len() as u32
        } else {
            JSON_UNPLANNED..JSON_UNPLANNED
        };
        self.cache.edges.push(JsonLayoutEdge { key, atom, to });
        self.cache.first_keys.push(JSON_UNPLANNED);
        let edge = (self.cache.edges.len() - 1) as u32;
        self.cache.edge_index.insert((layout, atom), edge);
        self.take(object, edge);
    }

    /// An object of keys one repeats: each key stored in turn, as CreateDataProperty does,
    /// so a repeated key keeps its first position and takes its last value.
    #[cold]
    fn unplanned_object(&mut self, keys: std::vec::Drain<'_, Atom>, values: std::vec::Drain<'_, Value>) -> Value {
        let object = self
            .vm
            .heap
            .alloc(Cell::Object(Vm::<H>::empty_object(self.object_prototype)));
        for (atom, value) in keys.zip(values) {
            let key = PropertyKey::string(atom);
            let stored = match self.vm.object_data(object).map(|data| data.shape()) {
                Some(shape) if self.vm.property_shape_slot(shape, key).is_none() => {
                    self.vm.create_shape_data_property(object, key, value, DEFAULT_DATA_ATTRIBUTES)
                }
                _ => self.vm.set_property(object, atom, value),
            };
            if let Err(error) = stored {
                self.error.get_or_insert(error);
            }
        }
        object
    }

    /// The string value of ASCII run `bytes`: the one made for the same run before.
    fn ascii_string(&mut self, bytes: &[u8]) -> Value {
        let hash = ascii_run_hash(bytes);
        let text = |range: &Range<u32>| &self.cache.string_bytes[range.start as usize..range.end as usize];
        if let Some((_, value)) = self.cache.strings.find(hash, |(range, _)| same_ascii(text(range), bytes)) {
            return *value;
        }
        let value = self.vm.heap.alloc_string(JsString::from_unit_run(Units::Ascii(bytes)));
        let start = self.cache.string_bytes.len() as u32;
        self.cache.string_bytes.extend_from_slice(bytes);
        let range = start..self.cache.string_bytes.len() as u32;
        let string_bytes = &self.cache.string_bytes;
        self.cache.strings.insert_unique(hash, (range, value), |(range, _)| {
            ascii_run_hash(&string_bytes[range.start as usize..range.end as usize])
        });
        value
    }
}

/// The cache goes back to the VM for the next parse unless this parse grew it large.
impl<H: Host> Drop for JsonHeapBuilder<'_, H> {
    fn drop(&mut self) {
        if self.cache.small() {
            self.vm.json_parse_cache = std::mem::take(&mut self.cache);
        }
    }
}

impl<H: Host> JsonBuilder for JsonHeapBuilder<'_, H> {
    type Value = Value;
    type Key = Atom;
    type Layout = JsonObjectLayout;
    const KEEPS_SOURCE: bool = false;
    fn literal(&mut self, value: JsonLiteral) -> Value {
        match value {
            JsonLiteral::Null => Value::NULL,
            JsonLiteral::True => Value::TRUE,
            JsonLiteral::False => Value::FALSE,
        }
    }
    fn number(&mut self, value: f64) -> Value {
        Value::number(value)
    }
    fn integer(&mut self, value: i32) -> Value {
        Value::integer(value)
    }
    #[inline(always)]
    fn string(&mut self, units: JsonStringUnits<'_>) -> Value {
        match units {
            JsonStringUnits::Run(Units::Ascii(bytes)) => self.ascii_string(bytes),
            units => self.vm.heap.alloc_string(JsString::from_unit_run(units.units())),
        }
    }
    fn layout(&mut self, parent: Option<&JsonObjectLayout>) -> JsonObjectLayout {
        let context = match parent {
            Some(parent) if parent.last != JSON_UNPLANNED => parent.last + 1,
            _ => JSON_ROOT_CONTEXT,
        };
        JsonObjectLayout {
            layout: JSON_EMPTY_LAYOUT,
            last: JSON_UNPLANNED,
            context,
        }
    }
    #[inline(always)]
    fn expected_key(&mut self, object: &mut JsonObjectLayout, matches: impl Fn(&[u8]) -> bool) -> Option<(Atom, usize)> {
        if object.layout == JSON_UNPLANNED {
            return None;
        }
        let edge = self.expected_edge(object);
        let entry = self.cache.edges.get(edge as usize)?;
        let key = self.cache.key_bytes.get(entry.key.start as usize..entry.key.end as usize)?;
        if !matches(key) {
            return None;
        }
        let found = (entry.atom, key.len());
        self.take(object, edge);
        Some(found)
    }
    fn key(&mut self, object: &mut JsonObjectLayout, units: Units<'_>) -> Atom {
        let atom = self.atom(units);
        self.advance(object, atom);
        atom
    }
    fn array(&mut self, values: std::vec::Drain<'_, Value>) -> Value {
        let values = values.as_slice();
        self.vm
            .heap
            .alloc_literal_array(self.array_prototype, values.len(), |index| values[index])
    }
    fn object(
        &mut self,
        object: JsonObjectLayout,
        keys: std::vec::Drain<'_, Atom>,
        values: std::vec::Drain<'_, Value>,
    ) -> Value {
        if object.layout == JSON_UNPLANNED {
            return self.unplanned_object(keys, values);
        }
        let shape = self.cache.layouts[object.layout as usize].shape;
        self.vm
            .heap
            .alloc_object_with_properties(self.object_prototype, shape, values.as_slice())
    }
    fn primitive(&mut self, value: Value, _source: Units<'_>) -> Value {
        value
    }
}

