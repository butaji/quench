//! The canonical Node test runner entry points.
//!
//! `run_file` is the single command-line entry point used by the
//! `run-stages` / `run-all` / `triage` binaries. The runner never
//! forks, never rewrites the host, and never inspects the host
//! state directly.

use std::path::Path;
use std::sync::Arc;

use crate::reader::{NodeFixture, NodeOutcome, NodeRunner};

pub struct NodeTestRunner {
    runner: NodeRunner,
}

impl Default for NodeTestRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeTestRunner {
    pub fn new() -> Self {
        let runner = NodeRunner::new().with_output_sink(Arc::new(|line| {
            println!("{line}");
        }));
        Self { runner }
    }

    pub fn run_file(&mut self, path: &Path) -> NodeOutcome {
        let fixture = match NodeFixture::from_path(path.to_path_buf()) {
            Ok(f) => f,
            Err(e) => return NodeOutcome::Fail { reason: e },
        };
        self.runner.run(&fixture)
    }

    pub fn run_source(&mut self, source: &str) -> NodeOutcome {
        let fixture =
            NodeFixture::from_source(std::path::PathBuf::from("<source>"), source.to_string());
        self.runner.run(&fixture)
    }
}

pub fn run_file(path: &Path) -> NodeOutcome {
    NodeTestRunner::new().run_file(path)
}
