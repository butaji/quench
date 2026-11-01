use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use tsrs_api::{Handler, Response, Session, SessionOptions};

/// The checker emits `TypeFlags` bits for the pinned TSRS revision.
const TSRS_TYPE_FLAG_NUMBER_LIKE: u64 = tsrs_checker::TypeFlags::NumberLike.bits() as u64;

#[path = "../../../crates/quench-node/src/host/hmr/type_worker_protocol.rs"]
mod protocol;

#[derive(Deserialize)]
struct Request {
    id: u64,
    path: String,
    positions_utf16: Vec<u32>,
}

#[derive(Serialize)]
struct Reply {
    id: u64,
    protocol_version: u32,
    typescript_version: &'static str,
    tsrs_revision: &'static str,
    types: Vec<Option<String>>,
    number_like_types: Vec<bool>,
    error: Option<String>,
}

fn main() -> io::Result<()> {
    let cwd = std::env::current_dir()?
        .to_string_lossy()
        .replace('\\', "/");
    let fs: Arc<dyn tsrs_vfs::FS> = Arc::new(tsrs_vfs::bundled::wrap_fs(tsrs_vfs::osvfs::fs()));
    let session = Session::new(SessionOptions::new(
        cwd,
        tsrs_vfs::bundled::default_lib_path(),
        fs,
        false,
    ));
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());

    for line in stdin.lock().lines() {
        let reply = match line {
            Ok(line) => match serde_json::from_str::<Request>(&line) {
                Ok(request) => handle(&session, request),
                Err(error) => Reply {
                    id: 0,
                    protocol_version: protocol::PROTOCOL_VERSION,
                    typescript_version: protocol::TYPESCRIPT_VERSION,
                    tsrs_revision: protocol::TSRS_REVISION,
                    types: Vec::new(),
                    number_like_types: Vec::new(),
                    error: Some(format!("invalid worker request: {error}")),
                },
            },
            Err(error) => {
                writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&Reply {
                        id: 0,
                        protocol_version: protocol::PROTOCOL_VERSION,
                        typescript_version: protocol::TYPESCRIPT_VERSION,
                        tsrs_revision: protocol::TSRS_REVISION,
                        types: Vec::new(),
                        number_like_types: Vec::new(),
                        error: Some(format!("failed to read worker request: {error}")),
                    })
                    .expect("serializing a worker reply cannot fail")
                )?;
                break;
            }
        };
        writeln!(
            stdout,
            "{}",
            serde_json::to_string(&reply).expect("serializing a worker reply cannot fail")
        )?;
        stdout.flush()?;
    }
    Ok(())
}

fn handle(session: &Session, request: Request) -> Reply {
    match query(session, &request.path, &request.positions_utf16) {
        Ok((types, number_like_types)) => Reply {
            id: request.id,
            protocol_version: protocol::PROTOCOL_VERSION,
            typescript_version: protocol::TYPESCRIPT_VERSION,
            tsrs_revision: protocol::TSRS_REVISION,
            types,
            number_like_types,
            error: None,
        },
        Err(error) => Reply {
            id: request.id,
            protocol_version: protocol::PROTOCOL_VERSION,
            typescript_version: protocol::TYPESCRIPT_VERSION,
            tsrs_revision: protocol::TSRS_REVISION,
            types: Vec::new(),
            number_like_types: Vec::new(),
            error: Some(error),
        },
    }
}

fn query(
    session: &Session,
    path: &str,
    positions_utf16: &[u32],
) -> Result<(Vec<Option<String>>, Vec<bool>), String> {
    if positions_utf16.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let path = Path::new(path)
        .canonicalize()
        .map_err(|error| format!("{path}: {error}"))?
        .to_string_lossy()
        .replace('\\', "/");
    let snapshot = call(session, "createSnapshot", json!({ "openFiles": [path] }))?;
    let snapshot_id = required_u64(&snapshot, "snapshot")?;
    let result = (|| {
        let project = snapshot
            .get("operation")
            .and_then(|value| value.get("openedFiles"))
            .and_then(Value::as_array)
            .and_then(|files| files.first())
            .and_then(|file| file.get("project"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "tsrs did not resolve an opened TypeScript file to a project".to_owned()
            })?;

        query_snapshot(session, snapshot_id, project, &path, positions_utf16)
    })();
    let release = call(session, "release", json!({ "snapshot": snapshot_id }));
    match (result, release) {
        (Ok(types), Ok(_)) => Ok(types),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn query_snapshot(
    session: &Session,
    snapshot: u64,
    project: &str,
    path: &str,
    positions_utf16: &[u32],
) -> Result<(Vec<Option<String>>, Vec<bool>), String> {
    let types = call(
        session,
        "getTypesAtPositions",
        json!({
            "snapshot": snapshot,
            "project": project,
            "file": path,
            "positions": positions_utf16,
        }),
    )?;
    let types = types
        .as_array()
        .ok_or_else(|| "tsrs returned a non-array type result".to_owned())?;
    if types.len() != positions_utf16.len() {
        return Err(format!(
            "tsrs returned {} types for {} positions",
            types.len(),
            positions_utf16.len()
        ));
    }
    let mut displays = Vec::with_capacity(types.len());
    let mut number_like_types = Vec::with_capacity(types.len());
    for ty in types {
        if ty.is_null() {
            displays.push(None);
            number_like_types.push(false);
            continue;
        }
        let type_id = required_u64(ty, "id")?;
        let flags = ty
            .get("flags")
            .and_then(Value::as_u64)
            .ok_or_else(|| "tsrs type response omitted numeric `flags`".to_owned())?;
        let display = call(
            session,
            "typeToString",
            json!({ "snapshot": snapshot, "project": project, "type": type_id }),
        )?
        .as_str()
        .map(|text| Some(text.to_owned()))
        .ok_or_else(|| "tsrs returned a non-string type display".to_owned())?;
        displays.push(display);
        number_like_types.push(flags & TSRS_TYPE_FLAG_NUMBER_LIKE != 0);
    }
    Ok((displays, number_like_types))
}

fn call(session: &Session, method: &str, params: Value) -> Result<Value, String> {
    let params = serde_json::to_vec(&params).map_err(|error| error.to_string())?;
    match session.handle_request(method, &params) {
        Ok(Response::Json(response)) => {
            serde_json::from_str(&response).map_err(|error| format!("tsrs {method}: {error}"))
        }
        Ok(Response::Binary(_)) => Err(format!("tsrs {method} returned binary data unexpectedly")),
        Err(error) => Err(format!("tsrs {method}: {error}")),
    }
}

fn required_u64(value: &Value, field: &str) -> Result<u64, String> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("tsrs response omitted numeric `{field}`"))
}
