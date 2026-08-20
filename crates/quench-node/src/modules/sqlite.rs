//! Small, synchronous SQLite backend for the `node:sqlite` surface.
//! The connection is shared by prepared statements so `prepare` remains useful
use rusqlite::{params_from_iter, types::Value as SqlValue, Connection};
use std::{cell::RefCell, collections::BTreeMap, path::Path, rc::Rc};

#[derive(Clone, Debug, PartialEq)]
pub enum Value { Null, Integer(i64), Real(f64), Text(String), Blob(Vec<u8>) }
impl From<Value> for SqlValue { fn from(v: Value) -> Self { match v { Value::Null=>SqlValue::Null, Value::Integer(x)=>SqlValue::Integer(x), Value::Real(x)=>SqlValue::Real(x), Value::Text(x)=>SqlValue::Text(x), Value::Blob(x)=>SqlValue::Blob(x) } } }

#[derive(Debug)]
pub struct DatabaseError(pub String);
impl From<rusqlite::Error> for DatabaseError { fn from(e: rusqlite::Error) -> Self { Self(e.to_string()) } }
pub type Result<T, E = DatabaseError> = std::result::Result<T, E>;
#[derive(Clone)]
pub struct DatabaseSync { conn: Rc<RefCell<Option<Connection>>> }
pub struct PreparedStatement { conn: Rc<RefCell<Option<Connection>>>, sql: String }
#[derive(Clone, Debug, PartialEq)]
pub struct Row { pub values: BTreeMap<String, Value> }

impl DatabaseSync {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> { Ok(Self { conn: Rc::new(RefCell::new(Some(Connection::open(path)?))) }) }
    pub fn memory() -> Result<Self> { Ok(Self { conn: Rc::new(RefCell::new(Some(Connection::open_in_memory()?))) }) }
    pub fn exec(&self, sql: &str) -> Result<usize> { let conn=self.conn.borrow(); Ok(conn.as_ref().ok_or_else(|| DatabaseError("database is closed".into()))?.execute_batch(sql).map(|_| 0)?) }
    pub fn prepare(&self, sql: impl Into<String>) -> Result<PreparedStatement> { if self.conn.borrow().is_none() { return Err(DatabaseError("database is closed".into())); } Ok(PreparedStatement { conn:self.conn.clone(), sql:sql.into() }) }
    pub fn close(&self) -> Result<()> { let mut slot=self.conn.borrow_mut(); if let Some(conn)=slot.take() { conn.close().map_err(|(_,e)| DatabaseError(e.to_string()))?; } Ok(()) }
}
impl PreparedStatement {
    pub fn run(&self, args: &[Value]) -> Result<usize> { let conn=self.conn.borrow(); let c=conn.as_ref().ok_or_else(|| DatabaseError("database is closed".into()))?; let vals:Vec<SqlValue>=args.iter().cloned().map(Into::into).collect(); Ok(c.execute(&self.sql, params_from_iter(vals.iter()))?) }
    pub fn all(&self, args: &[Value]) -> Result<Vec<Row>> { let conn=self.conn.borrow(); let c=conn.as_ref().ok_or_else(|| DatabaseError("database is closed".into()))?; let vals:Vec<SqlValue>=args.iter().cloned().map(Into::into).collect(); let mut stmt=c.prepare(&self.sql)?; let names:Vec<String>=stmt.column_names().iter().map(|s|s.to_string()).collect(); let rows=stmt.query_map(params_from_iter(vals.iter()), |r| { let mut values=BTreeMap::new(); for (i,n) in names.iter().enumerate() { let v:SqlValue=r.get(i)?; values.insert(n.clone(), match v { SqlValue::Null=>Value::Null, SqlValue::Integer(x)=>Value::Integer(x), SqlValue::Real(x)=>Value::Real(x), SqlValue::Text(x)=>Value::Text(x), SqlValue::Blob(x)=>Value::Blob(x) }); } Ok(Row{values}) })?; Ok(rows.collect::<std::result::Result<Vec<_>,_>>()?) }
}

use std::sync::atomic::{AtomicU64, Ordering};
use quench_runtime::{execute, host_api, value::Value as JsValue, execute::VmError};
use crate::host::HostState;
use crate::registry::{SPEC_SQLITE_ALL, SPEC_SQLITE_CLOSE, SPEC_SQLITE_EXEC, SPEC_SQLITE_PREPARE, SPEC_SQLITE_RUN};

thread_local! {
    static DATABASES: RefCell<BTreeMap<u64, DatabaseSync>> = RefCell::new(BTreeMap::new());
    static STATEMENTS: RefCell<BTreeMap<u64, PreparedStatement>> = RefCell::new(BTreeMap::new());
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
const ID: &str = "\0sqlite-id";

fn object_id(value: &JsValue) -> Option<u64> {
    match execute::get_property(value, ID) { JsValue::Number(n) if n >= 0.0 => Some(n as u64), _ => None }
}
fn db_error(error: DatabaseError) -> VmError { VmError::EvalError(error.0) }
fn argument(value: &JsValue) -> Value {
    match value {
        JsValue::Null | JsValue::Undefined => Value::Null,
        JsValue::Boolean(v) => Value::Integer(i64::from(*v)),
        JsValue::Number(v) if v.fract() == 0.0 => Value::Integer(*v as i64),
        JsValue::Number(v) => Value::Real(*v),
        JsValue::String(v) => Value::Text(v.clone()),
        JsValue::StringUnits(_) => Value::Text(execute::to_js_string(value).unwrap_or_default()),
        _ => Value::Null,
    }
}
pub fn construct(_: &Rc<RefCell<HostState>>, args: &[JsValue]) -> Result<JsValue, VmError> {
    let path = args.first().and_then(|v| execute::to_js_string(v).ok()).unwrap_or_else(|| ":memory:".into());
    let db = if path == ":memory:" { DatabaseSync::memory() } else { DatabaseSync::open(path) }.map_err(db_error)?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    DATABASES.with(|dbs| { dbs.borrow_mut().insert(id, db); });
    let mut object = host_api::object(vec![]);
    object = execute::set_property(object, ID, JsValue::Number(id as f64));
    for (name, spec) in [
        ("exec", SPEC_SQLITE_EXEC),
        ("prepare", SPEC_SQLITE_PREPARE),
        ("close", SPEC_SQLITE_CLOSE),
    ] {
        object = execute::set_property(object, name, crate::host::capability(spec));
    }
    Ok(object)
}
pub fn exec(_: &Rc<RefCell<HostState>>, receiver: Option<&JsValue>, args: &[JsValue]) -> Result<JsValue, VmError> {
    let id = object_id(receiver.ok_or(VmError::NotCallable)?).ok_or(VmError::NotCallable)?;
    let sql = execute::to_js_string(args.first().ok_or(VmError::NotCallable)?).map_err(|_| VmError::NotCallable)?;
    DATABASES.with(|dbs| dbs.borrow().get(&id).ok_or(VmError::NotCallable)?.exec(&sql).map(|_| JsValue::Undefined).map_err(db_error))
}
pub fn prepare(_: &Rc<RefCell<HostState>>, receiver: Option<&JsValue>, args: &[JsValue]) -> Result<JsValue, VmError> {
    let id = object_id(receiver.ok_or(VmError::NotCallable)?).ok_or(VmError::NotCallable)?;
    let sql = execute::to_js_string(args.first().ok_or(VmError::NotCallable)?).map_err(|_| VmError::NotCallable)?;
    let statement = DATABASES.with(|dbs| dbs.borrow().get(&id).ok_or(VmError::NotCallable)?.prepare(sql).map_err(db_error))?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    STATEMENTS.with(|statements| { statements.borrow_mut().insert(id, statement); });
    let mut object = host_api::object(vec![]);
    object = execute::set_property(object, ID, JsValue::Number(id as f64));
    object = execute::set_property(object, "run", crate::host::capability(SPEC_SQLITE_RUN));
    object = execute::set_property(object, "all", crate::host::capability(SPEC_SQLITE_ALL));
    Ok(object)
}
fn statement_args(args: &[JsValue]) -> Vec<Value> { args.iter().map(argument).collect() }
pub fn run(_: &Rc<RefCell<HostState>>, receiver: Option<&JsValue>, args: &[JsValue]) -> Result<JsValue, VmError> {
    let id = object_id(receiver.ok_or(VmError::NotCallable)?).ok_or(VmError::NotCallable)?;
    STATEMENTS.with(|statements| statements.borrow().get(&id).ok_or(VmError::NotCallable)?.run(&statement_args(args)).map(|n| JsValue::Number(n as f64)).map_err(db_error))
}
pub fn all(_: &Rc<RefCell<HostState>>, receiver: Option<&JsValue>, args: &[JsValue]) -> Result<JsValue, VmError> {
    let id = object_id(receiver.ok_or(VmError::NotCallable)?).ok_or(VmError::NotCallable)?;
    let rows = STATEMENTS.with(|statements| statements.borrow().get(&id).ok_or(VmError::NotCallable)?.all(&statement_args(args)).map_err(db_error))?;
    Ok(host_api::array(rows.into_iter().map(|row| host_api::object(row.values.into_iter().map(|(name, value)| (name, match value { Value::Null => JsValue::Null, Value::Integer(n) => JsValue::Number(n as f64), Value::Real(n) => JsValue::Number(n), Value::Text(s) => JsValue::String(s), Value::Blob(_) => JsValue::Undefined })).collect())).collect()))
}
pub fn close(_: &Rc<RefCell<HostState>>, receiver: Option<&JsValue>, _: &[JsValue]) -> Result<JsValue, VmError> {
    let id = object_id(receiver.ok_or(VmError::NotCallable)?).ok_or(VmError::NotCallable)?;
    DATABASES.with(|dbs| dbs.borrow().get(&id).ok_or(VmError::NotCallable)?.close().map(|_| JsValue::Undefined).map_err(db_error))
}

#[cfg(test)]
mod tests { use super::*; #[test] fn database_sync_fixture() { let db=DatabaseSync::memory().unwrap(); db.exec("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO users(name) VALUES ('Ada');").unwrap(); let q=db.prepare("SELECT id,name FROM users WHERE id=?1").unwrap(); let rows=q.all(&[Value::Integer(1)]).unwrap(); assert_eq!(rows[0].values["name"], Value::Text("Ada".into())); assert!(q.run(&[]).is_err()); db.close().unwrap(); assert!(db.exec("SELECT 1").is_err()); } }
