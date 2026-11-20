//! `StatementSync`: a prepared engine statement and its execution modes.
use super::database::not_open;
use super::values;
use crate::modules::keys::DONE;
use super::{boolean_argument, define_accessors, define_methods, invalid_this, put_pair, tagged, throw, with_state, Failure, Kind, State};
use crate::modules::host_members::{captured, constructor, current, function, leading, members};
use crate::modules::node_errors_shared_vm::throw_coded_error;
use crate::modules::stream_shared_vm::{get, iterator_result, put, Context, Outcome};
use quench_runtime::{RootId, RootedError};
use std::sync::Arc;
use turso_core::{Connection, LimboError, NamespaceId, Statement, StepResult, Value, IO, MAIN_DB_ID};
use turso_parser::ast;

members!(Member, "sqliteStatement", [
    /// `new StatementSync()`, which is never allowed.
    Construct,
    /// `run(...params)`.
    Run,
    /// `get(...params)`.
    Get,
    /// `all(...params)`.
    All,
    /// `iterate(...params)`.
    Iterate,
    /// `columns()`.
    Columns,
    /// The `sourceSQL` getter.
    SourceSql,
    /// The `expandedSQL` getter.
    ExpandedSql,
    /// `setReadBigInts(enabled)`.
    ReadBigInts,
    /// `setAllowBareNamedParameters(enabled)`.
    AllowBareNamed,
    /// `setReturnArrays(enabled)`.
    ReturnArrays,
    /// `next()` of an iterator from `iterate`, which captures its statement.
    IterNext,
    /// `return()` of such an iterator.
    IterReturn,
    /// `[Symbol.iterator]()` of such an iterator.
    IterSelf,
]);

/// An iterator's captures: its statement, and the state object holding `DONE`.
const ITERATOR_CAPTURES: usize = 2;

/// The prototype methods, in Node's order.
const METHODS: [(&str, Member); 8] = [
    ("run", Member::Run),
    ("get", Member::Get),
    ("all", Member::All),
    ("iterate", Member::Iterate),
    ("columns", Member::Columns),
    ("setReadBigInts", Member::ReadBigInts),
    ("setAllowBareNamedParameters", Member::AllowBareNamed),
    ("setReturnArrays", Member::ReturnArrays),
];

/// One prepared statement, with the options its executions read.
pub(crate) struct Prepared {
    pub(crate) database: usize,
    pub(crate) sql: String,
    pub(crate) stmt: Statement,
    read_big_ints: bool,
    pub(crate) allow_bare: bool,
    return_arrays: bool,
    /// Whether an `iterate` cursor is stepping this statement.
    cursor: bool,
}

impl Prepared {
    pub(crate) fn new(database: usize, sql: String, stmt: Statement) -> Self {
        Self { database, sql, stmt, read_big_ints: false, allow_bare: true, return_arrays: false, cursor: false }
    }
}

/// What a method reports after the engine has run.
enum Output {
    Run { changes: i64, last_insert_rowid: i64 },
    Row(Option<Vec<Value>>),
    Rows(Vec<Vec<Value>>),
}

/// The shape a statement's results are built with.
struct Shape {
    names: Vec<String>,
    big: bool,
    arrays: bool,
}

/// The `StatementSync` class and its prototype.
pub(crate) fn class(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let class = constructor(c, Member::Construct, "StatementSync")?;
    let prototype = get(c, class, "prototype")?;
    define_methods(c, prototype, &METHODS, &[])?;
    define_accessors(c, prototype, &[("sourceSQL", Member::SourceSql), ("expandedSQL", Member::ExpandedSql)])?;
    Ok((class, prototype))
}

/// Dispatches a call of any `StatementSync` member.
pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    match current::<Member>(c)? {
        Member::Run => execute(c, this, args, Member::Run),
        Member::Get => execute(c, this, args, Member::Get),
        Member::All => execute(c, this, args, Member::All),
        Member::Iterate => iterate(c, this, args),
        Member::Columns => columns(c, this),
        Member::IterNext => iterator_next(c),
        Member::IterReturn => iterator_return(c),
        Member::IterSelf => Ok(this),
        other => property(c, this, args, other),
    }
}

/// The constructor, the getters and the option setters.
fn property(c: &mut Context<'_>, this: RootId, args: &[RootId], member: Member) -> Outcome {
    match member {
        Member::SourceSql => {
            let index = statement_index(c, this)?;
            let sql = with_state(c, |state| state.statements[index].sql.clone());
            Ok(c.string_rooted(&sql))
        }
        Member::ExpandedSql => {
            let index = statement_index(c, this)?;
            let sql = with_state(c, |state| state.statements[index].stmt.expanded_sql());
            Ok(c.string_rooted(&sql))
        }
        Member::ReadBigInts => set_flag(c, this, args, "readBigInts", |entry, on| entry.read_big_ints = on),
        Member::AllowBareNamed => set_flag(c, this, args, "allowBareNamedParameters", |entry, on| entry.allow_bare = on),
        Member::ReturnArrays => set_flag(c, this, args, "returnArrays", |entry, on| entry.return_arrays = on),
        _ => Err(throw_coded_error(c, "ERR_ILLEGAL_CONSTRUCTOR", "Illegal constructor")),
    }
}

/// The statement index `this` names, or `ERR_INVALID_THIS`.
fn statement_index(c: &mut Context<'_>, this: RootId) -> Result<usize, RootedError> {
    match tagged(c, this, Kind::Statement)? {
        Some(index) => Ok(index),
        None => Err(invalid_this(c, "StatementSync")),
    }
}

/// `run`, `get` or `all`: binds the arguments and executes the statement.
fn execute(c: &mut Context<'_>, this: RootId, args: &[RootId], mode: Member) -> Outcome {
    let index = statement_index(c, this)?;
    let bindings = values::collect(c, args, index)?;
    let result = with_state(c, |state| execute_in(state, index, bindings, mode));
    let (output, shape) = result.map_err(|failure| throw(c, failure))?;
    match output {
        Output::Run { changes, last_insert_rowid } => run_result(c, changes, last_insert_rowid, shape.big),
        Output::Row(Some(row)) => values::row(c, &shape.names, &row, shape.big, shape.arrays),
        Output::Rows(rows) => {
            let mut objects = Vec::with_capacity(rows.len());
            for row in rows {
                objects.push(values::row(c, &shape.names, &row, shape.big, shape.arrays)?);
            }
            c.array_rooted(&objects)
        }
        Output::Row(None) => Ok(c.undefined()),
    }
}

/// Runs the statement in `mode` under the engine's state, resetting it afterwards.
fn execute_in(state: &mut State, index: usize, bindings: Vec<(usize, Value)>, mode: Member) -> Result<(Output, Shape), Failure> {
    let database = state.statements[index].database;
    let connection = state.connection(database).ok_or_else(not_open)?.clone();
    let io = state.databases[database].io();
    let entry = &mut state.statements[index];
    let shape = shape_of(entry);
    if entry.cursor {
        entry.stmt.reset_best_effort();
        entry.cursor = false;
    }
    values::bind(&mut entry.stmt, bindings)?;
    match drive(&io, &mut entry.stmt, mode, &connection) {
        Ok(output) => Ok((output, shape)),
        Err(error) => {
            entry.stmt.reset_best_effort();
            Err(Failure::Engine(error))
        }
    }
}

fn shape_of(entry: &Prepared) -> Shape {
    let names = (0..entry.stmt.num_columns()).map(|index| entry.stmt.get_column_name(index).into_owned()).collect();
    Shape { names, big: entry.read_big_ints, arrays: entry.return_arrays }
}

/// Steps the statement to completion in `mode`, then resets it.
fn drive(io: &Arc<dyn IO>, stmt: &mut Statement, mode: Member, connection: &Connection) -> Result<Output, LimboError> {
    let output = match mode {
        Member::Get => Output::Row(if next_row(io, stmt)? { Some(current_values(stmt)) } else { None }),
        Member::All => {
            let mut rows = Vec::new();
            while next_row(io, stmt)? {
                rows.push(current_values(stmt));
            }
            Output::Rows(rows)
        }
        _ => {
            while next_row(io, stmt)? {}
            Output::Run { changes: connection.changes(), last_insert_rowid: connection.last_insert_rowid() }
        }
    };
    stmt.reset()?;
    Ok(output)
}

fn current_values(stmt: &Statement) -> Vec<Value> {
    stmt.row().map(|row| row.get_values().cloned().collect()).unwrap_or_default()
}

/// Steps once: `true` for a row, `false` at the end. Engine IO is driven between steps.
fn next_row(io: &Arc<dyn IO>, stmt: &mut Statement) -> Result<bool, LimboError> {
    loop {
        match stmt.step()? {
            StepResult::Row => return Ok(true),
            StepResult::Done => return Ok(false),
            StepResult::IO | StepResult::Yield | StepResult::Sleep { .. } => io.step()?,
            StepResult::Busy => return Err(LimboError::Busy),
            StepResult::Interrupt => return Err(LimboError::Interrupt),
        }
    }
}

fn run_result(c: &mut Context<'_>, changes: i64, last_insert_rowid: i64, big: bool) -> Outcome {
    let (changes, rowid) = if big {
        (c.bigint_rooted(i128::from(changes)), c.bigint_rooted(i128::from(last_insert_rowid)))
    } else {
        (c.number(changes as f64), c.number(last_insert_rowid as f64))
    };
    let object = c.object_rooted()?;
    put_pair(c, object, [("lastInsertRowid", rowid), ("changes", changes)])?;
    Ok(object)
}

/// `iterate(...params)`: binds now, and steps as the returned iterator is consumed.
fn iterate(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let index = statement_index(c, this)?;
    let bindings = values::collect(c, args, index)?;
    let result = with_state(c, |state| -> Result<(), Failure> {
        let entry = &mut state.statements[index];
        if entry.cursor {
            entry.stmt.reset_best_effort();
            entry.cursor = false;
        }
        values::bind(&mut entry.stmt, bindings)
    });
    result.map_err(|failure| throw(c, failure))?;
    let iterator = c.object_rooted()?;
    let finished = c.object_rooted()?;
    let unfinished = c.boolean(false);
    put(c, finished, DONE, unfinished)?;
    for (name, member) in [("next", Member::IterNext), ("return", Member::IterReturn)] {
        let method = function(c, member, name, &[this, finished])?;
        put(c, iterator, name, method)?;
    }
    let key = super::iterator_symbol(c)?;
    let identity = function(c, Member::IterSelf, "[Symbol.iterator]", &[])?;
    c.define_data_property_rooted(iterator, key, identity, true, false, true)?;
    Ok(iterator)
}

/// The iterator's `next()`: steps the captured statement once, and stays finished once done.
fn iterator_next(c: &mut Context<'_>) -> Outcome {
    let [statement, finished] = captured::<ITERATOR_CAPTURES>(c)?;
    let index = statement_index(c, statement)?;
    if is_finished(c, finished)? {
        return end_of_iteration(c);
    }
    let result = with_state(c, |state| cursor_step(state, index));
    let row = result.map_err(|failure| throw(c, failure))?;
    let Some(values) = row else {
        let done = c.boolean(true);
        put(c, finished, DONE, done)?;
        return end_of_iteration(c);
    };
    let shape = with_state(c, |state| shape_of(&state.statements[index]));
    let row = values::row(c, &shape.names, &values, shape.big, shape.arrays)?;
    iterator_result(c, row, false)
}

/// The `done` property of an iterator's state object.
fn is_finished(c: &mut Context<'_>, finished: RootId) -> Result<bool, RootedError> {
    let done = get(c, finished, DONE)?;
    c.truthy_rooted(done)
}

fn end_of_iteration(c: &mut Context<'_>) -> Outcome {
    let undefined = c.undefined();
    iterator_result(c, undefined, true)
}

/// Steps the cursor of statement `index`: a row, or `None` once it has ended and reset.
fn cursor_step(state: &mut State, index: usize) -> Result<Option<Vec<Value>>, Failure> {
    let database = state.statements[index].database;
    let io = state.databases[database].io();
    let entry = &mut state.statements[index];
    entry.cursor = true;
    match next_row(&io, &mut entry.stmt) {
        Ok(true) => Ok(Some(current_values(&entry.stmt))),
        Ok(false) => {
            entry.cursor = false;
            entry.stmt.reset()?;
            Ok(None)
        }
        Err(error) => {
            entry.cursor = false;
            entry.stmt.reset_best_effort();
            Err(Failure::Engine(error))
        }
    }
}

/// The iterator's `return()`: ends an unfinished cursor.
fn iterator_return(c: &mut Context<'_>) -> Outcome {
    let [statement, finished] = captured::<ITERATOR_CAPTURES>(c)?;
    let index = statement_index(c, statement)?;
    with_state(c, |state| {
        let entry = &mut state.statements[index];
        if entry.cursor {
            entry.stmt.reset_best_effort();
            entry.cursor = false;
        }
    });
    let done = c.boolean(true);
    put(c, finished, DONE, done)?;
    end_of_iteration(c)
}

/// `columns()`: one object per result column, with the parts the engine reports.
fn columns(c: &mut Context<'_>, this: RootId) -> Outcome {
    let index = statement_index(c, this)?;
    let described = with_state(c, |state| {
        let entry = &state.statements[index];
        let connection = state.connection(entry.database).cloned();
        let stmt = &entry.stmt;
        (0..stmt.num_columns())
            .map(|position| {
                let name = stmt.get_column_name(position).into_owned();
                let declared = stmt.get_column_decltype(position);
                (name, declared, origin(connection.as_ref(), stmt, position))
            })
            .collect::<Vec<_>>()
    });
    let mut objects = Vec::with_capacity(described.len());
    for (name, declared, origin) in described {
        let object = c.object_rooted()?;
        let name = c.string_rooted(&name);
        let declared = optional_string(c, declared.as_deref());
        let (column, database, table) = match origin {
            Some((table, column)) => (c.string_rooted(&column), c.string_rooted("main"), c.string_rooted(&table)),
            None => (c.null(), c.null(), c.null()),
        };
        put_pair(c, object, [("column", column), ("database", database), ("name", name), ("table", table), ("type", declared)])?;
        objects.push(object);
    }
    c.array_rooted(&objects)
}

/// The main-database table and column that result column `position` reads straight from, as
/// `columns()` reports them. A subquery or an expression has none, and the engine exposes none.
fn origin(connection: Option<&Arc<Connection>>, stmt: &Statement, position: usize) -> Option<(String, String)> {
    let program = stmt.get_program();
    let ast::Expr::Column { table, column, .. } = &program.result_columns.get(position)?.expr else {
        return None;
    };
    let (_, table_ref) = program.table_references.find_table_by_internal_id(*table)?;
    let table_name = table_ref.get_name();
    if !is_main_table(connection, table_name) {
        return None;
    }
    let column_name = table_ref.get_column_at(*column)?.name.clone()?;
    Some((table_name.to_owned(), column_name))
}

/// Whether `table` is a real table of the main database.
fn is_main_table(connection: Option<&Arc<Connection>>, table: &str) -> bool {
    let Some(connection) = connection else { return false };
    connection.with_schema(MAIN_DB_ID, |schema| schema.get_btree_table(NamespaceId::ROOT, table).is_some())
}

fn optional_string(c: &mut Context<'_>, text: Option<&str>) -> RootId {
    match text {
        Some(text) => c.string_rooted(text),
        None => c.null(),
    }
}

/// `setReadBigInts` and its siblings: validates a boolean and stores it on the statement.
fn set_flag(c: &mut Context<'_>, this: RootId, args: &[RootId], name: &str, store: fn(&mut Prepared, bool)) -> Outcome {
    let index = statement_index(c, this)?;
    let ([value], _) = leading::<1>(c, args);
    let on = boolean_argument(c, value, name)?;
    with_state(c, |state| store(&mut state.statements[index], on));
    Ok(c.undefined())
}
