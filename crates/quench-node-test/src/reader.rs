//! Discovery and classification of Node fixture outcomes.

use std::path::{Path, PathBuf};

use quench_node::NodeHost;
use quench_runtime::ops::RealmId;
use quench_runtime::vm::VmContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeOutcome {
    Pass,
    Fail { reason: String },
    Skip { reason: String },
}

/// One Node fixture file + its raw source.
#[derive(Debug, Clone)]
pub struct NodeFixture {
    pub path: PathBuf,
    pub source: String,
}

impl NodeFixture {
    pub fn from_path(path: PathBuf) -> Result<Self, String> {
        let source =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let path = path.canonicalize().unwrap_or(path);
        Ok(Self { path, source })
    }

    pub fn from_source(path: PathBuf, source: String) -> Self {
        Self { path, source }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Canonical Node host runner. Owns the host handle and the
/// `VmContext`; both live for the duration of the run.
pub struct NodeRunner {
    pub host: std::rc::Rc<NodeHost>,
    pub context: VmContext,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
}

impl Default for NodeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeRunner {
    pub fn new() -> Self {
        let (host, context) = quench_node::host::install(RealmId::ROOT);
        Self {
            host,
            context,
            sink: std::sync::Arc::new(|_| {}),
        }
    }

    pub fn with_output_sink(self, sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        let (host, context) = quench_node::host::install_with_sink(RealmId::ROOT, sink.clone());
        Self {
            host,
            context,
            sink,
        }
    }

    /// Run one fixture and classify the completion.
    pub fn run(&mut self, fixture: &NodeFixture) -> NodeOutcome {
        // Fresh host per fixture with `node <file>` argv semantics.
        let script = fixture.path.to_string_lossy().into_owned();
        let (host, context) =
            quench_node::host::install_script(RealmId::ROOT, self.sink.clone(), &script);
        self.host = host;
        self.context = context;
        if let Some(dir) = fixture.path.parent() {
            self.host.set_main_dir(dir.to_string_lossy().into_owned());
        }
        let ops = match reduce_script(&fixture.source) {
            Ok(ops) => ops,
            Err(error) => {
                return NodeOutcome::Fail {
                    reason: format!("reduce: {error}"),
                };
            }
        };
        let result = quench_runtime::vm::execute_with_context(&ops, &self.context)
            .and_then(|_| self.drive("__quench_run_loop__();"));
        // `process.exit` unwinds with an error; `exit` handlers still run.
        let result = match result {
            Err(error) => match self.drive("__quench_run_exit__();") {
                Ok(_) => Err(error),
                Err(exit_error) => Err(exit_error),
            },
            ok => ok.map(|_| ()),
        };
        Self::classify(result, self.host.exit_code())
    }

    fn classify(
        result: Result<(), quench_runtime::vm::VmError>,
        exit_code: Option<i32>,
    ) -> NodeOutcome {
        match (result, exit_code) {
            (Ok(_), None | Some(0)) => NodeOutcome::Pass,
            (Ok(_), Some(code)) => NodeOutcome::Fail {
                reason: format!("exit code {code}"),
            },
            (Err(_), Some(0)) => NodeOutcome::Pass,
            (Err(error), Some(code)) => NodeOutcome::Fail {
                reason: format!("exit code {code}: {}", error.render()),
            },
            (Err(error), None) => NodeOutcome::Fail {
                reason: format!("runtime: {error:?}"),
            },
        }
    }

    /// Execute a tiny driver snippet (e.g. `__quench_run_loop__();`)
    /// in the same context, so the host pump runs inside an active
    /// execution frame with globals and capabilities available.
    fn drive(
        &self,
        source: &str,
    ) -> Result<quench_runtime::value::Value, quench_runtime::vm::VmError> {
        let ops = reduce_script(source).map_err(quench_runtime::vm::VmError::EvalError)?;
        quench_runtime::vm::execute_with_context(&ops, &self.context)
    }
}

fn reduce_script(source: &str) -> Result<Vec<quench_runtime::ops::Op>, String> {
    use quench_runtime::reduce::reduce_source;
    let program = reduce_source(source).map_err(|errors| errors.join("; "))?;
    Ok(program.ops().to_vec())
}
