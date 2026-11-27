//! What the functions of a program share: each function's parent, the Number constants, and the
//! atoms that name the global object.

use crate::bytecode::{Constant, INTRINSIC_SET_PROTOTYPE_OF_BINDING, ResidualProgram};
use crate::typed_ir::{IntRange, Type};
use crate::ValueType;
use super::quiet::{Arguments, BUILTINS};
use super::value::{Key, Origin};

/// Names that reach the global object as a value (`globalThis`, Node's `global`, and the browser names `self` and `window`).
const GLOBAL_OBJECT_NAMES: [&str; 4] = ["globalThis", "global", "self", "window"];
/// Methods that pass their receiver argument as the `this` of a call (`f.call(undefined)`): a sloppy callee gets the global object.
const THIS_PASSERS: [&str; 3] = ["call", "apply", "bind"];

/// Array methods that may change an array's elements or length, so a call of one on an array changes what its
/// elements cell names (`effects`, `flow`). Any other method is taken to leave them as they are.
const ARRAY_MUTATORS: [&str; 9] = ["push", "pop", "shift", "unshift", "splice", "fill", "copyWithin", "reverse", "sort"];
/// Array methods that run no callback and no code of the program: they move values into or out of the array and read no
/// property of an element. A call of one on an array is no escape of its receiver or arguments (`escapes`, `readers`);
/// `sort` is a mutator that calls its comparator, so it is not one.
const ARRAY_BUILTINS: [&str; 12] = ["push", "pop", "shift", "unshift", "splice", "fill", "copyWithin", "reverse", "slice", "indexOf", "lastIndexOf", "includes"];
/// Builtins that add or re-shape the properties of the objects they are given: a call hands them an
/// object, which the closed world must treat as changed (`shape::of_call`).
const PROPERTY_ADDERS: [&str; 4] = ["defineProperty", "defineProperties", "setPrototypeOf", "assign"];

/// The program-wide tables the per-function analyses read.
pub(super) struct Scope {
    /// Each function's parent, by function id.
    parents: Vec<Option<u32>>,
    /// The atoms that name the global object.
    global_objects: Vec<u32>,
    /// The atoms of the builtins that add properties to the objects they are given (`PROPERTY_ADDERS`).
    property_adders: Vec<u32>,
    /// The atoms of the methods that pass a `this` (`THIS_PASSERS`) the program names.
    this_passers: Vec<u32>,
    /// The atom `prototype` interns to, if the program names it.
    pub(super) prototype: Option<u32>,
    /// The atom `length` interns to, if the program names it: a store to it changes an array's length.
    pub(super) length: Option<u32>,
    /// The atom the intrinsic `Object.setPrototypeOf` class heritage calls is bound under, if the program names it
    /// (`accessors::heritage_link`).
    pub(super) set_prototype: Option<u32>,
    /// The atoms of `ARRAY_MUTATORS` the program names.
    mutators: Vec<u32>,
    /// The atoms of `ARRAY_BUILTINS` the program names.
    builtins: Vec<u32>,
    /// The atom `Array` interns to, if the program names it: `new Array(n)` and `Array(n)` construct an array.
    array: Option<u32>,
    /// Each String constant's atom, by constant index.
    pub(super) constant_atoms: Vec<Option<u32>>,
    /// Each constant's type, by constant index (`constant_type`).
    constant_types: Vec<Type>,
    /// Each nested field lookup's last field (`FieldSite`: `base.first.second`, or `base.first`): the field
    /// its read delivers, by site index; `None` for a site with a sink, which stores rather than reads.
    pub(super) site_reads: Vec<Option<u32>>,
    /// Each nested field lookup's base and fields (`base.first.second`), by site index, which a read's
    /// classes follow field by field (`effects::member_type`); `None` for a site with a sink.
    pub(super) site_paths: Vec<Option<(crate::bytecode::FieldBase, u32, Option<u32>)>>,
    /// The builtins of `quiet::BUILTINS` the program names, as (object atom, name atom, arguments).
    builtins: Vec<(Option<u32>, u32, Arguments)>,
}

impl Scope {
    pub(super) fn of(program: &ResidualProgram) -> Self {
        Self {
            parents: (0..program.function_count())
                .map(|id| program.with_record(id, |function| function.parent))
                .collect(),
            constant_types: program.constants.iter().map(constant_type).collect(),
            global_objects: program
                .atoms
                .iter()
                .enumerate()
                .filter(|(_, name)| GLOBAL_OBJECT_NAMES.contains(name))
                .map(|(atom, _)| atom as u32)
                .collect(),
            property_adders: program
                .atoms
                .iter()
                .enumerate()
                .filter(|(_, name)| PROPERTY_ADDERS.contains(name))
                .map(|(atom, _)| atom as u32)
                .collect(),
            this_passers: THIS_PASSERS.iter().filter_map(|name| atom_named(program, name)).collect(),
            prototype: program.atoms.iter().position(|name| name == "prototype").map(|atom| atom as u32),
            length: program.atoms.iter().position(|name| name == "length").map(|atom| atom as u32),
            set_prototype: atom_named(program, INTRINSIC_SET_PROTOTYPE_OF_BINDING),
            mutators: program
                .atoms
                .iter()
                .enumerate()
                .filter(|(_, name)| ARRAY_MUTATORS.contains(name))
                .map(|(atom, _)| atom as u32)
                .collect(),
            builtins: program
                .atoms
                .iter()
                .enumerate()
                .filter(|(_, name)| ARRAY_BUILTINS.contains(name))
                .map(|(atom, _)| atom as u32)
                .collect(),
            array: program.atoms.iter().position(|name| name == "Array").map(|atom| atom as u32),
            constant_atoms: program.constants.iter().map(|constant| match constant {
                Constant::String(text) => program.atoms.iter().position(|name| name == text.as_ref()).map(|atom| atom as u32),
                _ => None,
            }).collect(),
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
        }
    }

    /// The type of constant `index`: what the constant is, not what a register holds (`Word`).
    pub(super) fn constant_type(&self, index: usize) -> Type {
        self.constant_types.get(index).copied().unwrap_or(Type::ANY)
    }

    pub(super) fn is_global_object(&self, atom: u32) -> bool {
        self.global_objects.contains(&atom)
    }

    /// Whether `atom` names an array method that may change the array it is called on (`ARRAY_MUTATORS`).
    pub(super) fn is_array_mutator(&self, atom: u32) -> bool {
        self.mutators.contains(&atom)
    }

    /// Whether `atom` names an array builtin that runs no callback and no program code (`ARRAY_BUILTINS`).
    pub(super) fn is_array_builtin(&self, atom: u32) -> bool {
        self.builtins.contains(&atom)
    }

    /// Whether `origin` is the global `Array`, whose call allocates an array (`effects::array_constructed`).
    pub(super) fn is_array_constructor(&self, origin: Option<Origin>) -> bool {
        matches!(origin, Some(Origin::Binding(Key::Global(atom))) if self.array == Some(atom))
    }

    /// Whether the method named `atom` passes its receiver argument as a call's `this` (`THIS_PASSERS`).
    pub(super) fn passes_this(&self, atom: u32) -> bool {
        self.this_passers.contains(&atom)
    }

    /// Whether the builtin named `atom` adds properties to the objects it is given (`property_adders`).
    pub(super) fn adds_properties(&self, atom: u32) -> bool {
        self.property_adders.contains(&atom)
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

/// The atom whose name is `text`, if the program names it.
fn atom_named(program: &ResidualProgram, text: &str) -> Option<u32> {
    program.atoms.iter().position(|atom| {
        let atom: &str = atom.as_ref();
        atom == text
    }).map(|atom| atom as u32)
}

/// The type a program constant denotes: a Number's integer range where it is an int32 (its singleton), else a Number;
/// a string's, a BigInt's, a `null`'s, an `undefined`'s or a Boolean's exact type; `ANY` for any other constant.
pub fn constant_type(constant: &Constant) -> Type {
    match constant {
        Constant::Number(value) => crate::value::exact_int32(*value)
            .map_or(Type::NUMBER, |int| Type::int(IntRange::singleton(i64::from(int)))),
        Constant::Null => Type::language(ValueType::Null),
        Constant::Undefined => Type::language(ValueType::Undefined),
        Constant::Boolean(_) => Type::language(ValueType::Boolean),
        Constant::String(_) | Constant::StringUnits(_) => Type::language(ValueType::String),
        Constant::BigInt(_) => Type::language(ValueType::BigInt),
        _ => Type::ANY,
    }
}
