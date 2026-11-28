//! The points that run no guest code, as the closed world proves it. A constructor's body may run guest code
//! at a point only if that point may hand code the instance; a point the proofs here cover does not. The flow
//! records each candidate proof at its PC (`Quiet`), with the facts of its operands; `Proofs` decides the
//! program-wide part: the global names the program leaves alone, and the builtins' properties it never stores.
//! `BUILTINS` is the one table of the builtins a call may name, with the arguments each runs no guest code for.

use super::flow::{Call, FunctionFlow, Target};
use super::scope::Scope;
use super::shape::{Change, Owner};
use super::value::{Key, Origin, Word};
use crate::typed_ir::{Cell, Classes, Identity, Sites};
use std::collections::BTreeSet;

/// The argument types a builtin runs no guest code for: a conversion of an object runs `valueOf`, of a Number none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Arguments {
    /// No argument.
    Nothing,
    /// No argument, or one Number (`Array(n)` makes an array of length `n`).
    NothingOrNumber,
    /// Every argument is a Number.
    Numbers,
}

/// A builtin a call names: a global function (`object: None`) or a method of a global object, and the arguments it
/// runs no guest code for.
pub(super) struct Builtin {
    pub(super) object: Option<&'static str>,
    pub(super) name: &'static str,
    pub(super) arguments: Arguments,
}

/// The builtins the proofs cover. A call of one the program never rebinds runs no guest code for those arguments.
pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { object: None, name: "Array", arguments: Arguments::NothingOrNumber },
    Builtin { object: None, name: "Object", arguments: Arguments::Nothing },
    Builtin { object: Some("Math"), name: "abs", arguments: Arguments::Numbers },
    Builtin { object: Some("Math"), name: "ceil", arguments: Arguments::Numbers },
    Builtin { object: Some("Math"), name: "floor", arguments: Arguments::Numbers },
    Builtin { object: Some("Math"), name: "max", arguments: Arguments::Numbers },
    Builtin { object: Some("Math"), name: "min", arguments: Arguments::Numbers },
    Builtin { object: Some("Math"), name: "round", arguments: Arguments::Numbers },
    Builtin { object: Some("Math"), name: "sqrt", arguments: Arguments::Numbers },
    Builtin { object: Some("Math"), name: "trunc", arguments: Arguments::Numbers },
];

/// The String methods whose result the string rules type (`staged::typing::char_at` and the rest), by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StringMethod {
    CharAt,
    CharCodeAt,
    Substring,
    Slice,
}

/// The String methods the typing names, with their method names.
pub(super) const STRING_METHODS: [(&str, StringMethod); 4] = [
    ("charAt", StringMethod::CharAt),
    ("charCodeAt", StringMethod::CharCodeAt),
    ("substring", StringMethod::Substring),
    ("slice", StringMethod::Slice),
];

/// The String builtins a call's result is typed by (`Types::strings`), where the program leaves them as the host installed
/// them: the atom of the global `String` (`String(x)` converts `x` to a string), and the atoms of the methods of
/// `STRING_METHODS` no store or definition names.
#[derive(Default, PartialEq, Eq)]
pub(super) struct StringBuiltins {
    pub(super) global: Option<u32>,
    pub(super) methods: Vec<(u32, StringMethod)>,
}

/// A point that may run guest code and the proof that it does not, as the flow found it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Quiet {
    /// A read of global name `atom` by name, which is a data property the program declares or stores (`Proofs::proves`).
    Declared(u32),
    /// A call or read of a builtin: global `name` (`object: None`), or method `name` of global `object`.
    Intrinsic { object: Option<u32>, name: u32 },
    /// A read of field `atom` off `this`: an own data property once a store of the constructor made it, so the
    /// read runs no getter on the path that stored it (decided by the body, `class_layout`).
    OwnField(u32),
}

/// Whether the arguments `words` of a builtin satisfy its condition.
fn arguments_quiet(arguments: Arguments, words: &[Word]) -> bool {
    let numbers = words.iter().all(|word| word.is_number());
    match arguments {
        Arguments::Nothing => words.is_empty(),
        Arguments::NothingOrNumber => words.len() <= 1 && numbers,
        Arguments::Numbers => numbers,
    }
}

/// The proof of a call's point, if its callee is a builtin the table covers for its arguments: a global function
/// the callee register names, or a method the receiver's global object names.
pub(super) fn of_call(scope: &Scope, call: &Call) -> Option<Quiet> {
    let (object, name) = match (call.target, call.callee.origin) {
        (Target::Method(name), _) => match call.receiver.origin {
            Some(Origin::Binding(Key::Global(object))) => (Some(object), name),
            _ => return None,
        },
        (Target::Register, Some(Origin::Binding(Key::Global(name)))) => (None, name),
        _ => return None,
    };
    let arguments = scope.builtin(object, name)?;
    arguments_quiet(arguments, &call.arguments).then_some(Quiet::Intrinsic { object, name })
}

/// What the program does to the global object and the builtins, over all its functions.
#[derive(Default)]
pub(super) struct Proofs {
    /// Some code reaches a name the analysis cannot follow (`with`, `eval`, sloppy declarations).
    wild: bool,
    /// Some write reaches the global object by a name the analysis cannot name.
    global_wild: bool,
    /// Some store through a non-Number key may write a name of the global object (`FunctionFlow::computed_global`).
    computed: bool,
    /// The global object may be reachable as a value: some function makes it one (`FunctionFlow::global_source`), or
    /// some sloppy plain call reaches a function that reads its `this` (`FunctionFlow::plain_calls`).
    pub(super) global_source: bool,
    /// Some call defines properties on the objects it is given (`Change::Define`).
    defined: bool,
    /// Some store names a key the analysis cannot name (`Change::Any`).
    any_change: bool,
    /// The global names of the builtins some call may define properties on (`handed`): a member of one may change.
    changed: BTreeSet<u32>,
    /// The global names the program writes.
    pub(super) written: BTreeSet<u32>,
    /// The property names the program stores or defines by name.
    pub(super) stored: BTreeSet<u32>,
}

impl Proofs {
    pub(super) fn of(flows: &[FunctionFlow]) -> Self {
        let mut proofs = Self::default();
        for flow in flows {
            proofs.wild |= flow.wild_owner;
            proofs.global_wild |= flow.global_wild;
            proofs.computed |= flow.computed_global;
            proofs.global_source |= flow.global_source;
            proofs.global_source |= flow.plain_calls.iter().any(|&callee| {
                callee.and_then(|function| flows.get(function as usize)).map_or(true, |callee| callee.reads_this)
            });
            proofs.written.extend(flow.writes.iter().filter_map(|(_, key, _)| match key {
                Key::Global(atom) => Some(*atom),
                _ => None,
            }));
            // A wild holder that may be any object (`Classes::Many` or unknown sites) may be a builtin's object.
            if flow.wild.classes == Classes::Many || flow.wild.sites == Sites::Many {
                proofs.any_change = true;
            }
            for shape in &flow.shapes {
                // A change through an object the closed world holds as a builtin or the global object is a write to it.
                if let Owner::Host(identity, _) = shape.owner {
                    proofs.host_change(identity, shape.change);
                    continue;
                }
                // A store or delete reaches a builtin's own property only through an object that may be one: any object
                // (`Classes::Many`) or a prototype object. An instance of a program class (`Classes::Set`) is none.
                let builtin = match shape.owner {
                    Owner::Prototype(_) => true,
                    owner => owner.classes() == Some(Classes::Many),
                };
                match shape.change {
                    Change::Field(atom) if builtin => {
                        proofs.stored.insert(atom);
                    }
                    Change::Any if builtin => proofs.any_change = true,
                    Change::Define => proofs.defined = true,
                    Change::Field(_) | Change::Any => {}
                }
            }
        }
        proofs
    }

    /// A change the program makes through an object the closed world holds as `identity`: a store or delete of a builtin's
    /// property names it (`Field`), and any other change may name any of the builtin's (`Any`, `Define`). On the global object a
    /// store names a global (`Field`, as `binding_stores` records it), and a delete or define may name any.
    fn host_change(&mut self, identity: Identity, change: Change) {
        match (identity, change) {
            (Identity::Builtin(_), Change::Field(atom)) => {
                self.stored.insert(atom);
            }
            (Identity::Builtin(_), Change::Any | Change::Define) => self.any_change = true,
            (_, Change::Field(atom)) => {
                self.written.insert(atom);
            }
            (_, Change::Any) => self.global_wild = true,
            (_, Change::Define) => self.defined = true,
        }
    }

    /// A value a call may define properties on (`identity::builtin_proofs`). A value that is exactly a builtin changes the
    /// members of its global. A value that may be any object (`Identity::Many`, `Classes::Many`) may be one of the `leaked`
    /// builtins, so it changes each of those. The global object is `defined`, which fails `global_object_fixed`.
    pub(super) fn handed(&mut self, cell: &Cell, leaked: &BTreeSet<u32>) {
        match cell.identity {
            Identity::Builtin(path) => {
                self.changed.insert(path.global);
            }
            Identity::Many => self.changed.extend(leaked),
            _ => {}
        }
        if cell.classes == Classes::Many {
            self.changed.extend(leaked);
        }
        if matches!(cell.identity, Identity::GlobalObject) || cell.global {
            self.defined = true;
        }
    }

    /// Whether the proof `quiet` holds for the whole program. An own-field read is decided by the body, not here.
    pub(super) fn proves(&self, quiet: Quiet) -> bool {
        match quiet {
            Quiet::Declared(atom) => !self.wild && !self.defined && self.written.contains(&atom),
            Quiet::Intrinsic { object: None, name } => self.intrinsic(name),
            Quiet::Intrinsic { object: Some(object), name } => {
                self.intrinsic(object) && !self.stored.contains(&name) && !self.any_change && !self.changed.contains(&object)
            }
            Quiet::OwnField(_) => false,
        }
    }

    /// Whether a String method named `atom` is still the host's: no store or definition names it, and no call the analysis
    /// cannot follow may be handed `String` or its prototype (`changed` names the global `string`, String.prototype's).
    pub(super) fn string_method(&self, atom: u32, string: Option<u32>) -> bool {
        !self.wild && !self.defined && !self.any_change && string.is_none_or(|global| !self.changed.contains(&global))
            && !self.stored.contains(&atom)
    }

    /// Whether global `atom` is still the builtin the host installed: the program never writes it, nor reaches the
    /// global object by a name it cannot follow, nor defines a property. A computed-key store reaches the global object
    /// only if the program makes it a value (`global_source`).
    fn intrinsic(&self, atom: u32) -> bool {
        self.global_object_fixed() && !self.written.contains(&atom)
    }

    /// Whether the program's own stores, defines and deletes leave the global object's properties as the host made them, by
    /// the names they write: no code reaches a name the analysis cannot follow, no write reaches the global object by a name it
    /// cannot name, no store through a computed key may write a global name while the global object is a value, and no
    /// builtin defines a property. Each global name is then fixed by `written` alone (`intrinsic`, `globals::GlobalShape`).
    pub(super) fn global_object_fixed(&self) -> bool {
        !self.wild && !self.global_wild && !(self.computed && self.global_source) && !self.defined
    }

    /// Whether the global object keeps the binding of `global` (`globalThis`, if the program names it) as the realm installed it:
    /// the program never writes that name, and no store, define or delete it makes reaches a name it cannot follow
    /// (`wild`, `defined`, a computed store into a global object that is a value). Unlike `global_object_fixed` it does not
    /// count a read of the global object as a value (`global_wild` with no `wild`): that read changes no binding. Every
    /// `global_wild` source the analysis records beside `wild` (a name operation, a capture) also sets `wild`.
    pub(super) fn global_object_identity_fixed(&self, global: Option<u32>) -> bool {
        !self.wild && !self.defined && !(self.computed && self.global_source) && global.is_none_or(|atom| !self.written.contains(&atom))
    }
}
