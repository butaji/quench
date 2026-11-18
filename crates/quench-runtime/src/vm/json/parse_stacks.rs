//! The stacks a JSON parse gathers containers' members on, which a builder that parses
//! again lends each parse, so a parse allocates none of its own.

/// A container whose members a parse is gathering: where its members start on the parse's
/// stacks, and an object's layout so far.
pub(super) enum JsonFrame<L> {
    Array { values: usize },
    Object { values: usize, keys: usize, layout: L },
}

/// The members a fresh parse's stacks start with room for, which most texts' containers fit.
const JSON_STACK_START: usize = 16;

/// A parse's stacks (`JsonParser`): the values and keys of the containers it is gathering,
/// and the containers themselves.
pub(super) struct JsonStacks<V, K, L> {
    pub(super) values: Vec<V>,
    pub(super) keys: Vec<K>,
    pub(super) frames: Vec<JsonFrame<L>>,
}

impl<V, K, L> JsonStacks<V, K, L> {
    /// Stacks of one parse, with room for `JSON_STACK_START` members.
    pub(super) fn with_room() -> Self {
        Self {
            values: Vec::with_capacity(JSON_STACK_START),
            keys: Vec::with_capacity(JSON_STACK_START),
            frames: Vec::with_capacity(JSON_STACK_START),
        }
    }

    /// Empties what a parse that stopped at an error left, keeping the storage.
    pub(super) fn clear(&mut self) {
        self.values.clear();
        self.keys.clear();
        self.frames.clear();
    }
}

/// Stacks with no storage yet, which taking them out of a builder leaves behind.
impl<V, K, L> Default for JsonStacks<V, K, L> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            keys: Vec::new(),
            frames: Vec::new(),
        }
    }
}
