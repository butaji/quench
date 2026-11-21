//! `node:sqlite`: `DatabaseSync` and `StatementSync` over the pure-Rust `turso_core` engine.
//! An instance keeps its engine state in `State`; the JavaScript object carries only its
//! index there, under a private symbol that is never visible to guest code.
use crate::host::NodeAccess;
pub(crate) mod database;
pub(crate) mod statement;
pub(crate) mod values;

use crate::modules::host_members::{accessors, function, Members};
use crate::modules::node_errors_shared_vm::{throw_coded_error, throw_coded_type_error};
use crate::modules::stream_shared_vm::{get, put, Context, Outcome};
use quench_runtime::{RootId, RootedError, ValueType};
use std::sync::Arc;
use turso_core::{Connection, LimboError};

use database::Handle;
use statement::Prepared;

/// The two instance kinds a hidden index can name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Database,
    Statement,
}

/// Per-process engine state: every handle a `DatabaseSync` or `StatementSync` made. Entries
/// are not reclaimed when their JavaScript object is collected.
#[derive(Default)]
pub(crate) struct State {
    keys: [Option<RootId>; 2],
    pub(crate) databases: Vec<Handle>,
    pub(crate) statements: Vec<Prepared>,
}

impl State {
    /// Whether no handle has been made, so a startup snapshot holds none of the engine's state.
    pub(crate) fn snapshot_quiescent(&self) -> bool {
        self.databases.is_empty() && self.statements.is_empty() && self.keys.iter().all(Option::is_none)
    }

    /// The open connection of database `database`, if it is open.
    pub(crate) fn connection(&self, database: usize) -> Option<&Arc<Connection>> {
        self.databases.get(database).and_then(|handle| handle.connection.as_ref())
    }
}

/// `node:sqlite`'s exports, in Node's order.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let (statement_class, statement_prototype) = statement::class(c)?;
    let (database_class, _) = database::class(c, statement_prototype)?;
    let exports = c.object_rooted()?;
    put(c, exports, "DatabaseSync", database_class)?;
    put(c, exports, "StatementSync", statement_class)?;
    let constants = constants(c)?;
    put(c, exports, "constants", constants)?;
    let backup = database::unsupported(c, "backup")?;
    put(c, exports, "backup", backup)?;
    Ok(exports)
}

/// Node 22's `constants`: the changeset conflict and operation codes, in its key order.
const CHANGESET_CONSTANTS: [(&str, f64); 8] = [
    ("SQLITE_CHANGESET_OMIT", 0.0),
    ("SQLITE_CHANGESET_REPLACE", 1.0),
    ("SQLITE_CHANGESET_ABORT", 2.0),
    ("SQLITE_CHANGESET_DATA", 1.0),
    ("SQLITE_CHANGESET_NOTFOUND", 2.0),
    ("SQLITE_CHANGESET_CONFLICT", 3.0),
    ("SQLITE_CHANGESET_CONSTRAINT", 4.0),
    ("SQLITE_CHANGESET_FOREIGN_KEY", 5.0),
];

fn constants(c: &mut Context<'_>) -> Outcome {
    let object = c.object_rooted()?;
    for (name, value) in CHANGESET_CONSTANTS {
        let number = c.number(value);
        put(c, object, name, number)?;
    }
    Ok(object)
}

/// Stores each named value as an own enumerable property of `object`.
pub(crate) fn put_pair<const N: usize>(c: &mut Context<'_>, object: RootId, pairs: [(&str, RootId); N]) -> Result<(), RootedError> {
    for (name, value) in pairs {
        put(c, object, name, value)?;
    }
    Ok(())
}

/// Declares the prototype methods of one class: writable and configurable, not enumerable.
pub(crate) fn define_methods<M: Members>(
    c: &mut Context<'_>,
    target: RootId,
    methods: &[(&str, M)],
    captures: &[RootId],
) -> Result<(), RootedError> {
    for &(name, member) in methods {
        let value = function(c, member, name, captures)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(target, key, value, true, false, true)?;
    }
    Ok(())
}

/// Declares the prototype getters of one class.
pub(crate) fn define_accessors<M: Members>(c: &mut Context<'_>, target: RootId, getters: &[(&str, M)]) -> Result<(), RootedError> {
    let getters: Vec<(&str, M, Option<M>)> = getters.iter().map(|&(name, member)| (name, member, None)).collect();
    accessors(c, target, &getters)
}

/// The hidden private symbol that keys `kind`'s index, created once per process.
fn key(c: &mut Context<'_>, kind: Kind) -> Result<RootId, RootedError> {
    let slot = kind as usize;
    if let Some(key) = c.node().shared_state().borrow().sqlite.keys[slot] {
        return Ok(key);
    }
    let symbol = c.private_symbol_rooted(Some("sqlite"));
    let key = c.retain(symbol)?;
    c.node().shared_state().borrow_mut().sqlite.keys[slot] = Some(key);
    Ok(key)
}

/// Records `index` on `object` as the `kind` state it names.
pub(crate) fn tag(c: &mut Context<'_>, object: RootId, kind: Kind, index: usize) -> Result<(), RootedError> {
    let key = key(c, kind)?;
    let value = c.number(index as f64);
    c.set_property_rooted(object, key, value, object).map(drop)
}

/// The index `this` names as a `kind`, or `None` when `this` is not one.
pub(crate) fn tagged(c: &mut Context<'_>, this: RootId, kind: Kind) -> Result<Option<usize>, RootedError> {
    if !c.is_object_rooted(this)? {
        return Ok(None);
    }
    let key = key(c, kind)?;
    let value = c.get_property_rooted(this, key)?;
    Ok(c.rooted_value(value).and_then(|value| value.as_number()).map(|index| index as usize))
}

/// `Symbol.iterator`, the key an iterator's identity method is installed under.
pub(crate) fn iterator_symbol(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    get(c, symbol, "iterator")
}

/// Runs `f` over the process's engine state. `f` must not call into the guest.
pub(crate) fn with_state<T>(c: &mut Context<'_>, f: impl FnOnce(&mut State) -> T) -> T {
    let shared = c.node().shared_state();
    let mut guard = shared.borrow_mut();
    f(&mut guard.sqlite)
}

/// A new object whose prototype is `prototype`, tagged as `kind` number `index`.
pub(crate) fn instance(c: &mut Context<'_>, prototype: RootId, kind: Kind, index: usize) -> Outcome {
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    tag(c, object, kind, index)?;
    Ok(object)
}

/// An engine failure or a Node argument or state error, before it becomes a thrown value.
pub(crate) enum Failure {
    Engine(LimboError),
    /// An `Error` with a Node code.
    Coded(&'static str, String),
    /// A `TypeError` with a Node code.
    Type(&'static str, String),
    /// A `RangeError` with a Node code.
    Range(&'static str, String),
    /// An `ERR_SQLITE_ERROR` with the engine's result code and message.
    Sqlite(i32, String),
}

impl From<LimboError> for Failure {
    fn from(error: LimboError) -> Self {
        Failure::Engine(error)
    }
}

/// Throws `failure` as the JavaScript error Node throws for it.
pub(crate) fn throw(c: &mut Context<'_>, failure: Failure) -> RootedError {
    match failure {
        Failure::Engine(error) => values::engine_error(c, &error),
        Failure::Coded(code, message) => throw_coded_error(c, code, &message),
        Failure::Type(code, message) => throw_coded_type_error(c, code, &message),
        Failure::Range(code, message) => values::range_error(c, code, &message),
        Failure::Sqlite(code, message) => values::sqlite_error(c, &message, code),
    }
}

/// `ERR_INVALID_THIS` for a method called on something that is not a `class` instance.
pub(crate) fn invalid_this(c: &mut Context<'_>, class: &'static str) -> RootedError {
    let message = format!("Value of \"this\" must be of type {class}");
    throw(c, Failure::Type("ERR_INVALID_THIS", message))
}

/// The string `value` holds, or `ERR_INVALID_ARG_TYPE` naming `name`.
pub(crate) fn string_argument(c: &mut Context<'_>, value: RootId, name: &str) -> Result<String, RootedError> {
    match c.string_text(value)? {
        Some(text) => Ok(text),
        None => Err(crate::modules::util_shared_vm::argument_type_error(c, value, name, "string")?),
    }
}

/// The boolean `value` holds, or `ERR_INVALID_ARG_TYPE` naming `name`.
pub(crate) fn boolean_argument(c: &mut Context<'_>, value: RootId, name: &str) -> Result<bool, RootedError> {
    if c.value_type_rooted(value) != Some(ValueType::Boolean) {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, value, name, "boolean")?);
    }
    c.truthy_rooted(value)
}

/// The new target of a class constructor, or the `TypeError` for a call without `new`.
pub(crate) fn require_new(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    match c.new_target_rooted() {
        Some(target) => Ok(target),
        None => {
            let message = format!("Class constructor {name} cannot be invoked without 'new'");
            Err(throw(c, Failure::Type("ERR_CONSTRUCT_CALL_REQUIRED", message)))
        }
    }
}
