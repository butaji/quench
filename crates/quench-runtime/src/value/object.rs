//! JavaScript objects with prototype chain support.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;
use regress::Regex;
use rustc_hash::FxBuildHasher;

use crate::ast::Statement;
use crate::env::Environment;
use crate::value::function::ValueFunction;
use crate::value::kind::{ExoticKind, ObjectKind};
use crate::value::{Symbol, Value};

/// Runtime property key — canonicalizes array indices to `Idx(u32)`.
/// Also used as the key type for `Object.props` (currently separate maps,
/// gradually migrating to a single `IndexMap<Key, Desc>` per TComp model).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// Non-index string key (TComp: Rc<str> for cheap cloning)
    Str(Rc<str>),
    /// Canonical array index (0 ..= 4294967294)
    Idx(u32),
    /// Symbol key (per spec 6.1.7.1, Sym(Rc<Symbol>))
    Sym(Rc<Symbol>),
}

impl From<&str> for Key {
    fn from(s: &str) -> Self {
        as_key(s)
    }
}

/// Convert a string property key to a `Key`, canonicalizing array indices.
pub fn as_key(s: &str) -> Key {
    // Fast path: single digit
    if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() {
        return Key::Idx((s.as_bytes()[0] - b'0') as u32);
    }
    if let Ok(n) = s.parse::<u32>() {
        if n <= 4294967294 {
            return Key::Idx(n);
        }
    }
    Key::Str(Rc::from(s))
}

/// Returns `true` if `s` is a canonical array index string ("0" through "4294967294").
pub fn is_array_index(s: &str) -> bool {
    matches!(as_key(s), Key::Idx(_))
}

/// Maximum number of dense array elements. Indices at or above this are
/// stored as plain properties instead of growing the elements Vec, so a
/// single `o["1000000000"] = 1` cannot allocate a billion-element Vec.
pub const MAX_ARRAY_ELEMENTS: usize = 1 << 20;

/// Parse a property key as an array index only if it is the canonical form:
/// `"01"` or `"1e2"` are plain string keys, not indices. Also rejects
/// indices at or above MAX_ARRAY_ELEMENTS.
fn as_array_index(key: &str) -> Option<usize> {
    let idx = key.parse::<usize>().ok()?;
    if idx < MAX_ARRAY_ELEMENTS && key == idx.to_string() {
        Some(idx)
    } else {
        None
    }
}

/// Promise state for Promise objects
#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

/// Promise-specific data stored in Promise objects
#[derive(Debug, Clone)]
pub struct PromiseObjectData {
    pub state: PromiseState,
    pub result: Value,
    pub on_fulfilled_callbacks: Vec<Value>,
    pub on_rejected_callbacks: Vec<Value>,
}

impl Default for PromiseObjectData {
    fn default() -> Self {
        Self::new()
    }
}

/// Exotic-specific typed state — replaces `ObjectKind` + scattered fields.
/// Every Object has exactly one `ObjData` variant.
#[derive(Debug, Clone)]
pub enum ObjData {
    Ordinary,
    /// Array exotic (9.4.2): length stored in `props["length"]`
    Array,
    /// String exotic (9.4.3): [[StringData]]
    String(Rc<str>),
    /// Function object (9.2, 9.3, 9.4.1)
    Func,
    /// Proxy exotic (9.5)
    Proxy {
        target: Rc<RefCell<Object>>,
        handler: Rc<RefCell<Object>>,
    },
    /// Arguments exotic (9.4.4): sloppy-mode mapped arguments
    Args {
        mapped: std::collections::HashMap<u32, String>,
    },
    /// Integer-Indexed exotic (9.4.5): TypedArray
    Idx {
        buffer: Rc<RefCell<Object>>,
        offset: u64,
        length: u64,
        name: TypedArrayName,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TypedArrayName {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
}

/// [[ThisMode]] for function objects (ECMA-262 9.2.1)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThisMode {
    Lexical, // arrow functions
    Strict,  // class methods, strict functions
    Global,  // sloppy functions
}

/// TComp: ECMA-262 6.2.5 PropertyDescriptor — minimal, exact.
/// All fields are `Option` so `IsDataDescriptor` / `IsAccessorDescriptor`
/// can distinguish "not present" from `false`.
#[derive(Debug, Clone, Default)]
pub struct Desc {
    pub value: Option<Value>,
    pub writable: Option<bool>,
    pub get: Option<Value>,
    pub set: Option<Value>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}

impl Desc {
    pub fn is_data(&self) -> bool {
        self.value.is_some() || self.writable.is_some()
    }
    pub fn is_accessor(&self) -> bool {
        self.get.is_some() || self.set.is_some()
    }
    pub fn is_generic(&self) -> bool {
        !self.is_data() && !self.is_accessor()
    }
}

/// Store pointer type for getter/setter AST bodies (needed during eval
/// before the function value is resolved). Kept separate from `Desc`.
#[derive(Debug, Clone)]
pub struct GetterBody {
    pub body: Rc<Vec<Statement>>,
    pub closure: Rc<RefCell<Environment>>,
}

#[derive(Debug, Clone)]
pub struct SetterBody {
    pub param: String,
    pub body: Rc<Vec<Statement>>,
    pub closure: Rc<RefCell<Environment>>,
}

impl PromiseObjectData {
    pub fn new() -> Self {
        PromiseObjectData {
            state: PromiseState::Pending,
            result: Value::Undefined,
            on_fulfilled_callbacks: Vec::new(),
            on_rejected_callbacks: Vec::new(),
        }
    }

    pub fn fulfill(&mut self, value: Value) {
        self.state = PromiseState::Fulfilled;
        self.result = value;
    }

    pub fn reject(&mut self, reason: Value) {
        self.state = PromiseState::Rejected;
        self.result = reason;
    }

    pub fn add_fulfilled_callback(&mut self, callback: Value) {
        self.on_fulfilled_callbacks.push(callback);
    }

    pub fn add_rejected_callback(&mut self, callback: Value) {
        self.on_rejected_callbacks.push(callback);
    }
}

/// Getter function representation - stores closure and body for lazy evaluation
#[derive(Debug, Clone)]
pub struct Getter {
    pub closure: Rc<RefCell<Environment>>,
    pub body: Vec<Statement>,
}

/// Getter storage in object - stores body and closure for proper scope capture
#[derive(Debug, Clone)]
pub struct GetterStorage {
    pub body: std::rc::Rc<Vec<Statement>>,
    /// Closure environment at the time the getter was created
    pub closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    /// Function value when the getter was installed via
    /// `Object.defineProperty` (takes precedence over body/closure and
    /// preserves function identity for descriptors).
    pub func: Option<Value>,
}

/// Setter storage in object
#[derive(Debug, Clone)]
pub struct SetterStorage {
    pub param: String,
    pub body: std::rc::Rc<Vec<Statement>>,
    /// Closure environment at the time the object was created
    pub closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    /// Function value when installed via `Object.defineProperty`.
    pub func: Option<Value>,
}

/// Setter function representation
#[derive(Debug, Clone)]
pub struct Setter {
    pub closure: Rc<RefCell<Environment>>,
    pub param: String,
    pub body: Vec<Statement>,
}

/// Property descriptor flags per ECMAScript spec
#[derive(Debug, Clone, Default)]
pub struct PropertyFlags {
    pub value: Option<Value>,
    pub writable: bool,
    pub enumerable: bool,
    pub configurable: bool,
}

impl PropertyFlags {
    /// Default flags for a normal property
    pub fn default_data() -> Self {
        PropertyFlags {
            value: None,
            writable: true,
            enumerable: true,
            configurable: true,
        }
    }

    /// Default flags for accessor property
    pub fn default_accessor() -> Self {
        PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: true,
        }
    }
}

/// ECMA-262 6.2.5 PropertyDescriptor — unified representation of a property.
/// All fields are Option so we can distinguish "not present" vs "false"
/// when the spec checks IsDataDescriptor / IsAccessorDescriptor.
#[derive(Debug, Clone, Default)]
pub struct PropertyDescriptor {
    pub value: Option<Value>,
    pub writable: Option<bool>,
    pub get: Option<Value>,
    pub set: Option<Value>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
    // AST-level getter/setter storage (class literal accessors, not defineProperty)
    pub get_body: Option<Rc<Vec<Statement>>>,
    pub get_closure: Option<Rc<RefCell<Environment>>>,
    pub set_body: Option<Rc<Vec<Statement>>>,
    pub set_closure: Option<Rc<RefCell<Environment>>>,
    pub set_param: Option<String>,
}

impl PropertyDescriptor {
    /// IsDataDescriptor (ES 6.2.6): has [[Value]] or [[Writable]]
    pub fn is_data(&self) -> bool {
        self.value.is_some() || self.writable.is_some()
    }

    /// IsAccessorDescriptor (ES 6.2.6): has [[Get]] or [[Set]]
    pub fn is_accessor(&self) -> bool {
        self.get.is_some()
            || self.set.is_some()
            || self.get_body.is_some()
            || self.set_body.is_some()
    }
}

/// 11 internal methods + 2 function extras — the spec's object interface.
/// Methods take `Key` (not `&str`) for array-index-canonicalized dispatch.
pub struct VTable {
    pub get_prototype_of: fn(&Object) -> Option<Rc<RefCell<Object>>>,
    pub set_prototype_of: fn(&mut Object, Option<Rc<RefCell<Object>>>) -> bool,
    pub is_extensible: fn(&Object) -> bool,
    pub prevent_extensions: fn(&mut Object) -> bool,
    pub get_own_property: fn(&Object, &Key) -> Option<Desc>,
    pub define_own_property: fn(&mut Object, &Key, &Desc) -> bool,
    pub has_property: fn(&Object, &Key) -> bool,
    pub get: fn(&Object, &Key, Value) -> Value,
    pub set: fn(&mut Object, &Key, Value, Value) -> bool,
    pub delete: fn(&mut Object, &Key) -> bool,
    pub own_property_keys: fn(&Object) -> Vec<Key>,
    pub call: Option<fn(&Object, Value, Vec<Value>) -> Result<Value, crate::value::JsError>>,
    pub construct: Option<fn(&Object, Vec<Value>, Value) -> Result<Value, crate::value::JsError>>,
}

/// Runtime internal slots storage — replaces scattered fields like
/// promise_data, internal_regex, exotic_kind, etc.
pub type Slots = rustc_hash::FxHashMap<&'static str, Value>;

/// JavaScript object with prototype chain support.
#[derive(Clone)]
pub struct Object {
    /// Own properties of the object (insertion-ordered)
    pub properties: IndexMap<String, Value>,
    /// Array elements (for dense arrays)
    pub elements: Vec<Value>,
    /// Kind of object for special behavior
    pub kind: ObjectKind,
    /// Prototype object for inheritance chain (or null for end of chain)
    pub prototype: Option<Rc<RefCell<Object>>>,
    /// Getter functions for properties (stores body for later evaluation)
    getters: IndexMap<String, GetterStorage>,
    /// Setter functions for properties
    setters: IndexMap<String, SetterStorage>,
    /// Property descriptor flags (for defineProperty support)
    descriptors: IndexMap<String, PropertyFlags>,
    /// Promise-specific data (only for Promise objects)
    pub promise_data: Option<PromiseObjectData>,
    /// Internal regex (for RegExp objects)
    pub internal_regex: Option<Regex>,
    /// Internal regex source string
    pub internal_regex_source: Option<String>,
    /// Internal regex flags string
    pub internal_regex_flags: Option<String>,
    /// Exotic kind for boxed primitives (String, Number, Boolean objects)
    pub exotic_kind: Option<ExoticKind>,
    /// Symbol-keyed properties (stored separately from string-keyed)
    pub symbol_properties: IndexMap<String, Value>,
    /// Array holes: indices that were elided (e.g., [,] has hole at index 0).
    pub holes: HashSet<usize>,
    /// Whether new properties can be added (false after Object.preventExtensions).
    /// Object.freeze also sets this to false.
    pub extensible: bool,
    /// TComp: unified property map — replaces properties/elements/getters/setters/descriptors
    pub props: IndexMap<Key, Desc, FxBuildHasher>,
    /// TComp: internal slots for ArrayLength, PromiseData, ProxyTarget, etc.
    pub slots: Slots,
    /// TComp: exotic-specific state
    pub data: ObjData,
    /// TComp: vtable for exotic behavior dispatch (ES 9.1, 9.4, 9.5, 10.x)
    pub vtable: &'static VTable,
}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // properties may contain Value::Object pointing to self — avoid infinite recursion
        f.debug_struct("Object")
            .field("kind", &self.kind)
            .field("properties", &self.properties.keys().collect::<Vec<_>>())
            .field("elements_len", &self.elements.len())
            .finish()
    }
}

// ─── Ordinary VTable implementations (TComp: operate on Object.props, Key) ─

fn ordinary_get_prototype_of(obj: &Object) -> Option<Rc<RefCell<Object>>> {
    obj.prototype.clone()
}

fn ordinary_set_prototype_of(obj: &mut Object, proto: Option<Rc<RefCell<Object>>>) -> bool {
    if !obj.extensible && obj.prototype.is_some() {
        return false;
    }
    obj.prototype = proto;
    true
}

fn ordinary_is_extensible(obj: &Object) -> bool {
    obj.extensible
}

fn ordinary_prevent_extensions(obj: &mut Object) -> bool {
    obj.extensible = false;
    true
}

fn ordinary_get_own_property(obj: &Object, key: &Key) -> Option<Desc> {
    // Primary: check props
    if let Some(desc) = obj.props.get(key) {
        return Some(desc.clone());
    }
    // Fallback: old maps (during migration)
    let key_str = match key {
        Key::Str(s) => s.as_ref(),
        Key::Idx(i) => return None, // array indices not in old maps as strings
        Key::Sym(s) => return None, // symbol keys not in old maps as strings
    };
    let flags = obj.descriptors.get(key_str).cloned().unwrap_or_default();
    if let Some(val) = obj.properties.get(key_str) {
        return Some(Desc {
            value: Some(val.clone()),
            writable: Some(flags.writable),
            enumerable: Some(flags.enumerable),
            configurable: Some(flags.configurable),
            ..Default::default()
        });
    }
    if let Some(g) = obj.getters.get(key_str) {
        return Some(Desc {
            get: g.func.clone(),
            enumerable: Some(flags.enumerable),
            configurable: Some(flags.configurable),
            ..Default::default()
        });
    }
    if let Some(s) = obj.setters.get(key_str) {
        return Some(Desc {
            set: s.func.clone(),
            enumerable: Some(flags.enumerable),
            configurable: Some(flags.configurable),
            ..Default::default()
        });
    }
    None
}

fn ordinary_define_own_property(obj: &mut Object, key: &Key, desc: &Desc) -> bool {
    let key_str = match key {
        Key::Str(s) => Some(s.as_ref()),
        _ => None,
    };
    // Check extensible
    if !obj.extensible && !obj.props.contains_key(key) {
        return false;
    }
    if desc.is_data() {
        let val = desc.value.clone().unwrap_or(Value::Undefined);
        obj.props.insert(key.clone(), desc.clone());
        // Backward compat with old maps
        if let Some(ks) = key_str {
            obj.properties.insert(ks.to_string(), val);
            let flags = PropertyFlags {
                value: desc.value.clone(),
                writable: desc.writable.unwrap_or(false),
                enumerable: desc.enumerable.unwrap_or(false),
                configurable: desc.configurable.unwrap_or(false),
            };
            obj.descriptors.insert(ks.to_string(), flags);
            obj.getters.shift_remove(ks);
            obj.setters.shift_remove(ks);
        }
        true
    } else if desc.is_accessor() {
        obj.props.insert(key.clone(), desc.clone());
        if let Some(ks) = key_str {
            let flags = PropertyFlags {
                value: None,
                writable: false,
                enumerable: desc.enumerable.unwrap_or(true),
                configurable: desc.configurable.unwrap_or(true),
            };
            obj.descriptors.insert(ks.to_string(), flags);
            if let Some(ref g) = desc.get {
                obj.set_getter_func(ks, g.clone());
            }
            if let Some(ref s) = desc.set {
                obj.set_setter_func(ks, s.clone());
            }
            obj.properties.shift_remove(ks);
        }
        true
    } else {
        // Generic — update flags only
        if let Some(entry) = obj.props.get_mut(key) {
            if let Some(e) = desc.enumerable {
                entry.enumerable = Some(e);
            }
            if let Some(c) = desc.configurable {
                entry.configurable = Some(c);
            }
        }
        true
    }
}

fn ordinary_has_property(obj: &Object, key: &Key) -> bool {
    obj.props.contains_key(key)
        || match key {
            Key::Str(s) => {
                obj.properties.contains_key(s.as_ref())
                    || obj.getters.contains_key(s.as_ref())
                    || obj.setters.contains_key(s.as_ref())
            }
            _ => false,
        }
}

fn ordinary_get(obj: &Object, key: &Key, _receiver: Value) -> Value {
    if let Some(desc) = obj.props.get(key) {
        if let Some(ref val) = desc.value {
            return val.clone();
        }
        if let Some(ref get_func) = desc.get {
            return get_func.clone();
        }
    }
    // Fall back to old get via string key
    let key_str = match key {
        Key::Str(s) => s.as_ref(),
        Key::Idx(i) => return obj.get(&i.to_string()).unwrap_or(Value::Undefined),
        Key::Sym(s) => {
            return if let Some(ref d) = s.desc {
                obj.get(d).unwrap_or(Value::Undefined)
            } else {
                Value::Undefined
            }
        }
    };
    obj.get(key_str).unwrap_or(Value::Undefined)
}

fn ordinary_set(obj: &mut Object, key: &Key, value: Value, _receiver: Value) -> bool {
    if !obj.extensible {
        return false;
    }
    if let Some(desc) = obj.props.get(key) {
        if desc.set.is_some() || desc.get.is_some() {
            return false; // accessor — caller handles
        }
        if desc.writable == Some(false) {
            return false;
        }
    }
    obj.props.insert(
        key.clone(),
        Desc {
            value: Some(value.clone()),
            writable: Some(true),
            enumerable: Some(true),
            configurable: Some(true),
            ..Default::default()
        },
    );
    // Backward compat with old maps
    let key_str = match key {
        Key::Str(s) => s.as_ref(),
        Key::Idx(i) => {
            let s = i.to_string();
            obj.set(&s, value);
            return true;
        }
        Key::Sym(_) => return true,
    };
    obj.set(key_str, value);
    true
}

fn ordinary_delete(obj: &mut Object, key: &Key) -> bool {
    obj.props.shift_remove(key);
    if let Key::Str(s) = key {
        obj.properties.shift_remove(s.as_ref());
        obj.descriptors.shift_remove(s.as_ref());
        obj.getters.shift_remove(s.as_ref());
        obj.setters.shift_remove(s.as_ref());
    } else if let Key::Idx(i) = key {
        let s = i.to_string();
        obj.properties.shift_remove(&s);
        if (*i as usize) < obj.elements.len() {
            obj.elements[*i as usize] = Value::Undefined;
            obj.holes.insert(*i as usize);
        }
    }
    true
}

fn ordinary_own_property_keys(obj: &Object) -> Vec<Key> {
    let mut keys: Vec<Key> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for (k, _) in &obj.props {
        match k {
            Key::Idx(i) => indices.push(*i),
            other => keys.push(other.clone()),
        }
    }
    indices.sort_unstable();
    let mut result: Vec<Key> = indices.into_iter().map(Key::Idx).collect();
    result.extend(keys);
    result
}

/// VTable for ordinary (non-exotic) objects.
pub static ORDINARY_VTABLE: VTable = VTable {
    get_prototype_of: ordinary_get_prototype_of,
    set_prototype_of: ordinary_set_prototype_of,
    is_extensible: ordinary_is_extensible,
    prevent_extensions: ordinary_prevent_extensions,
    get_own_property: ordinary_get_own_property,
    define_own_property: ordinary_define_own_property,
    has_property: ordinary_has_property,
    get: ordinary_get,
    set: ordinary_set,
    delete: ordinary_delete,
    own_property_keys: ordinary_own_property_keys,
    call: None,
    construct: None,
};

// ─── Array VTable (ES 9.4.2) ─────────────────────────────────────────

/// Array exotic [[DefineOwnProperty]] (9.4.2.1).
fn array_define_own_property(obj: &mut Object, key: &Key, desc: &Desc) -> bool {
    if key == &as_key("length") {
        return array_set_length(obj, desc);
    }
    // Array index: if index >= length, auto-extend
    if let Key::Idx(index) = key {
        let current_length = array_length_value(obj) as u32;
        if *index >= current_length && *index < 4294967295 {
            let new_len_val = Value::Number((*index + 1) as f64);
            obj.props.insert(
                as_key("length"),
                Desc {
                    value: Some(new_len_val),
                    writable: Some(true),
                    enumerable: Some(false),
                    configurable: Some(false),
                    ..Default::default()
                },
            );
            obj.properties
                .insert("length".to_string(), Value::Number((*index + 1) as f64));
            let needed = (*index + 1) as usize;
            if obj.elements.len() < needed {
                obj.elements.resize(needed, Value::Undefined);
            }
            obj.elements[*index as usize] = desc.value.clone().unwrap_or(Value::Undefined);
        }
    }
    ordinary_define_own_property(obj, key, desc)
}

/// Get the numeric length from an array object.
fn array_length_value(obj: &Object) -> f64 {
    if let Some(desc) = obj.props.get(&as_key("length")) {
        if let Some(Value::Number(n)) = desc.value {
            return n;
        }
    }
    obj.properties
        .get("length")
        .and_then(|v| match v {
            Value::Number(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0.0)
}

/// ArraySetLength (9.4.2.4).
fn array_set_length(obj: &mut Object, desc: &Desc) -> bool {
    let new_len = match &desc.value {
        Some(Value::Number(n)) => *n as u32,
        Some(_) => {
            return false;
        }
        None => return true,
    };
    let old_len = array_length_value(obj) as u32;
    if new_len < old_len {
        for i in new_len..old_len {
            obj.props.shift_remove(&Key::Idx(i));
        }
        if new_len as usize <= obj.elements.len() {
            obj.elements.truncate(new_len as usize);
        }
    }
    let len_val = Value::Number(new_len as f64);
    obj.props.insert(
        as_key("length"),
        Desc {
            value: Some(len_val.clone()),
            writable: Some(true),
            enumerable: Some(false),
            configurable: Some(false),
            ..Default::default()
        },
    );
    obj.properties.insert("length".to_string(), len_val);
    true
}

/// VTable for Array exotic objects — overrides only define_own_property.
pub static ARRAY_VTABLE: VTable = VTable {
    define_own_property: array_define_own_property,
    ..ORDINARY_VTABLE
};

impl Object {
    /// Create a new ordinary object with no prototype
    pub fn new(kind: ObjectKind) -> Self {
        // Determine ObjData from ObjectKind
        let data = match kind {
            ObjectKind::Array => ObjData::Array,
            _ => ObjData::Ordinary,
        };
        Object {
            properties: IndexMap::new(),
            elements: Vec::new(),
            kind,
            prototype: None,
            getters: IndexMap::new(),
            setters: IndexMap::new(),
            descriptors: IndexMap::new(),
            promise_data: None,
            internal_regex: None,
            internal_regex_source: None,
            internal_regex_flags: None,
            exotic_kind: None,
            symbol_properties: IndexMap::new(),
            holes: HashSet::new(),
            extensible: true,
            slots: rustc_hash::FxHashMap::default(),
            props: IndexMap::with_hasher(FxBuildHasher),
            data,
            vtable: &ORDINARY_VTABLE,
        }
    }

    /// Create a new object with a specific prototype
    pub fn with_prototype(kind: ObjectKind, prototype: Rc<RefCell<Object>>) -> Self {
        let data = match kind {
            ObjectKind::Array => ObjData::Array,
            _ => ObjData::Ordinary,
        };
        Object {
            properties: IndexMap::new(),
            elements: Vec::new(),
            kind,
            prototype: Some(prototype),
            getters: IndexMap::new(),
            setters: IndexMap::new(),
            descriptors: IndexMap::new(),
            promise_data: None,
            internal_regex: None,
            internal_regex_source: None,
            internal_regex_flags: None,
            exotic_kind: None,
            symbol_properties: IndexMap::new(),
            holes: HashSet::new(),
            extensible: true,
            slots: rustc_hash::FxHashMap::default(),
            props: IndexMap::with_hasher(FxBuildHasher),
            data,
            vtable: &ORDINARY_VTABLE,
        }
    }

    /// Create a new array object
    pub fn new_array(len: usize) -> Self {
        let mut obj = Object::new(ObjectKind::Array);
        // Defensive cap: callers that want a RangeError for huge lengths
        // should use new_array_checked; never allocate unbounded memory here.
        let len = len.min(MAX_ARRAY_ELEMENTS);
        obj.elements = vec![Value::Undefined; len];
        let len_val = Value::Number(len as f64);
        obj.properties.insert("length".to_string(), len_val.clone());
        obj.props.insert(
            as_key("length"),
            Desc {
                value: Some(len_val),
                writable: Some(true),
                enumerable: Some(false),
                configurable: Some(false),
                ..Default::default()
            },
        );
        if let Some(proto) = crate::builtins::get_array_prototype() {
            obj.prototype = Some(proto);
        }
        obj.vtable = &ARRAY_VTABLE;
        obj
    }

    /// Create a new array object, rejecting lengths above MAX_ARRAY_ELEMENTS
    /// with a RangeError (the `new Array(n)` path should prefer this).
    pub fn new_array_checked(len: usize) -> Result<Self, crate::value::error::JsError> {
        if len > MAX_ARRAY_ELEMENTS {
            return Err(crate::value::error::JsError::new(
                "RangeError: invalid array length",
            ));
        }
        Ok(Self::new_array(len))
    }

    /// Get a property value, including prototype chain lookup.
    /// Simple recursion: drops each Ref before recursing, so no RefCell conflict.
    pub fn get(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.get_own(key) {
            return Some(v);
        }
        let proto = self.prototype.clone();
        proto.and_then(|p| {
            let r = p.borrow();
            r.get(key)
        })
    }

    /// Get own property only (no prototype chain)
    fn get_own(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.properties.get(key) {
            return Some(v.clone());
        }
        if let Some(idx) = as_array_index(key) {
            if idx < self.elements.len() {
                return Some(self.elements[idx].clone());
            }
        }
        None
    }

    /// Get a property by Value key (for Symbol keys).
    /// Searches own properties only, does not follow prototype chain.
    pub fn get_property(&self, key: &Value) -> Option<Value> {
        if let Value::Symbol(sym) = key {
            return self
                .symbol_properties
                .get(sym.desc.as_deref().unwrap_or(""))
                .cloned();
        }
        None
    }

    /// Set a Symbol-keyed property.
    pub fn set_symbol(&mut self, key: &str, value: Value) {
        // Check if property is non-writable via descriptors
        if let Some(flags) = self.descriptors.get(key) {
            if !flags.writable {
                return;
            }
        } else {
            // Ensure descriptor entry exists for sync
            self.descriptors.insert(
                key.to_string(),
                PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: true,
                },
            );
        }
        self.symbol_properties.insert(key.to_string(), value);
    }

    /// Check if object has a Symbol-keyed property.
    pub fn has_symbol(&self, key: &Value) -> bool {
        if let Value::Symbol(sym) = key {
            return self
                .symbol_properties
                .contains_key(sym.desc.as_deref().unwrap_or(""));
        }
        false
    }

    /// Set a Symbol-keyed property using the full Value::Symbol.
    pub fn set_symbol_value(&mut self, value: Value) {
        if let Value::Symbol(sym_key) = &value {
            let key = sym_key
                .desc
                .clone()
                .map(|d| d.to_string())
                .unwrap_or_default();
            if let Some(flags) = self.descriptors.get(&key) {
                if !flags.writable {
                    return;
                }
            } else {
                self.descriptors.insert(
                    key.clone(),
                    PropertyFlags {
                        value: None,
                        writable: true,
                        enumerable: true,
                        configurable: true,
                    },
                );
            }
            self.symbol_properties.insert(key, value);
        }
    }

    /// Set a property value on this object only (no prototype chain).
    /// Respects writable flag from property descriptor.
    /// Always creates a matching descriptor entry so properties and
    /// descriptors stay in sync (fixes getOwnPropertyDescriptor for
    /// properties created via simple assignment).
    pub fn set(&mut self, key: &str, value: Value) {
        if let Some(flags) = self.descriptors.get_mut(key) {
            if !flags.writable {
                return;
            }
            flags.value = Some(value.clone());
        } else {
            // Ensure a default descriptor entry exists so every property
            // has an associated PropertyFlags entry.
            self.descriptors.insert(
                key.to_string(),
                PropertyFlags {
                    value: Some(value.clone()),
                    writable: true,
                    enumerable: true,
                    configurable: true,
                },
            );
        }

        if let Some(idx) = as_array_index(key) {
            while self.elements.len() <= idx {
                self.elements.push(Value::Undefined);
            }
            self.elements[idx] = value;
            // Removing a hole when setting a value
            self.holes.remove(&idx);
            self.properties.insert(
                "length".to_string(),
                Value::Number(self.elements.len() as f64),
            );
        } else {
            // Non-canonical numeric keys ("01") and indices at or above
            // MAX_ARRAY_ELEMENTS are stored as plain properties, so they
            // neither alias elements nor grow the Vec unboundedly.
            self.properties.insert(key.to_string(), value);
        }
    }

    /// Set a property on a function stored in this object's properties.
    /// Returns true if the property was set on a function.
    pub fn set_function_property(&mut self, key: &str, prop: &str, value: Value) -> bool {
        if let Some(existing) = self.properties.get_mut(key) {
            match existing {
                Value::Function(ref f) => {
                    f.set_property(prop, value);
                    return true;
                }
                Value::NativeFunction(ref nf) => {
                    nf.set_property(prop, value);
                    return true;
                }
                _ => return false,
            }
        }
        false
    }

    /// Get mutable access to a function property for in-place modification.
    /// Returns the function and its key for the closure pattern.
    pub fn get_function_mut(&mut self, key: &str) -> Option<&mut ValueFunction> {
        self.properties.get_mut(key).and_then(|v| match v {
            Value::Function(ref mut f) => Some(f),
            _ => None,
        })
    }

    /// Define a property with explicit descriptor flags
    pub fn define(&mut self, key: &str, value: Value, flags: PropertyFlags) {
        // Remove existing getter/setter if redefining as data property
        if flags.value.is_some() || !self.getters.contains_key(key) {
            self.getters.shift_remove(key);
            self.setters.shift_remove(key);
        }
        self.properties.insert(key.to_string(), value);
        self.descriptors.insert(key.to_string(), flags);
    }

    /// Get property descriptor for a key
    pub fn get_descriptor(&self, key: &str) -> Option<PropertyFlags> {
        self.descriptors.get(key).cloned()
    }

    // ─── TComp PropertyDescriptor API ──────────────────────────────────

    /// GetOwnProperty (ES 9.1.5): returns the property descriptor for an own
    /// property, or None if the property doesn't exist.
    pub fn get_own_property(&self, key: &str) -> Option<PropertyDescriptor> {
        // Check data properties
        if let Some(val) = self.properties.get(key) {
            let flags = self.descriptors.get(key).cloned().unwrap_or_default();
            return Some(PropertyDescriptor {
                value: Some(val.clone()),
                writable: Some(flags.writable),
                enumerable: Some(flags.enumerable),
                configurable: Some(flags.configurable),
                ..Default::default()
            });
        }
        // Check accessor properties
        if let Some(g) = self.getters.get(key) {
            let flags = self.descriptors.get(key).cloned().unwrap_or_default();
            let get_val = g.func.clone().or_else(|| {
                // AST-defined getter: no function value yet
                None
            });
            return Some(PropertyDescriptor {
                get: get_val,
                enumerable: Some(flags.enumerable),
                configurable: Some(flags.configurable),
                get_body: Some(Rc::clone(&g.body)),
                get_closure: Some(Rc::clone(&g.closure)),
                ..Default::default()
            });
        }
        if let Some(s) = self.setters.get(key) {
            let flags = self.descriptors.get(key).cloned().unwrap_or_default();
            let set_val = s.func.clone();
            return Some(PropertyDescriptor {
                set: set_val,
                enumerable: Some(flags.enumerable),
                configurable: Some(flags.configurable),
                set_body: Some(Rc::clone(&s.body)),
                set_closure: Some(Rc::clone(&s.closure)),
                set_param: Some(s.param.clone()),
                ..Default::default()
            });
        }
        // Check array elements
        if let Some(idx) = as_array_index(key) {
            if idx < self.elements.len() {
                return Some(PropertyDescriptor {
                    value: Some(self.elements[idx].clone()),
                    writable: Some(true),
                    enumerable: Some(true),
                    configurable: Some(true),
                    ..Default::default()
                });
            }
        }
        None
    }

    /// DefineOwnProperty (ES 9.1.6): create or update a property with the
    /// given descriptor. Returns true on success.
    pub fn define_own_property(&mut self, key: &str, desc: &PropertyDescriptor) -> bool {
        // Check extensible
        if !self.extensible && !self.properties.contains_key(key) {
            return false;
        }
        if desc.is_data() {
            // Data descriptor: store value + flags
            let value = desc.value.clone().unwrap_or(Value::Undefined);
            let flags = PropertyFlags {
                value: Some(value.clone()),
                writable: desc.writable.unwrap_or(true),
                enumerable: desc.enumerable.unwrap_or(true),
                configurable: desc.configurable.unwrap_or(true),
            };
            self.properties.insert(key.to_string(), value);
            self.descriptors.insert(key.to_string(), flags);
            // Remove any existing accessor
            self.getters.shift_remove(key);
            self.setters.shift_remove(key);
            true
        } else if desc.is_accessor() {
            // Accessor descriptor
            let flags = PropertyFlags {
                value: None,
                writable: false,
                enumerable: desc.enumerable.unwrap_or(true),
                configurable: desc.configurable.unwrap_or(true),
            };
            self.descriptors.insert(key.to_string(), flags);
            // Update getter
            if let Some(ref get_val) = desc.get {
                self.set_getter_func(key, get_val.clone());
            } else if let (Some(ref body), Some(ref closure)) = (&desc.get_body, &desc.get_closure)
            {
                self.getters.insert(
                    key.to_string(),
                    GetterStorage {
                        body: Rc::clone(body),
                        closure: Rc::clone(closure),
                        func: None,
                    },
                );
            }
            // Update setter
            if let Some(ref set_val) = desc.set {
                self.set_setter_func(key, set_val.clone());
            } else if let (Some(ref body), Some(ref closure)) = (&desc.set_body, &desc.set_closure)
            {
                self.setters.insert(
                    key.to_string(),
                    SetterStorage {
                        param: desc.set_param.clone().unwrap_or_default(),
                        body: Rc::clone(body),
                        closure: Rc::clone(closure),
                        func: None,
                    },
                );
            }
            // Remove any existing data property
            self.properties.shift_remove(key);
            true
        } else {
            // Generic descriptor (only enumerable/configurable): update flags
            if let Some(ref mut flags) = self.descriptors.get_mut(key) {
                if let Some(e) = desc.enumerable {
                    flags.enumerable = e;
                }
                if let Some(c) = desc.configurable {
                    flags.configurable = c;
                }
            }
            true
        }
    }

    /// Set a getter function for a property
    pub fn set_getter(
        &mut self,
        key: &str,
        body: std::rc::Rc<Vec<Statement>>,
        closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    ) {
        self.getters.insert(
            key.to_string(),
            GetterStorage {
                body,
                closure,
                func: None,
            },
        );
    }

    /// Install a getter from a function value (Object.defineProperty path)
    pub fn set_getter_func(&mut self, key: &str, func: Value) {
        self.getters.insert(
            key.to_string(),
            GetterStorage {
                body: std::rc::Rc::new(Vec::new()),
                closure: std::rc::Rc::new(std::cell::RefCell::new(Environment::new())),
                func: Some(func),
            },
        );
    }

    /// Set a setter function for a property
    pub fn set_setter(
        &mut self,
        key: &str,
        param: String,
        body: std::rc::Rc<Vec<Statement>>,
        closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    ) {
        self.setters.insert(
            key.to_string(),
            SetterStorage {
                param,
                body,
                closure,
                func: None,
            },
        );
    }

    /// Install a setter from a function value (Object.defineProperty path)
    pub fn set_setter_func(&mut self, key: &str, func: Value) {
        self.setters.insert(
            key.to_string(),
            SetterStorage {
                param: String::new(),
                body: std::rc::Rc::new(Vec::new()),
                closure: std::rc::Rc::new(std::cell::RefCell::new(Environment::new())),
                func: Some(func),
            },
        );
    }

    /// Define an accessor property (get/set function values + flags) without
    /// creating a data property of the same name.
    pub fn define_accessor(
        &mut self,
        key: &str,
        getter: Option<Value>,
        setter: Option<Value>,
        flags: PropertyFlags,
    ) {
        if let Some(g) = getter {
            self.set_getter_func(key, g);
        }
        if let Some(s) = setter {
            self.set_setter_func(key, s);
        }
        self.descriptors.insert(key.to_string(), flags);
    }

    /// Check if property has a getter
    pub fn has_getter(&self, key: &str) -> bool {
        self.getters.contains_key(key)
    }

    /// Check if property has a setter
    pub fn has_setter(&self, key: &str) -> bool {
        self.setters.contains_key(key)
    }

    /// Get the getter storage for a property
    pub fn get_getter(&self, key: &str) -> Option<&GetterStorage> {
        self.getters.get(key)
    }

    /// Get the setter storage for a property
    pub fn get_setter(&self, key: &str) -> Option<&SetterStorage> {
        self.setters.get(key)
    }

    /// Get all property keys (own properties only, including getters/setters).
    /// For arrays, includes actual element indices from elements Vec.
    /// Does not include "length" as an own key (it's a property, not an index).
    pub fn own_keys(&self) -> Vec<String> {
        let mut keys = self.array_indices();
        // HashSet dedup: keys.contains(key) was an O(n) linear scan per key,
        // making own_keys O(n^2) overall.
        let mut seen: std::collections::HashSet<String> = keys.iter().cloned().collect();
        self.add_non_numeric_keys(&mut keys, &mut seen);
        self.add_accessor_keys(&mut keys, &mut seen);
        keys
    }

    /// Like `own_keys` but also includes non-enumerable own properties
    /// (for `Object.getOwnPropertyNames`).
    pub fn own_property_names(&self) -> Vec<String> {
        let mut keys = self.array_indices();
        let mut seen: std::collections::HashSet<String> = keys.iter().cloned().collect();
        for key in self.properties.keys() {
            if as_array_index(key).is_none() && !seen.contains(key) {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
        for key in self.getters.keys().chain(self.setters.keys()) {
            if !seen.contains(key) {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
        keys
    }

    fn array_indices(&self) -> Vec<String> {
        if self.kind == ObjectKind::Array {
            (0..self.elements.len()).map(|i| i.to_string()).collect()
        } else {
            let mut numeric: Vec<(usize, String)> = self
                .properties
                .keys()
                .filter_map(|k| as_array_index(k).map(|i| (i, k.clone())))
                .collect();
            numeric.sort_by_key(|(i, _)| *i);
            numeric.into_iter().map(|(_, k)| k).collect()
        }
    }

    fn add_non_numeric_keys(
        &self,
        keys: &mut Vec<String>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        for key in self.properties.keys() {
            if key != "length"
                && as_array_index(key).is_none()
                && !seen.contains(key)
                && self.is_enumerable(key)
            {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
    }

    fn add_accessor_keys(
        &self,
        keys: &mut Vec<String>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        for key in self.getters.keys() {
            if !seen.contains(key) && self.is_enumerable(key) {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
        for key in self.setters.keys() {
            if !seen.contains(key) && !self.getters.contains_key(key) && self.is_enumerable(key) {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
    }

    /// Check if property exists (own or prototype chain).
    /// Simple recursion: drops each Ref before recursing, so no RefCell conflict.
    pub fn has(&self, key: &str) -> bool {
        if self.has_own(key) {
            return true;
        }
        self.prototype.as_ref().is_some_and(|p| p.borrow().has(key))
    }

    /// Check own property only (no prototype chain)
    pub(crate) fn has_own(&self, key: &str) -> bool {
        if self.properties.contains_key(key)
            || self.getters.contains_key(key)
            || self.setters.contains_key(key)
        {
            return true;
        }
        as_array_index(key)
            .map(|i| i < self.elements.len() && !self.holes.contains(&i))
            .unwrap_or(false)
    }

    /// Delete own property. For numeric keys on arrays, removes from elements.
    /// Respects configurable flag from property descriptor.
    pub fn delete(&mut self, key: &str) -> bool {
        // Check if property is non-configurable
        if let Some(flags) = self.descriptors.get(key) {
            if !flags.configurable {
                return false; // Cannot delete non-configurable property
            }
        }

        if let Some(idx) = as_array_index(key) {
            if idx < self.elements.len() {
                self.elements[idx] = Value::Undefined;
                // Deleting an array element creates a hole
                self.holes.insert(idx);
                self.properties.insert(
                    "length".to_string(),
                    Value::Number(self.elements.len() as f64),
                );
                return true;
            }
        }
        self.descriptors.shift_remove(key);
        let had_getter = self.getters.shift_remove(key).is_some();
        let had_setter = self.setters.shift_remove(key).is_some();
        self.properties.shift_remove(key).is_some() || had_getter || had_setter
    }

    /// Check if a property is enumerable
    pub fn is_enumerable(&self, key: &str) -> bool {
        self.descriptors
            .get(key)
            .map(|f| f.enumerable)
            .unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::kind::ObjectKind;

    #[test]
    fn test_non_canonical_numeric_key_does_not_alias_elements() {
        let mut obj = Object::new_array(3);
        obj.elements[1] = Value::Number(2.0);

        // "01" is not the canonical form of 1: it must be a plain property
        obj.set("01", Value::Number(9.0));
        assert_eq!(obj.get("1"), Some(Value::Number(2.0)));
        assert_eq!(obj.get("01"), Some(Value::Number(9.0)));
        assert_eq!(obj.elements.len(), 3, "elements must not grow for '01'");

        // Canonical indices still hit the elements Vec
        obj.set("1", Value::Number(5.0));
        assert_eq!(obj.elements[1], Value::Number(5.0));
    }

    #[test]
    fn test_huge_index_does_not_grow_elements() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.set("1000000000", Value::Number(1.0));
        assert!(obj.elements.is_empty(), "huge index must not grow elements");
        assert_eq!(obj.get("1000000000"), Some(Value::Number(1.0)));
    }
}
