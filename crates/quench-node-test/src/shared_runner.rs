//! Shared-VM worker adapter for the inventory-driven compatibility runner.

use crate::NodeOutcome;
use quench_node::{
    shared_run::{execute_shared, SharedCompletion, SharedInput},
    EntryGoal,
};
use std::path::Path;

pub fn run_file(path: &Path) -> NodeOutcome {
    run_shared_file(path, file_entry_goal(path))
}

fn file_entry_goal(path: &Path) -> EntryGoal {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("mjs") => EntryGoal::Node,
        _ => EntryGoal::CommonJs,
    }
}

fn run_shared_file(path: &Path, goal: EntryGoal) -> NodeOutcome {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            return NodeOutcome::Fail {
                reason: format!("read {}: {error}", path.display()),
            };
        }
    };
    let executable = match std::env::current_exe() {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(error) => {
            return NodeOutcome::Fail {
                reason: format!("resolve shared worker executable: {error}"),
            };
        }
    };
    let script = path.to_string_lossy().into_owned();
    match execute_shared(
        SharedInput::File {
            path: path.to_path_buf(),
            exec_argv: crate::fixture_metadata::fixture_flags(&source),
            goal,
        },
        vec![executable, script],
    ) {
        Ok(SharedCompletion::Completed) => NodeOutcome::Pass,
        Ok(SharedCompletion::PendingTopLevelAwait) => NodeOutcome::Fail {
            reason: "shared VM has unsettled top-level await (exit status 13)".into(),
        },
        Ok(SharedCompletion::GuestExit { code }) => NodeOutcome::GuestExit { code },
        Err(reason) => NodeOutcome::Fail { reason },
    }
}
