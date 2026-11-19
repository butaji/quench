//! `DatabaseSync`: one engine database, opened on demand, and the handles it prepares.
use super::{define_accessors, define_methods, instance, invalid_this, string_argument, tagged, throw, with_state, Failure, Kind};
use crate::modules::host_members::{captured, constructor, current, function, leading, members};
use crate::modules::stream_shared_vm::{arg, get, Context, Outcome};
use quench_runtime::{RootId, RootedError, ValueType};
use std::sync::Arc;
use std::time::Duration;
use turso_core::{ConnectOpts, Connection, Database, Dialect, LimboError, MemoryIO, OpenFlags, OpenOptions, PlatformIO, IO};

members!(Member, "sqliteDatabase", [
    /// `new DatabaseSync(path, options)`.
    Construct,
    /// `open()`.
    Open,
    /// `close()`.
    Close,
    /// `exec(sql)`.
    Exec,
    /// `prepare(sql)`, which captures the `StatementSync` prototype.
    Prepare,
    /// The `isOpen` getter.
    IsOpen,
    /// The `isTransaction` getter.
    IsTransaction,
    /// A feature this engine lacks; the captured label names it.
    Unsupported,
]);

/// The methods a `DatabaseSync` prototype carries, in Node's order.
const METHODS: [(&str, Member); 4] = [
    ("open", Member::Open),
    ("close", Member::Close),
    ("exec", Member::Exec),
    ("prepare", Member::Prepare),
];

/// The prototype methods this engine does not provide.
const UNSUPPORTED: [&str; 6] = ["createSession", "applyChangeset", "loadExtension", "enableLoadExtension", "function", "aggregate"];

/// The options a handle is opened with.
#[derive(Clone, Copy)]
pub(crate) struct Options {
    pub(crate) open: bool,
    pub(crate) read_only: bool,
    pub(crate) foreign_keys: bool,
    pub(crate) double_quoted: bool,
    pub(crate) timeout_ms: u64,
}

/// One database: its storage, its options, and its connection while it is open.
pub(crate) struct Handle {
    path: String,
    io: Arc<dyn IO>,
    options: Options,
    pub(crate) connection: Option<Arc<Connection>>,
}

impl Handle {
    /// The storage this handle's connections read and write.
    pub(crate) fn io(&self) -> Arc<dyn IO> {
        self.io.clone()
    }
}

/// The dialect a `DatabaseSync` opens its database in: the engine's SQLite parser.
struct SqliteDialect;

impl Dialect for SqliteDialect {
    fn name(&self) -> &'static str {
        "sqlite"
    }
}

/// The `DatabaseSync` class and its prototype. `statement_prototype` is what `prepare` gives
/// its statements.
pub(crate) fn class(c: &mut Context<'_>, statement_prototype: RootId) -> Result<(RootId, RootId), RootedError> {
    let class = constructor(c, Member::Construct, "DatabaseSync")?;
    let prototype = get(c, class, "prototype")?;
    for &(name, member) in &METHODS {
        let captures = if member == Member::Prepare { vec![statement_prototype] } else { Vec::new() };
        define_methods(c, prototype, &[(name, member)], &captures)?;
    }
    define_accessors(c, prototype, &[("isOpen", Member::IsOpen), ("isTransaction", Member::IsTransaction)])?;
    for name in UNSUPPORTED {
        let label = c.string_rooted(name);
        define_methods(c, prototype, &[(name, Member::Unsupported)], &[label])?;
    }
    Ok((class, prototype))
}

/// A function of a feature this engine does not provide: it throws, never succeeds.
pub(crate) fn unsupported(c: &mut Context<'_>, name: &str) -> Outcome {
    let label = c.string_rooted(name);
    function(c, Member::Unsupported, name, &[label])
}

/// Dispatches a call of any `DatabaseSync` member.
pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    match current::<Member>(c)? {
        Member::Construct => construct(c, args),
        Member::Open => open_method(c, this),
        Member::Close => close(c, this),
        Member::Exec => exec(c, this, args),
        Member::Prepare => prepare(c, this, args),
        Member::IsOpen => {
            let index = database_index(c, this)?;
            let open = with_state(c, |state| state.databases[index].connection.is_some());
            Ok(c.boolean(open))
        }
        Member::IsTransaction => {
            let index = database_index(c, this)?;
            let connection = open_connection(c, index)?;
            Ok(c.boolean(!connection.get_auto_commit()))
        }
        Member::Unsupported => {
            let [label] = captured::<1>(c)?;
            let name = c.string_text(label)?.unwrap_or_else(|| "this feature".to_owned());
            let message = format!("{name} is not supported by this SQLite engine");
            Err(throw(c, Failure::Coded("ERR_INVALID_STATE", message)))
        }
    }
}

/// `new DatabaseSync(path, options)`.
fn construct(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let target = super::require_new(c, "DatabaseSync")?;
    let path = arg(c, args, 0);
    let path = string_argument(c, path, "path")?;
    let options = arg(c, args, 1);
    let options = parse_options(c, options)?;
    let handle = new_handle(path, options).map_err(|failure| throw(c, failure))?;
    let index = with_state(c, |state| {
        state.databases.push(handle);
        state.databases.len() - 1
    });
    if options.open {
        open_index(c, index)?;
    }
    let prototype = get(c, target, "prototype")?;
    instance(c, prototype, Kind::Database, index)
}

/// A handle over `path`'s storage: memory for `:memory:`, the platform's files otherwise.
fn new_handle(path: String, options: Options) -> Result<Handle, Failure> {
    let io: Arc<dyn IO> = if turso_core::is_memory_like(&path) {
        Arc::new(MemoryIO::new())
    } else {
        Arc::new(PlatformIO::new()?)
    };
    Ok(Handle { path, io, options, connection: None })
}

/// The options object's fields, with Node's defaults.
fn parse_options(c: &mut Context<'_>, options: RootId) -> Result<Options, RootedError> {
    let defaults = Options { open: true, read_only: false, foreign_keys: true, double_quoted: false, timeout_ms: 0 };
    if c.value_type_rooted(options).is_none_or(|kind| kind.is_nullish()) {
        return Ok(defaults);
    }
    if !c.is_object_rooted(options)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, options, "options", "object")?);
    }
    Ok(Options {
        open: flag(c, options, "open", defaults.open)?,
        read_only: flag(c, options, "readOnly", defaults.read_only)?,
        foreign_keys: flag(c, options, "enableForeignKeyConstraints", defaults.foreign_keys)?,
        double_quoted: flag(c, options, "enableDoubleQuotedStringLiterals", defaults.double_quoted)?,
        timeout_ms: timeout(c, options)?,
    })
}

/// One boolean option, `default` when absent.
fn flag(c: &mut Context<'_>, options: RootId, name: &str, default: bool) -> Result<bool, RootedError> {
    let value = get(c, options, name)?;
    if c.value_type_rooted(value).is_none_or(|kind| kind.is_nullish()) {
        return Ok(default);
    }
    super::boolean_argument(c, value, &format!("options.{name}"))
}

/// The `timeout` option in milliseconds, 0 when absent.
fn timeout(c: &mut Context<'_>, options: RootId) -> Result<u64, RootedError> {
    let value = get(c, options, "timeout")?;
    if c.value_type_rooted(value).is_none_or(|kind| kind.is_nullish()) {
        return Ok(0);
    }
    if c.value_type_rooted(value) != Some(ValueType::Number) {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, value, "options.timeout", "number")?);
    }
    let milliseconds = c.to_number(value)?;
    Ok(milliseconds.max(0.0) as u64)
}

/// Opens the handle at `index`, failing when it is already open.
fn open_index(c: &mut Context<'_>, index: usize) -> Result<(), RootedError> {
    let result = with_state(c, |state| -> Result<(), Failure> {
        let handle = &mut state.databases[index];
        if handle.connection.is_some() {
            return Err(Failure::Coded("ERR_INVALID_STATE", "database is already open".to_owned()));
        }
        handle.connection = Some(open_connection_for(handle)?);
        Ok(())
    });
    result.map_err(|failure| throw(c, failure))
}

/// A new connection to the handle's storage, configured as its options say.
fn open_connection_for(handle: &Handle) -> Result<Arc<Connection>, LimboError> {
    let flags = if handle.options.read_only { OpenFlags::ReadOnly } else { OpenFlags::Create };
    let options = OpenOptions::new(Arc::new(SqliteDialect)).flags(flags);
    let database = Database::open(handle.io.clone(), &handle.path, options)?;
    let connection = database.connect(ConnectOpts::new())?;
    connection.set_foreign_keys_enabled(handle.options.foreign_keys);
    connection.set_dqs_dml(handle.options.double_quoted);
    connection.set_busy_timeout(Duration::from_millis(handle.options.timeout_ms));
    Ok(connection)
}

fn open_method(c: &mut Context<'_>, this: RootId) -> Outcome {
    let index = database_index(c, this)?;
    open_index(c, index)?;
    Ok(c.undefined())
}

fn close(c: &mut Context<'_>, this: RootId) -> Outcome {
    let index = database_index(c, this)?;
    let result = with_state(c, |state| -> Result<(), Failure> {
        let handle = &mut state.databases[index];
        let connection = handle.connection.take().ok_or_else(not_open)?;
        connection.close()?;
        Ok(())
    });
    result.map_err(|failure| throw(c, failure))?;
    Ok(c.undefined())
}

fn exec(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let index = database_index(c, this)?;
    let ([sql], _) = leading::<1>(c, args);
    let sql = string_argument(c, sql, "sql")?;
    let result = with_state(c, |state| -> Result<(), Failure> {
        let connection = state.connection(index).ok_or_else(not_open)?.clone();
        if sql.trim().is_empty() {
            return Ok(());
        }
        connection.prepare_execute_batch(&sql)?;
        Ok(())
    });
    result.map_err(|failure| throw(c, failure))?;
    Ok(c.undefined())
}

fn prepare(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let index = database_index(c, this)?;
    let [statement_prototype] = captured::<1>(c)?;
    let ([sql], _) = leading::<1>(c, args);
    let sql = string_argument(c, sql, "sql")?;
    let result = with_state(c, |state| -> Result<usize, Failure> {
        let connection = state.connection(index).ok_or_else(not_open)?.clone();
        let stmt = connection.prepare(&sql)?;
        state.statements.push(super::statement::Prepared::new(index, sql, stmt));
        Ok(state.statements.len() - 1)
    });
    let statement = result.map_err(|failure| throw(c, failure))?;
    instance(c, statement_prototype, Kind::Statement, statement)
}

/// The database index `this` names, or `ERR_INVALID_THIS`.
fn database_index(c: &mut Context<'_>, this: RootId) -> Result<usize, RootedError> {
    match tagged(c, this, Kind::Database)? {
        Some(index) => Ok(index),
        None => Err(invalid_this(c, "DatabaseSync")),
    }
}

/// The open connection of database `index`, or `ERR_INVALID_STATE`.
pub(crate) fn open_connection(c: &mut Context<'_>, index: usize) -> Result<Arc<Connection>, RootedError> {
    let connection = with_state(c, |state| state.connection(index).cloned());
    connection.ok_or_else(|| throw(c, not_open()))
}

/// The error for a closed database.
pub(crate) fn not_open() -> Failure {
    Failure::Coded("ERR_INVALID_STATE", "database is not open".to_owned())
}
