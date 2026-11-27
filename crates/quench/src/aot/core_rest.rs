//! The core image's rest (C5 D2): the state a closed-world image starts from, dumped by its build.
//!
//! The linked core image runs once more with the dump variable set: it builds its realm and
//! loads its entry on the closed world its own catalog names (`image_driver_core::start_state`),
//! and writes that state as the dump it adopts at each start. The dump is of the image itself,
//! so the addresses in it are the image's. A build whose dump fails still links a working image:
//! its start builds the same state.

use quench_runtime::native_link::image_dump::{self, Dump, UNDUMPED};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

/// Dumps the start state of the linked core `image` into `work`, which `report` names; none
/// where it cannot be dumped.
pub(super) fn dump(image: &Path, work: &Path, report: &mut Value) -> Vec<Dump> {
    match image_dump::dump_image(image, work, &[Vec::new()], UNDUMPED, &|_: &mut Command| {}) {
        Ok(dumps) => {
            report["bootstrap"] = json!({"status": "dumped", "kind": "closed_world_state", "runs_entry_at_start": true,
                "runs": dumps.iter().map(|(dump, raw)| json!({"raw_bytes": raw, "page_bytes": dump.page_bytes()})).collect::<Vec<_>>()});
            dumps.into_iter().map(|(dump, _)| dump).collect()
        }
        Err(error) => {
            report["bootstrap"] = json!({"status": "unavailable", "diagnostic": error});
            Vec::new()
        }
    }
}
