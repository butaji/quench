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

#[cfg(test)]
mod tests { use super::*; #[test] fn database_sync_fixture() { let db=DatabaseSync::memory().unwrap(); db.exec("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO users(name) VALUES ('Ada');").unwrap(); let q=db.prepare("SELECT id,name FROM users WHERE id=?1").unwrap(); let rows=q.all(&[Value::Integer(1)]).unwrap(); assert_eq!(rows[0].values["name"], Value::Text("Ada".into())); assert!(q.run(&[]).is_err()); db.close().unwrap(); assert!(db.exec("SELECT 1").is_err()); } }
