use crate::host::shared_vm::source_transform::TypeQueryPosition;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};

#[path = "type_worker_protocol.rs"]
mod protocol;

/// Bounds simultaneous checker requests and the pipe backlog; callers queue the rest.
const MAX_IN_FLIGHT_TYPE_QUERIES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TypeWorkerFact {
    pub(crate) function_start: u32,
    pub(crate) parameter_index: u32,
    pub(crate) type_text: String,
    pub(crate) is_number_like: bool,
}

pub(crate) struct TypeWorkerUpdate {
    pub(crate) path: PathBuf,
    pub(crate) source_name: String,
    pub(crate) function_start_offset: u32,
    pub(crate) source_digest: crate::host::hmr::FileContentDigest,
    pub(crate) facts: Vec<TypeWorkerFact>,
}

pub(crate) struct TypeQueryRequest {
    pub(crate) path: PathBuf,
    pub(crate) source_name: String,
    pub(crate) function_start_offset: u32,
    pub(crate) positions: Vec<TypeQueryPosition>,
}

pub(crate) struct TypeWorker {
    input: Option<BufWriter<ChildStdin>>,
    child: Child,
    reader: Option<JoinHandle<()>>,
    responses: Receiver<Result<Response, String>>,
    next_id: u64,
    pending: FxHashMap<u64, PendingQuery>,
    latest: FxHashMap<PathBuf, u64>,
}

struct PendingQuery {
    path: PathBuf,
    source_name: String,
    function_start_offset: u32,
    positions: Vec<TypeQueryPosition>,
}

#[derive(Serialize)]
struct Request {
    id: u64,
    path: String,
    positions_utf16: Vec<u32>,
}

#[derive(Deserialize)]
struct Response {
    id: u64,
    protocol_version: u32,
    typescript_version: String,
    tsrs_revision: String,
    types: Vec<Option<String>>,
    number_like_types: Vec<bool>,
    error: Option<String>,
}

impl TypeWorker {
    pub(crate) fn start() -> Result<Self, String> {
        let executable = worker_executable()
            .ok_or_else(|| {
                "quench-tsrs-worker was not found; run tools/build-quench-node-hmr.sh from the Quench repository or set QUENCH_TSRS_WORKER".to_owned()
            })?;
        let mut child = Command::new(&executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| format!("{}: {error}", executable.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "quench-tsrs-worker stdin was not piped".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "quench-tsrs-worker stdout was not piped".to_owned())?;
        let (sender, responses) = mpsc::channel();
        let reader = thread::Builder::new()
            .name("quench-tsrs-reader".to_owned())
            .spawn(move || read_responses(stdout, sender))
            .map_err(|error| format!("failed to start tsrs response reader: {error}"))?;
        Ok(Self {
            input: Some(BufWriter::new(stdin)),
            child,
            reader: Some(reader),
            responses,
            next_id: 1,
            pending: FxHashMap::default(),
            latest: FxHashMap::default(),
        })
    }

    pub(crate) fn submit(
        &mut self,
        path: &Path,
        source_name: &str,
        function_start_offset: u32,
        positions: &[TypeQueryPosition],
    ) -> Result<(), String> {
        let path = path.to_path_buf();
        self.latest.remove(&path);
        if positions.is_empty() {
            return Ok(());
        }
        let source_digest = positions[0].source_digest;
        if positions
            .iter()
            .any(|position| position.source_digest != source_digest)
        {
            return Err("HMR type-query positions contain multiple source versions".to_owned());
        }
        if !self.has_capacity() {
            return Err(format!(
                "checker queue reached {MAX_IN_FLIGHT_TYPE_QUERIES} in-flight requests"
            ));
        }
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| "HMR type-query request id exhausted".to_owned())?;
        let request = Request {
            id,
            path: path.to_string_lossy().replace('\\', "/"),
            positions_utf16: positions
                .iter()
                .map(|position| position.source_position_utf16)
                .collect(),
        };
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| "quench-tsrs-worker stdin is closed".to_owned())?;
        serde_json::to_writer(&mut *input, &request).map_err(|error| error.to_string())?;
        input
            .write_all(b"\n")
            .and_then(|()| input.flush())
            .map_err(|error| format!("failed to send HMR type query: {error}"))?;
        self.pending.insert(
            id,
            PendingQuery {
                path: path.clone(),
                source_name: source_name.to_owned(),
                function_start_offset,
                positions: positions.to_vec(),
            },
        );
        self.latest.insert(path, id);
        Ok(())
    }

    pub(crate) fn has_capacity(&self) -> bool {
        self.pending.len() < MAX_IN_FLIGHT_TYPE_QUERIES
    }

    pub(crate) fn invalidate(&mut self, path: &Path) {
        self.latest.remove(path);
    }

    pub(crate) fn poll(&mut self) -> Vec<Result<TypeWorkerUpdate, String>> {
        let mut updates = Vec::new();
        loop {
            match self.responses.try_recv() {
                Ok(Ok(response)) => self.accept_response(response, &mut updates),
                Ok(Err(error)) => updates.push(Err(error)),
                Err(TryRecvError::Empty) => return updates,
                Err(TryRecvError::Disconnected) => {
                    updates.push(Err("quench-tsrs-worker exited".to_owned()));
                    return updates;
                }
            }
        }
    }

    fn accept_response(
        &mut self,
        response: Response,
        updates: &mut Vec<Result<TypeWorkerUpdate, String>>,
    ) {
        let Some(query) = self.pending.remove(&response.id) else {
            return;
        };
        if self.latest.get(&query.path) != Some(&response.id) {
            return;
        }
        self.latest.remove(&query.path);
        if response.protocol_version != protocol::PROTOCOL_VERSION
            || response.typescript_version != protocol::TYPESCRIPT_VERSION
            || response.tsrs_revision != protocol::TSRS_REVISION
        {
            updates.push(Err(format!(
                "quench-tsrs-worker version mismatch: protocol {}, TypeScript {}, TSRS {}; expected protocol {}, TypeScript {}, TSRS {}",
                response.protocol_version,
                response.typescript_version,
                response.tsrs_revision,
                protocol::PROTOCOL_VERSION,
                protocol::TYPESCRIPT_VERSION,
                protocol::TSRS_REVISION
            )));
            return;
        }
        if let Some(error) = response.error {
            updates.push(Err(error));
            return;
        }
        if response.types.len() != query.positions.len()
            || response.number_like_types.len() != query.positions.len()
        {
            updates.push(Err(format!(
                "tsrs returned {} display facts and {} numeric facts for {} source positions",
                response.types.len(),
                response.number_like_types.len(),
                query.positions.len()
            )));
            return;
        }
        let source_digest = query.positions[0].source_digest;
        let facts = query
            .positions
            .into_iter()
            .zip(response.types.into_iter().zip(response.number_like_types))
            .filter_map(|(position, (type_text, is_number_like))| {
                type_text.map(|type_text| TypeWorkerFact {
                    function_start: position.function_start,
                    parameter_index: position.parameter_index,
                    type_text,
                    is_number_like,
                })
            })
            .collect();
        updates.push(Ok(TypeWorkerUpdate {
            path: query.path,
            source_name: query.source_name,
            function_start_offset: query.function_start_offset,
            source_digest,
            facts,
        }));
    }
}

impl Drop for TypeWorker {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_responses(stdout: impl io::Read, sender: mpsc::Sender<Result<Response, String>>) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {
                let response = serde_json::from_str(&line)
                    .map_err(|error| format!("invalid quench-tsrs-worker response: {error}"));
                if sender.send(response).is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(format!("failed to read quench-tsrs-worker response: {error}")));
                return;
            }
        }
    }
}

fn worker_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("QUENCH_TSRS_WORKER") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join(worker_name()));
        }
    }
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let helper_target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/quench-tsrs-worker/target")
        .join(profile)
        .join(worker_name());
    candidates.push(helper_target);
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn worker_name() -> &'static str {
    if cfg!(windows) {
        "quench-tsrs-worker.exe"
    } else {
        "quench-tsrs-worker"
    }
}
