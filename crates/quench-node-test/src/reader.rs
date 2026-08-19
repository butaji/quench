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
}

impl Default for NodeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeRunner {
    pub fn new() -> Self {
        let realm = RealmId::ROOT;
        let (host, context) = quench_node::host::install(realm);
        Self { host, context }
    }

    pub fn with_output_sink(self, sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        let (host, context) = quench_node::host::install_with_sink(RealmId::ROOT, sink);
        Self { host, context }
    }

    /// Run one fixture and classify the completion.
    pub fn run(&mut self, fixture: &NodeFixture) -> NodeOutcome {
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
        match quench_runtime::vm::execute_with_context(&ops, &self.context) {
            Ok(_) => NodeOutcome::Pass,
            Err(error) => NodeOutcome::Fail {
                reason: format!("runtime: {error:?}"),
            },
        }
    }
}

fn reduce_script(source: &str) -> Result<Vec<quench_runtime::ops::Op>, String> {
    use quench_runtime::reduce::reduce_source;
    let program = reduce_source(source).map_err(|errors| errors.join("; "))?;
    Ok(program.ops().to_vec())
}
