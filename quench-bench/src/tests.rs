use super::{
    CorpusRecord, FixtureRecord, HostRecord, SuiteRecord, artifact, fixture_artifact, now_ns,
    peak_rss_bytes, read_checkpoint, resume_metadata_matches, validate_resume, wait_child,
    write_checkpoint,
};
use std::process::Command;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[test]
fn converts_wait4_peak_rss_to_bytes() {
    let raw_rss = 1234;
    let expected: u64 = if cfg!(target_os = "linux") {
        1_263_616
    } else {
        raw_rss as u64
    };
    assert_eq!(peak_rss_bytes(raw_rss), Some(expected));
}

#[test]
// `wait_child` reaps the child by pid through wait4 (its usage is what is measured).
#[allow(clippy::zombie_processes)]
fn wait4_reports_exit_status_and_resource_usage() {
    let mut command = Command::new("sh");
    command.arg("-c").arg("exit 7");
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let child = command.spawn().expect("spawn shell");
    let (status, timed_out, usage) = wait_child(child.id(), 5000).expect("wait4 child");
    assert_eq!(status, 7);
    assert!(!timed_out);
    assert!(usage.peak_rss_bytes.is_some_and(|rss| rss > 0));
}

#[test]
// `wait_child` kills and reaps the child's process group by pid through wait4.
#[allow(clippy::zombie_processes)]
fn timeout_terminates_the_process_group() {
    let mut command = Command::new("sh");
    command.arg("-c").arg("sleep 5");
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let child = command.spawn().expect("spawn shell");
    let (status, timed_out, _) = wait_child(child.id(), 5).expect("wait4 child");
    assert_eq!(status, -1);
    assert!(timed_out);
}

/// The corpus record a checkpoint's fixture set needs.
fn checkpoint_corpus() -> CorpusRecord {
    CorpusRecord {
        pinned_revision: Some("b".repeat(40)),
        checkout_revision: Some("b".repeat(40)),
        checkout_clean: true,
        fixture_set_matches: true,
    }
}

/// The host record a checkpoint's environment needs.
fn checkpoint_host() -> HostRecord {
    HostRecord {
        uname: "test host".into(),
        rustc: "rustc test".into(),
        model: Some("test model".into()),
        memory_bytes: Some(1),
        memory_limit_bytes: Some(1),
        cpu_quota: Some("400000 100000".into()),
        logical_cpus: Some(4),
        process_metrics_backend: "wait4 test".into(),
    }
}

/// A checkpoint report selecting `fixture`, which every resume check reads.
fn checkpoint_report(fixture: &Path) -> SuiteRecord {
    SuiteRecord {
        schema: super::SUITE_REPORT_SCHEMA,
        suite: super::Suite::JetStream3,
        jit_mode: super::JitMode::Jit,
        jit_diagnostics: false,
        jit_js_general_dispatch: false,
        jit_wasm_root: Some(19),
        jit_wasm_root_only: true,
        jit_wasm_root_first: false,
        jit_wasm_selective: false,
        jit_wasm_selective_ab: false,
        created_unix_ns: now_ns(),
        rounds_requested: 11,
        timeout_ms: 300_000,
        source_revision: "a".repeat(40),
        source_dirty: false,
        corpus: checkpoint_corpus(),
        host: checkpoint_host(),
        engines: Vec::new(),
        suite_inputs: Vec::new(),
        selected_fixtures: vec![fixture.display().to_string()],
        fixtures: BTreeMap::new(),
        complete: false,
        qualification_ready: false,
    }
}

/// A temporary checkpoint directory with a written fixture and its report path.
fn checkpoint_directory() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "quench-bench-checkpoint-{}-{}",
        std::process::id(),
        now_ns()
    ));
    fs::create_dir(&directory).expect("create checkpoint test directory");
    let fixture_path = directory.join("fixture.js");
    fs::write(&fixture_path, "// pinned fixture\n").expect("write fixture");
    let report_path = directory.join("report.json");
    (directory, fixture_path, report_path)
}

#[test]
fn checkpoint_round_trips_and_validates_fixture_inputs() {
    let (directory, fixture_path, report_path) = checkpoint_directory();
    let expected = checkpoint_report(&fixture_path);
    let mut saved = expected.clone();
    saved.fixtures.insert(
        fixture_path.display().to_string(),
        FixtureRecord {
            source: artifact(&fixture_path),
            valid: false,
            output_equal: false,
            summaries: BTreeMap::new(),
            unsupported_engines: BTreeMap::new(),
            rounds: Vec::new(),
        },
    );

    write_checkpoint(&report_path, &saved);
    let loaded = read_checkpoint(&report_path);
    validate_resume(&loaded, &expected, &[PathBuf::from(&fixture_path)]);
    assert_eq!(loaded.fixtures.len(), 1);
    assert!(loaded.jit_wasm_root_only);
    let mut different_mode = expected.clone();
    different_mode.jit_wasm_root_only = false;
    assert!(!resume_metadata_matches(&loaded, &different_mode));
    fs::remove_dir_all(directory).expect("remove checkpoint test directory");
}

#[test]
fn checkpoint_resumes_root_first_and_selective_ab_modes() {
    let (directory, fixture_path, report_path) = checkpoint_directory();
    let expected = checkpoint_report(&fixture_path);
    write_checkpoint(&report_path, &expected);
    let loaded = read_checkpoint(&report_path);
    let mut root_first = expected.clone();
    root_first.jit_wasm_root_only = false;
    root_first.jit_wasm_root_first = true;
    let root_first_path = directory.join("root-first-report.json");
    write_checkpoint(&root_first_path, &root_first);
    let root_first_loaded = read_checkpoint(&root_first_path);
    assert!(root_first_loaded.jit_wasm_root_first);
    assert!(!resume_metadata_matches(&loaded, &root_first));
    let mut selective_ab = expected;
    selective_ab.jit_wasm_root_only = false;
    selective_ab.jit_wasm_selective_ab = true;
    let selective_ab_path = directory.join("selective-ab-report.json");
    write_checkpoint(&selective_ab_path, &selective_ab);
    let selective_ab_loaded = read_checkpoint(&selective_ab_path);
    assert!(selective_ab_loaded.jit_wasm_selective_ab);
    assert!(resume_metadata_matches(&selective_ab_loaded, &selective_ab));
    assert!(!resume_metadata_matches(&loaded, &selective_ab));
    fs::remove_dir_all(directory).expect("remove checkpoint test directory");
}

#[test]
fn directory_artifacts_hash_sorted_relative_paths_and_contents() {
    let directory = std::env::temp_dir().join(format!(
        "quench-bench-directory-hash-{}-{}",
        std::process::id(),
        now_ns()
    ));
    fs::create_dir_all(directory.join("nested")).expect("create directory fixture");
    fs::write(directory.join("z.js"), "last").expect("write first file");
    fs::write(directory.join("nested/a.wasm"), "first").expect("write nested file");

    let first = super::sha256(&directory).expect("hash directory fixture");
    assert_eq!(first.len(), 64);
    assert_eq!(first, super::sha256(&directory).expect("repeat hash"));
    fs::write(directory.join("nested/a.wasm"), "changed").expect("change nested file");
    assert_ne!(
        first,
        super::sha256(&directory).expect("hash changed fixture")
    );

    fs::remove_dir_all(directory).expect("remove directory fixture");
}

#[test]
fn synthetic_wasm_fixture_artifact_hashes_its_module_directory() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let fixture = workspace.join(super::jetstream::fixture_path("HashSet-wasm"));
    let source = fixture_artifact(super::Suite::JetStream3, &fixture);
    assert_eq!(source.path, fixture.display().to_string());
    assert!(source.sha256.as_deref().is_some_and(super::valid_sha256));
}
