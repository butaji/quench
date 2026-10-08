//! The checked-in Node `test/parallel` manifest is the profile selection authority.

use crate::case_process::{RunResult, observe_parallel_case};
use std::{path::PathBuf, process::ExitCode, time::Duration};

const PARALLEL_DIR: &str = "tests/node/test/parallel";
const MANIFEST: &str = "crates/quench-node-test/node-tests/parallel.txt";

struct Entry {
    name: String,
    profiles: Vec<String>,
}

pub fn run(profile: Option<&str>, filter: Option<&str>, timeout_secs: u64) -> ExitCode {
    let fixtures = match select(profile, filter) {
        Ok(fixtures) => fixtures,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("run-parallel"));
    let mut counts = [0usize; RunResult::COUNT];
    for fixture in &fixtures {
        let (result, reason) =
            match observe_parallel_case(&executable, fixture, Duration::from_secs(timeout_secs)) {
                Ok(observation) => {
                    let reason = match observation.worker.as_ref() {
                        Some(crate::NodeOutcome::Fail { reason }) => Some(reason.clone()),
                        Some(crate::NodeOutcome::GuestExit { code }) => Some(format!(
                            "guest exit status {code} requires harness classification"
                        )),
                        _ => observation.signal.map(|signal| {
                            let stderr = String::from_utf8_lossy(&observation.stderr);
                            format!(
                                "worker terminated by signal {}; stderr: {}",
                                signal,
                                stderr.trim()
                            )
                        }),
                    };
                    (observation.outcome(), reason)
                }
                Err(error) => (RunResult::Unclassified, Some(error)),
            };
        counts[result as usize] += 1;
        match reason {
            Some(reason) => println!(
                "{}  {}: {reason}",
                result.label().to_uppercase(),
                fixture.display()
            ),
            None => println!("{}  {}", result.label().to_uppercase(), fixture.display()),
        }
    }
    println!(
        "parallel: pass={} skip={} fail={} timeout={} crash={} unclassified={} total={}",
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        counts[4],
        counts[5],
        fixtures.len()
    );
    if counts[RunResult::Pass as usize] == fixtures.len() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn select(profile: Option<&str>, filter: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let root = std::env::current_dir().map_err(|error| format!("repository directory: {error}"))?;
    let parallel_root = root.join(PARALLEL_DIR);
    if !parallel_root.is_dir() {
        return Err(format!(
            "upstream Node fixture directory is missing: {PARALLEL_DIR}; initialize the tests/node submodule"
        ));
    }
    let entries = read_manifest()?;
    validate(&entries, &parallel_root)?;
    let fixtures: Vec<_> = entries
        .into_iter()
        .filter(|entry| profile.is_none_or(|name| entry.profiles.iter().any(|value| value == name)))
        .filter(|entry| filter.is_none_or(|needle| entry.name.contains(needle)))
        .map(|entry| parallel_root.join(entry.name))
        .collect();
    if fixtures.is_empty() {
        return Err(format!(
            "parallel manifest has no members for profile {:?} and filter {:?}",
            profile, filter
        ));
    }
    Ok(fixtures)
}

fn read_manifest() -> Result<Vec<Entry>, String> {
    let manifest =
        std::fs::read_to_string(MANIFEST).map_err(|error| format!("read {MANIFEST}: {error}"))?;
    let entries: Vec<_> = manifest
        .lines()
        .filter_map(|line| {
            let (name, annotation) = line.split_once('#').unwrap_or((line, ""));
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            let profiles = annotation
                .split_whitespace()
                .filter_map(|part| part.strip_prefix("profile="))
                .map(str::to_owned)
                .collect();
            Some(Entry {
                name: name.to_owned(),
                profiles,
            })
        })
        .collect();
    if entries.is_empty() {
        return Err(format!("{MANIFEST} is missing or empty"));
    }
    Ok(entries)
}

fn validate(entries: &[Entry], parallel_root: &std::path::Path) -> Result<(), String> {
    let mut seen = std::collections::HashSet::with_capacity(entries.len());
    for Entry { name, .. } in entries {
        if std::path::Path::new(name)
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(format!(
                "fixture must be relative to the parallel suite: {name}"
            ));
        }
        if !seen.insert(name) {
            return Err(format!("duplicate fixture entry: {name}"));
        }
        let path = parallel_root.join(name);
        if !path.is_file() {
            return Err(format!("fixture does not exist: {name}"));
        }
        if !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("js" | "mjs" | "cjs")
        ) {
            return Err(format!("fixture has unsupported extension: {name}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate, Entry};
    use std::fs;

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.to_owned(),
            profiles: Vec::new(),
        }
    }

    #[test]
    fn manifest_validation_rejects_duplicates_unsafe_paths_and_unknown_extensions() {
        let root =
            std::env::temp_dir().join(format!("quench-node-manifest-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("test-a.js"), "").unwrap();
        fs::write(root.join("test-b.txt"), "").unwrap();
        assert!(validate(&[entry("test-a.js"), entry("test-a.js")], &root).is_err());
        assert!(validate(&[entry("test-b.txt")], &root).is_err());
        for name in [
            "../test-a.js".to_string(),
            root.join("test-a.js").display().to_string(),
        ] {
            let error = validate(&[entry(&name)], &root).unwrap_err();
            assert!(error.contains("relative to the parallel suite"));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_validation_accepts_all_fixture_extensions() {
        let root =
            std::env::temp_dir().join(format!("quench-node-manifest-valid-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        for name in ["test-a.js", "test-b.mjs", "test-c.cjs"] {
            fs::write(root.join(name), "").unwrap();
        }
        let entries = ["test-a.js", "test-b.mjs", "test-c.cjs"].map(entry);
        assert!(validate(&entries, &root).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
