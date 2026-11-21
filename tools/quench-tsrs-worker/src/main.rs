//! Quench's TypeScript checker worker. It answers, for each function of a file, the language
//! types (Quench `ValueType` bits) the checker gives its parameters: aliases, enums, contextual
//! callback parameters and inferred types the syntax alone does not name. Quench keeps them as
//! declared-parameter facts, which tiers check once at entry, so a stale or wrong answer costs
//! a fallback, never a wrong result.
//!
//! Protocol (one JSON object per line on stdin, one reply per line on stdout):
//! `{"id", "path", "sites": [{"key", "positions_utf16": [..]}], "out"?}` ->
//! `{"id", "protocol_version", "typescript_version", "tsrs_revision", "sites": [{"key",
//! "languages": [..]}], "error"}`. With `out`, the worker also writes the facts file there
//! (atomically): a `FACTS_MAGIC` header line, then `key l0 l1 ..` per site.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use tsrs_api::{Handler, Response, Session, SessionOptions};
use tsrs_checker::TypeFlags as F;

#[path = "../../../crates/quench/src/host/ts_checker/protocol.rs"]
#[allow(dead_code)]
mod protocol;
use protocol::*;

#[derive(Deserialize)]
struct Request {
    id: u64,
    path: String,
    sites: Vec<Site>,
    #[serde(default)]
    out: Option<String>,
}

#[derive(Deserialize)]
struct Site {
    key: u32,
    positions_utf16: Vec<u32>,
}

#[derive(Serialize)]
struct Answer {
    key: u32,
    languages: Vec<u16>,
}

#[derive(Serialize)]
struct Reply {
    id: u64,
    protocol_version: u32,
    typescript_version: &'static str,
    tsrs_revision: &'static str,
    sites: Vec<Answer>,
    error: Option<String>,
}

fn main() -> io::Result<()> {
    let cwd = std::env::current_dir()?.to_string_lossy().replace('\\', "/");
    let fs: Arc<dyn tsrs_vfs::FS> = Arc::new(tsrs_vfs::bundled::wrap_fs(tsrs_vfs::osvfs::fs()));
    let session = Session::new(SessionOptions::new(cwd, tsrs_vfs::bundled::default_lib_path(), fs, false));
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line?;
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                let id = request.id;
                let result = answer(&session, &request).and_then(|sites| {
                    if let Some(out) = &request.out { write_facts(out, &sites)?; }
                    Ok(sites)
                });
                reply(id, result)
            }
            Err(error) => reply(0, Err(format!("invalid worker request: {error}"))),
        };
        writeln!(stdout, "{}", serde_json::to_string(&reply).expect("a reply serializes"))?;
        stdout.flush()?;
    }
    Ok(())
}

fn reply(id: u64, result: Result<Vec<Answer>, String>) -> Reply {
    let (sites, error) = match result { Ok(sites) => (sites, None), Err(error) => (Vec::new(), Some(error)) };
    Reply { id, protocol_version: PROTOCOL_VERSION, typescript_version: TYPESCRIPT_VERSION, tsrs_revision: TSRS_REVISION, sites, error }
}

fn write_facts(out: &str, sites: &[Answer]) -> Result<(), String> {
    let mut text = format!("{FACTS_MAGIC} {PROTOCOL_VERSION} {TSRS_REVISION}\n");
    for site in sites {
        text.push_str(&site.key.to_string());
        for language in &site.languages { text.push(' '); text.push_str(&language.to_string()); }
        text.push('\n');
    }
    let temporary = format!("{out}.{}.tmp", std::process::id());
    if let Some(parent) = Path::new(out).parent() { std::fs::create_dir_all(parent).map_err(|e| format!("{out}: {e}"))?; }
    std::fs::write(&temporary, text).map_err(|e| format!("{temporary}: {e}"))?;
    std::fs::rename(&temporary, out).map_err(|e| format!("{out}: {e}"))
}

struct Query<'s> {
    session: &'s Session,
    snapshot: u64,
    project: String,
}

fn answer(session: &Session, request: &Request) -> Result<Vec<Answer>, String> {
    let positions: Vec<u32> = request.sites.iter().flat_map(|site| site.positions_utf16.iter().copied()).collect();
    if positions.is_empty() {
        return Ok(request.sites.iter().map(|site| Answer { key: site.key, languages: Vec::new() }).collect());
    }
    let path = Path::new(&request.path).canonicalize().map_err(|e| format!("{}: {e}", request.path))?
        .to_string_lossy().replace('\\', "/");
    let snapshot = call(session, "createSnapshot", json!({ "openFiles": [path] }))?;
    let snapshot = required_u64(&snapshot, "snapshot")
        .and_then(|id| Ok((id, snapshot.pointer("/operation/openedFiles/0/project").and_then(Value::as_str)
            .ok_or("tsrs did not resolve the file to a project")?.to_owned())));
    let (snapshot, project) = snapshot?;
    let query = Query { session, snapshot, project };
    let result = query.languages(&path, &positions).map(|languages| {
        let mut languages = languages.into_iter();
        request.sites.iter().map(|site| Answer {
            key: site.key,
            languages: languages.by_ref().take(site.positions_utf16.len()).collect(),
        }).collect()
    });
    let _ = call(session, "release", json!({ "snapshot": snapshot }));
    result
}

impl Query<'_> {
    fn languages(&self, path: &str, positions: &[u32]) -> Result<Vec<u16>, String> {
        let types = call(self.session, "getTypesAtPositions",
            json!({ "snapshot": self.snapshot, "project": self.project, "file": path, "positions": positions }))?;
        let types = types.as_array().ok_or("tsrs returned a non-array type result")?;
        if types.len() != positions.len() {
            return Err(format!("tsrs returned {} types for {} positions", types.len(), positions.len()));
        }
        Ok(types.iter().map(|ty| self.language(ty, 0)).collect())
    }

    /// The language types a checker type admits at run time; `LANGUAGE_ANY` where it names no
    /// closed set (any, unknown, type parameters, failures).
    fn language(&self, ty: &Value, depth: u32) -> u16 {
        let (Some(id), Some(flags)) = (ty.get("id").and_then(Value::as_u64), ty.get("flags").and_then(Value::as_u64)) else {
            return LANGUAGE_ANY;
        };
        let flags = F::from_bits_truncate(flags as u32);
        if flags.intersects(F::UnionOrIntersection) {
            if depth > 4 { return LANGUAGE_ANY; }
            let Ok(Value::Array(members)) = call(self.session, "getTypesOfType",
                json!({ "snapshot": self.snapshot, "project": self.project, "objectId": id })) else { return LANGUAGE_ANY };
            if members.is_empty() { return LANGUAGE_ANY; }
            let languages = members.iter().map(|member| self.language(member, depth + 1));
            return if flags.intersects(F::Union) {
                languages.fold(0, |set, language| set | language)
            } else {
                // `number & { brand }` is a number: an object-like member narrows nothing.
                languages.fold(LANGUAGE_ANY, |set, language|
                    if language & !(LANGUAGE_OBJECT | LANGUAGE_FUNCTION) == 0 { set } else { set & language })
            };
        }
        let mut set = 0;
        if flags.intersects(F::NumberLike) { set |= LANGUAGE_NUMBER; }
        if flags.intersects(F::StringLike) { set |= LANGUAGE_STRING; }
        if flags.intersects(F::BooleanLike) { set |= LANGUAGE_BOOLEAN; }
        if flags.intersects(F::BigIntLike) { set |= LANGUAGE_BIGINT; }
        if flags.intersects(F::ESSymbolLike) { set |= LANGUAGE_SYMBOL; }
        if flags.intersects(F::VoidLike) { set |= LANGUAGE_UNDEFINED; }
        if flags.intersects(F::Null) { set |= LANGUAGE_NULL; }
        if flags.intersects(F::Object | F::NonPrimitive) { set |= LANGUAGE_OBJECT | LANGUAGE_FUNCTION; }
        if set == 0 || flags.intersects(F::AnyOrUnknown | F::TypeVariable | F::Never | F::Index | F::Conditional | F::Substitution) {
            return LANGUAGE_ANY;
        }
        set
    }
}

fn call(session: &Session, method: &str, params: Value) -> Result<Value, String> {
    let params = serde_json::to_vec(&params).map_err(|error| error.to_string())?;
    match session.handle_request(method, &params) {
        Ok(Response::Json(response)) => serde_json::from_str(&response).map_err(|error| format!("tsrs {method}: {error}")),
        Ok(Response::Binary(_)) => Err(format!("tsrs {method} returned binary data unexpectedly")),
        Err(error) => Err(format!("tsrs {method}: {error}")),
    }
}

fn required_u64(value: &Value, field: &str) -> Result<u64, String> {
    value.get(field).and_then(Value::as_u64).ok_or_else(|| format!("tsrs response omitted numeric `{field}`"))
}
