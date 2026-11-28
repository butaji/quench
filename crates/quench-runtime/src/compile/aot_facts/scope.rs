//! What the functions of a program share: each function's parent, the Number constants, and the
//! atoms that name the global object.

use crate::bytecode::{Constant, INTRINSIC_SET_PROTOTYPE_OF_BINDING, ProgramKind, ResidualProgram};
use crate::heap::TypedArrayKind;
use crate::typed_ir::{IntRange, Type};
use std::collections::BTreeSet;
use crate::ValueType;
use super::quiet::{Arguments, BUILTINS, Proofs, Quiet, STRING_METHODS, StringBuiltins, StringMethod};
use super::value::{Key, Origin};

/// The names the realm installs as the global object (`globalThis`, and Node's `global`, which the host installs). A browser
/// name (`self`, `window`) is unbound unless the program writes it, and a program reaches the global object to write it only
/// through one of these, its root `this`, or a sloppy call, each of which already makes the global object a value.
const GLOBAL_OBJECT_NAMES: [&str; 2] = ["globalThis", "global"];
/// Methods that pass their receiver argument as the `this` of a call (`f.call(undefined)`): a sloppy callee gets the global object.
const THIS_PASSERS: [&str; 3] = ["call", "apply", "bind"];

/// Array methods that may change an array's elements or length, so a call of one on an array changes what its
/// elements cell names (`effects`, `flow`). Any other method is taken to leave them as they are.
const ARRAY_MUTATORS: [&str; 9] = ["push", "pop", "shift", "unshift", "splice", "fill", "copyWithin", "reverse", "sort"];
/// Array methods that run no callback and no code of the program: they move values into or out of the array and read no
/// property of an element. A call of one on an array is no escape of its receiver or arguments (`escapes`, `readers`);
/// `sort` is a mutator that calls its comparator, so it is not one.
const ARRAY_BUILTINS: [&str; 12] = ["push", "pop", "shift", "unshift", "splice", "fill", "copyWithin", "reverse", "slice", "indexOf", "lastIndexOf", "includes"];
/// What a builtin that touches the properties of the objects it is given does to them, one row per name (`PROPERTY_BUILTINS`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Descriptors {
    /// It may make a property non-writable or an accessor, or an object non-extensible, so a store may fail or run a trap.
    Change,
    /// It does so only when given its second argument (`Object.create(proto, descriptors)`).
    WithSecond,
    /// It writes plain data properties only (`Object.assign`).
    Keep,
}

/// The builtins that add, re-shape or lock the properties of the objects they are given: whether a call re-shapes its
/// arguments (`Scope::adds_properties`, read by `shape::of_call` and the prototype-writes fact), and what it does to their
/// descriptors (`Scope::changes_descriptors`, read by the field readers so a store that may fail is never dropped).
const PROPERTY_BUILTINS: [(&str, bool, Descriptors); 9] = [
    ("defineProperty", true, Descriptors::Change),
    ("defineProperties", true, Descriptors::Change),
    ("setPrototypeOf", true, Descriptors::Change),
    ("assign", true, Descriptors::Keep),
    ("freeze", false, Descriptors::Change),
    ("seal", false, Descriptors::Change),
    ("preventExtensions", false, Descriptors::Change),
    ("create", false, Descriptors::WithSecond),
    ("Proxy", false, Descriptors::Change),
];
/// The typed-array constructors whose kind has a native element (`TypedArrayKind::has_native_element`): a call of one
/// allocates a typed array of that kind (`effects::typed_array_constructed`).
const TYPED_ARRAY_CONSTRUCTORS: [(&str, TypedArrayKind); 9] = [
    ("Uint8Array", TypedArrayKind::Uint8),
    ("Uint8ClampedArray", TypedArrayKind::Uint8Clamped),
    ("Uint16Array", TypedArrayKind::Uint16),
    ("Uint32Array", TypedArrayKind::Uint32),
    ("Int8Array", TypedArrayKind::Int8),
    ("Int16Array", TypedArrayKind::Int16),
    ("Int32Array", TypedArrayKind::Int32),
    ("Float32Array", TypedArrayKind::Float32),
    ("Float64Array", TypedArrayKind::Float64),
];

/// The program-wide tables the per-function analyses read.
pub(super) struct Scope {
    /// Each function's parent, by function id.
    parents: Vec<Option<u32>>,
    /// Whether the program is a Module: its root is strict code, whose `this` is `undefined` (`root_this_is_global`).
    module: bool,
    /// The atoms that name the global object.
    global_objects: Vec<u32>,
    /// The atoms of the names a builtin reads off an object with no read instruction of the program (`readers::PROTOCOL_NAMES`).
    pub(super) protocols: std::collections::BTreeSet<u32>,
    /// The atom `globalThis` interns to, if the program names it: the one global name the realm installs as the global
    /// object itself (`GlobalObject`). `global` is the host's alias of it, and `self` and `window` are not installed.
    global_this: Option<u32>,
    /// The atoms of the property builtins the program names, with their row of `PROPERTY_BUILTINS`.
    property_adders: Vec<(u32, bool, Descriptors)>,
    /// The atoms of the methods that pass a `this` (`THIS_PASSERS`) the program names.
    this_passers: Vec<u32>,
    /// The atom `prototype` interns to, if the program names it.
    pub(super) prototype: Option<u32>,
    /// The atom `constructor` interns to, if the program names it: the read that reaches the `Function` constructor.
    pub(super) constructor: Option<u32>,
    /// The atom `length` interns to, if the program names it: a store to it changes an array's length.
    pub(super) length: Option<u32>,
    /// The atom `__proto__` interns to, if the program names it: a read of it is a builtin prototype (`Cell::prototype`).
    proto: Option<u32>,
    /// The atom the intrinsic `Object.setPrototypeOf` class heritage calls is bound under, if the program names it
    /// (`accessors::heritage_link`).
    pub(super) set_prototype: Option<u32>,
    /// The atoms of `ARRAY_MUTATORS` the program names.
    mutators: Vec<u32>,
    /// The atoms of `ARRAY_BUILTINS` the program names.
    array_builtins: Vec<u32>,
    /// The atom `Array` interns to, if the program names it: `new Array(n)` and `Array(n)` construct an array.
    array: Option<u32>,
    /// The atoms of the native typed-array constructors the program names, with their kinds (`TYPED_ARRAY_CONSTRUCTORS`).
    typed_arrays: Vec<(u32, TypedArrayKind)>,
    /// Each String constant's atom, by constant index.
    pub(super) constant_atoms: Vec<Option<u32>>,
    /// Each constant's type, by constant index (`constant_type`).
    constant_types: Vec<Type>,
    /// Each Number constant's value, by constant index (`constant_number`); None for any other constant.
    constant_numbers: Vec<Option<f64>>,
    /// Each nested field lookup's last field (`FieldSite`: `base.first.second`, or `base.first`): the field
    /// its read delivers, by site index; `None` for a site with a sink, which stores rather than reads.
    pub(super) site_reads: Vec<Option<u32>>,
    /// Each nested field lookup's base and fields (`base.first.second`), by site index, which a read's
    /// classes follow field by field (`effects::member_type`); `None` for a site with a sink.
    pub(super) site_paths: Vec<Option<(crate::bytecode::FieldBase, u32, Option<u32>)>>,
    /// The builtins of `quiet::BUILTINS` the program names, as (object atom, name atom, arguments).
    builtins: Vec<(Option<u32>, u32, Arguments)>,
    /// The atom the global `String` interns to, if the program names it (`String(x)` is a string conversion).
    string_global: Option<u32>,
    /// The atoms of the `STRING_METHODS` the program names, with their methods.
    string_methods: Vec<(u32, StringMethod)>,
    /// The atoms whose text is a canonical array index: a string key of one reads an element, not a named field.
    index_names: BTreeSet<u32>,
    /// Each object literal site's keys, in the order its values sit in the literal's registers (`ObjectSite::atoms`).
    pub(super) object_sites: Vec<Vec<u32>>,
    /// The atom `assign` interns to, if the program names it: `Object.assign` copies its source's keys (`copies`).
    pub(super) assign: Option<u32>,
    /// The atom `construct` interns to, when the program names `Reflect` and `construct` (`Reflect.construct`): a construction
    /// it makes may take a new_target other than the constructor (`shapes::direct_constructions_only`).
    pub(super) reflect_construct: Option<u32>,
}

impl Scope {
    pub(super) fn of(program: &ResidualProgram) -> Self {
        Self {
            parents: (0..program.function_count())
                .map(|id| program.with_record(id, |function| function.parent))
                .collect(),
            module: matches!(program.kind, ProgramKind::Module),
            protocols: program.atoms.iter().enumerate().filter(|(_, name)| super::readers::PROTOCOL_NAMES.contains(&name.as_ref())).map(|(atom, _)| atom as u32).collect(),
            constant_types: program.constants.iter().zip(constant_atoms(program))
                .map(|(constant, atom)| constant_type(constant, atom)).collect(),
            constant_numbers: program.constants.iter().map(|constant| match constant {
                Constant::Number(value) => Some(*value),
                _ => None,
            }).collect(),
            global_objects: program
                .atoms
                .iter()
                .enumerate()
                .filter(|(_, name)| GLOBAL_OBJECT_NAMES.contains(name))
                .map(|(atom, _)| atom as u32)
                .collect(),
            global_this: program.atoms.iter().position(|name| name == "globalThis").map(|atom| atom as u32),
            property_adders: program
                .atoms
                .iter()
                .enumerate()
                .filter_map(|(atom, name)| PROPERTY_BUILTINS.iter().find(|row| row.0 == name.as_ref() as &str).map(|row| (atom as u32, row.1, row.2)))
                .collect(),
            this_passers: THIS_PASSERS.iter().filter_map(|name| atom_named(program, name)).collect(),
            prototype: program.atoms.iter().position(|name| name == "prototype").map(|atom| atom as u32),
            constructor: program.atoms.iter().position(|name| name == "constructor").map(|atom| atom as u32),
            length: program.atoms.iter().position(|name| name == "length").map(|atom| atom as u32),
            proto: program.atoms.iter().position(|name| name == "__proto__").map(|atom| atom as u32),
            set_prototype: atom_named(program, INTRINSIC_SET_PROTOTYPE_OF_BINDING),
            mutators: program
                .atoms
                .iter()
                .enumerate()
                .filter(|(_, name)| ARRAY_MUTATORS.contains(name))
                .map(|(atom, _)| atom as u32)
                .collect(),
            array_builtins: program
                .atoms
                .iter()
                .enumerate()
                .filter(|(_, name)| ARRAY_BUILTINS.contains(name))
                .map(|(atom, _)| atom as u32)
                .collect(),
            array: program.atoms.iter().position(|name| name == "Array").map(|atom| atom as u32),
            typed_arrays: TYPED_ARRAY_CONSTRUCTORS.iter().filter_map(|(name, kind)| atom_named(program, name).map(|atom| (atom, *kind))).collect(),
            constant_atoms: constant_atoms(program),
            object_sites: program.object_sites.iter().map(|site| site.atoms.iter().copied().collect()).collect(),
            assign: program.atoms.iter().position(|name| name == "assign").map(|atom| atom as u32),
            reflect_construct: atom_named(program, "Reflect").and_then(|_| atom_named(program, "construct")),
            site_reads: program.field_sites.iter().map(|site| site.sink.is_none().then(|| site.second.map_or(site.first.0, |(atom, _)| atom))).collect(),
            site_paths: program.field_sites.iter().map(|site| site.sink.is_none().then(|| (site.base, site.first.0, site.second.map(|(atom, _)| atom)))).collect(),
            builtins: BUILTINS.iter().filter_map(|builtin| {
                let name = atom_named(program, builtin.name)?;
                let object = match builtin.object {
                    Some(object) => Some(atom_named(program, object)?),
                    None => None,
                };
                Some((object, name, builtin.arguments))
            }).collect(),
            string_global: atom_named(program, "String"),
            string_methods: STRING_METHODS.iter().filter_map(|(name, method)| Some((atom_named(program, name)?, *method))).collect(),
            index_names: program.atoms.iter().enumerate()
                .filter(|(_, name)| {
                    let name: &str = name.as_ref();
                    is_array_index(name)
                })
                .map(|(atom, _)| atom as u32).collect(),
        }
    }

    /// The String builtins the program leaves as the host installed them, under the program's proofs (`Proofs`).
    pub(super) fn string_builtins(&self, proofs: &Proofs) -> StringBuiltins {
        StringBuiltins {
            global: self.string_global.filter(|&atom| proofs.proves(Quiet::Intrinsic { object: None, name: atom })),
            methods: self.string_methods.iter().copied().filter(|&(atom, _)| proofs.string_method(atom, self.string_global)).collect(),
        }
    }

    /// Whether the string key `atom` names an array index, so a read of it is an element read.
    pub(super) fn names_index(&self, atom: u32) -> bool {
        self.index_names.contains(&atom)
    }

    /// The type of constant `index`: what the constant is, not what a register holds (`Word`).
    pub(super) fn constant_type(&self, index: usize) -> Type {
        self.constant_types.get(index).copied().unwrap_or(Type::ANY)
    }

    /// The value of the Number constant at `index`, None where the program's constant there is no Number.
    pub(super) fn constant_number(&self, index: usize) -> Option<f64> {
        self.constant_numbers.get(index).copied().flatten()
    }

    pub(super) fn is_global_object(&self, atom: u32) -> bool {
        self.global_objects.contains(&atom)
    }

    /// Whether `atom` is `globalThis`, the global name the realm installs as the global object (`Identity::GlobalObject`).
    pub(super) fn is_global_this(&self, atom: u32) -> bool {
        self.global_this == Some(atom)
    }

    /// The atom `globalThis` interns to, if the program names it.
    pub(super) fn global_this(&self) -> Option<u32> {
        self.global_this
    }

    /// Whether `atom` names an array method that may change the array it is called on (`ARRAY_MUTATORS`).
    pub(super) fn is_array_mutator(&self, atom: u32) -> bool {
        self.mutators.contains(&atom)
    }

    /// Whether `atom` names an array builtin that runs no callback and no program code (`ARRAY_BUILTINS`).
    pub(super) fn is_array_builtin(&self, atom: u32) -> bool {
        self.array_builtins.contains(&atom)
    }

    /// Whether `origin` is the global `Array`, whose call allocates an array (`effects::array_constructed`).
    pub(super) fn is_array_constructor(&self, origin: Option<Origin>) -> bool {
        matches!(origin, Some(Origin::Binding(Key::Global(atom))) if self.array == Some(atom))
    }

    /// The native kind of the typed-array constructor `origin` is the global binding of, if it is one.
    pub(super) fn typed_array_kind(&self, origin: Option<Origin>) -> Option<TypedArrayKind> {
        match origin {
            Some(Origin::Binding(Key::Global(atom))) => self.typed_arrays.iter().find(|(named, _)| *named == atom).map(|(_, kind)| *kind),
            _ => None,
        }
    }

    /// Whether the method named `atom` passes its receiver argument as a call's `this` (`THIS_PASSERS`).
    pub(super) fn passes_this(&self, atom: u32) -> bool {
        self.this_passers.contains(&atom)
    }

    /// The parent of function `function`, None for a root.
    pub(super) fn parent(&self, function: u32) -> Option<u32> {
        self.parents.get(function as usize).copied().flatten()
    }

    /// Whether the program is a Module.
    pub(super) fn is_module(&self) -> bool {
        self.module
    }

    /// Whether the root's `this` is the global object: a Script's is (sloppy top-level code), a Module's is `undefined`
    /// (its code is strict, as a Module is).
    pub(super) fn root_this_is_global(&self) -> bool {
        !self.module
    }

    /// Whether `atom` names a prototype object's property a builtin reads as one: `prototype` or `__proto__` (`Cell::prototype`).
    pub(super) fn is_prototype_name(&self, atom: u32) -> bool {
        self.prototype == Some(atom) || self.proto == Some(atom)
    }

    /// Whether the builtin named `atom`, called with `arguments` arguments, may define a property of an object it is given, or make
    /// one non-writable or non-extensible (`PROPERTY_BUILTINS`). `Object.create` does so only with its second argument, the descriptors.
    pub(super) fn changes_descriptors(&self, atom: u32, arguments: usize) -> bool {
        self.property_adders.iter().any(|&(named, _, descriptors)| named == atom && match descriptors {
            Descriptors::Change => true,
            Descriptors::WithSecond => arguments >= 2,
            Descriptors::Keep => false,
        })
    }

    /// Whether `atom` names a builtin of `PROPERTY_BUILTINS` the program names: a call of it may define or lock the properties of
    /// the objects it is given (`identity::may_define`).
    pub(super) fn is_property_builtin(&self, atom: u32) -> bool {
        self.property_adders.iter().any(|&(named, _, _)| named == atom)
    }

    /// Whether the builtin named `atom` adds or re-shapes properties of the objects it is given (`PROPERTY_BUILTINS`).
    pub(super) fn adds_properties(&self, atom: u32) -> bool {
        self.property_adders.iter().any(|&(named, shapes, _)| named == atom && shapes)
    }

    /// The arguments the builtin `name` of `object` (a global, for a method) runs no guest code for, if the program names it.
    pub(super) fn builtin(&self, object: Option<u32>, name: u32) -> Option<Arguments> {
        self.builtins.iter().find(|(o, n, _)| *o == object && *n == name).map(|(_, _, arguments)| *arguments)
    }

    /// The function owning the environment `depth` levels out from `function`, which a capture reads.
    pub(super) fn owner(&self, function: u32, depth: u16) -> Option<u32> {
        (0..=depth).try_fold(function, |owner, _| self.parents.get(owner as usize).copied().flatten())
    }
}

/// Whether `text` is a canonical array index: decimal digits with no leading zero, below 2^32 - 1.
fn is_array_index(text: &str) -> bool {
    let canonical = text == "0" || (!text.is_empty() && !text.starts_with('0') && text.bytes().all(|byte| byte.is_ascii_digit()));
    canonical && text.parse::<u32>().is_ok_and(|index| index < u32::MAX)
}

/// The atom whose name is `text`, if the program names it.
fn atom_named(program: &ResidualProgram, text: &str) -> Option<u32> {
    program.atoms.iter().position(|atom| {
        let atom: &str = atom.as_ref();
        atom == text
    }).map(|atom| atom as u32)
}

/// Each constant's atom, by constant index: a String constant whose text the program names interns to that atom.
fn constant_atoms(program: &ResidualProgram) -> Vec<Option<u32>> {
    program.constants.iter().map(|constant| match constant {
        Constant::String(text) => program.atoms.iter().position(|name| name == text.as_ref()).map(|atom| atom as u32),
        _ => None,
    }).collect()
}

/// The type a program constant denotes: a Number's integer range where it is an int32 (its singleton), else a Number;
/// a string's exact length, which is an atom's (`atom`) where the program names its text, so the string is that atom, and
/// flat, as the runtime makes a constant's text as one run of units (`JsString::from_str`, `JsString::from_units`);
/// a BigInt's, a `null`'s, an `undefined`'s or a Boolean's exact type; `ANY` for any other constant.
pub fn constant_type(constant: &Constant, atom: Option<u32>) -> Type {
    match constant {
        Constant::Number(value) => crate::value::exact_int32(*value)
            .map_or(Type::NUMBER, |int| Type::int(IntRange::singleton(i64::from(int)))),
        Constant::Null => Type::language(ValueType::Null),
        Constant::Undefined => Type::language(ValueType::Undefined),
        Constant::Boolean(_) => Type::language(ValueType::Boolean),
        Constant::String(text) => {
            let length = u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX);
            let atoms: Vec<u32> = atom.into_iter().collect();
            Type::string_atoms(&atoms, length, length, true)
        }
        Constant::StringUnits(units) => {
            let length = u32::try_from(units.len()).unwrap_or(u32::MAX);
            Type::string_length(length, length, true)
        }
        Constant::BigInt(_) => Type::language(ValueType::BigInt),
        _ => Type::ANY,
    }
}
