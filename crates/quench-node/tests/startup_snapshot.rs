//! The startup snapshot's lifecycle as processes see it: a default run of an entry that loads
//! modules schedules the snapshot, concurrent first runs start one builder that leaves one
//! complete file and no lock or unfinished write, later runs restore it, and a snapshot that
//! does not restore boots the process as if there were none.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const QUENCH: &str = env!("CARGO_BIN_EXE_quench-node");
/// How long a builder may take to leave its snapshot (a debug build bootstraps slowly).
const BUILD_DEADLINE: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(100);
/// Concurrent first runs of one entry.
const RACING_RUNS: usize = 4;
const ENTRY: &str = "const path = require('path'); console.log(path.join('a', 'b'));";
const EXPECTED: &str = "a/b\n";

struct Case {
    root: PathBuf,
}

impl Case {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("quench-snapshot-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join("app/main.cjs"), ENTRY).unwrap();
        Self { root }
    }

    fn cache(&self) -> PathBuf {
        self.root.join("cache/compile")
    }

    /// Runs the entry with the default snapshot policy and only this case's cache.
    fn run(&self, snapshot: Option<&str>) -> Output {
        let mut command = Command::new(QUENCH);
        command
            .arg(self.root.join("app/main.cjs"))
            .env_remove("NODE_OPTIONS")
            .env_remove("QUENCH_STARTUP_SNAPSHOT")
            .env("QUENCH_COMPILE_CACHE", self.cache());
        if let Some(value) = snapshot {
            command.env("QUENCH_STARTUP_SNAPSHOT", value);
        }
        command.output().unwrap()
    }

    /// Every file under the snapshot directory, by extension.
    fn snapshot_files(&self) -> Vec<PathBuf> {
        fn walk(directory: &Path, files: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, files);
                } else {
                    files.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(&self.root.join("cache/snapshot"), &mut files);
        files
    }

    /// Waits until the snapshot directory holds a finished snapshot and nothing else.
    fn await_built(&self) -> PathBuf {
        let started = Instant::now();
        loop {
            let files = self.snapshot_files();
            if let [only] = files.as_slice() {
                if only.extension().is_some_and(|extension| extension == "qss") {
                    return only.clone();
                }
            }
            assert!(started.elapsed() < BUILD_DEADLINE, "no single finished snapshot: {files:?}");
            std::thread::sleep(POLL);
        }
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn assert_ran(output: &Output) {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), EXPECTED);
}

#[test]
fn concurrent_first_runs_build_one_snapshot_that_later_runs_restore() {
    let case = Case::new("race");
    let runs: Vec<_> = (0..RACING_RUNS)
        .map(|_| {
            let mut command = Command::new(QUENCH);
            command
                .arg(case.root.join("app/main.cjs"))
                .env_remove("NODE_OPTIONS")
                .env_remove("QUENCH_STARTUP_SNAPSHOT")
                .env("QUENCH_COMPILE_CACHE", case.cache());
            command.stdout(std::process::Stdio::piped()).spawn().unwrap()
        })
        .collect();
    for run in runs {
        assert_ran(&run.wait_with_output().unwrap());
    }
    // The first runs exited before their builder finished; it leaves one preloaded snapshot,
    // with no lock or unfinished write beside it.
    let snapshot = case.await_built();
    assert!(snapshot.to_string_lossy().ends_with(".preloaded.qss"), "{snapshot:?}");
    let restored = case.run(Some("trace"));
    assert_ran(&restored);
    assert!(
        String::from_utf8_lossy(&restored.stderr).contains("startup snapshot restored (preloaded)"),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    assert_ran(&case.run(None));
    assert_eq!(case.snapshot_files(), vec![snapshot]);
}

#[test]
fn a_snapshot_that_does_not_restore_boots_the_process_without_one() {
    let case = Case::new("corrupt");
    assert_ran(&case.run(None));
    let snapshot = case.await_built();
    std::fs::write(&snapshot, b"not a snapshot").unwrap();
    let fallback = case.run(Some("trace"));
    assert_ran(&fallback);
    assert!(
        String::from_utf8_lossy(&fallback.stderr).contains("not restored"),
        "{}",
        String::from_utf8_lossy(&fallback.stderr)
    );
    assert_ran(&case.run(None));
}

#[test]
fn off_never_schedules_a_snapshot() {
    let case = Case::new("off");
    assert_ran(&case.run(Some("off")));
    std::thread::sleep(POLL * 10);
    assert_eq!(case.snapshot_files(), Vec::<PathBuf>::new());
}
