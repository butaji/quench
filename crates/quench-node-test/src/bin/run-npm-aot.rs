//! Native admission and original scenario observations for the pinned npm corpus.
use quench_node_test::case_process::{observe_command, CaseObservation};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const SELECTED_PACKAGES: usize = 300;
const SCHEMA_VERSION: u64 = 1;
const OBSERVATION_ENVIRONMENT: [(&str, &str); 2] = [("NO_COLOR", "1"), ("TZ", "UTC")];
const REMOVED_OBSERVATION_ENVIRONMENT: [&str; 2] = ["FORCE_COLOR", "DEBUG"];

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))
}
fn success(observation: &CaseObservation) -> bool {
    observation.exit_code == Some(0) && observation.signal.is_none() && !observation.timed_out
}
fn observe(
    mut command: Command,
    directory: &Path,
    timeout: Duration,
) -> Result<CaseObservation, String> {
    command.current_dir(directory).envs(OBSERVATION_ENVIRONMENT);
    for name in REMOVED_OBSERVATION_ENVIRONMENT {
        command.env_remove(name);
    }
    observe_command(command, timeout)
}
fn effects(directory: &Path) -> Result<Value, String> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<Value>) -> Result<(), String> {
        let mut entries = fs::read_dir(directory)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned();
            if kind.is_dir() {
                visit(root, &path, files)?;
            } else if kind.is_symlink() {
                files.push(json!({"path":relative,"symlink":fs::read_link(path).map_err(|e|e.to_string())?}));
            } else if kind.is_file() {
                let bytes = fs::read(path).map_err(|e| e.to_string())?;
                files.push(json!({"path":relative,"bytes":bytes.len(),"sha256":digest(&bytes)}));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(directory, directory, &mut files)?;
    Ok(json!(files))
}
fn observation(observation: &CaseObservation) -> Value {
    // CaseObservation owns the byte-exact output and process/resource protocol.
    serde_json::to_value(observation).expect("process observations are serializable")
}

struct Options {
    node: PathBuf,
    compiler: PathBuf,
    output: PathBuf,
    compile_timeout: Duration,
    execute_timeout: Duration,
    profile: String,
    sdk_report: Option<PathBuf>,
    filter: Option<String>,
}
impl Options {
    fn parse() -> Result<Self, String> {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        let [node, compiler, output, compile_seconds, execute_seconds, rest @ ..] = args.as_slice()
        else {
            return Err("usage: run-npm-aot NODE COMPILER REPORT COMPILE_SECONDS EXECUTE_SECONDS [--profile iteration|production] [--sdk-report FILE] [--package NAME]".into());
        };
        let duration = |value: &str| -> Result<Duration, String> {
            let seconds = value.parse::<u64>().map_err(|e| e.to_string())?;
            if seconds == 0 {
                return Err("positive deadlines are required".into());
            }
            Ok(Duration::from_secs(seconds))
        };
        let mut options = Self {
            node: fs::canonicalize(node).map_err(|e| e.to_string())?,
            compiler: fs::canonicalize(compiler).map_err(|e| e.to_string())?,
            output: PathBuf::from(output),
            compile_timeout: duration(compile_seconds)?,
            execute_timeout: duration(execute_seconds)?,
            profile: "iteration".into(),
            sdk_report: None,
            filter: None,
        };
        let mut rest = rest.iter();
        while let Some(option) = rest.next() {
            let value = rest
                .next()
                .ok_or_else(|| format!("missing {option} value"))?;
            match option.as_str() {
                "--profile" if matches!(value.as_str(), "iteration" | "production") => {
                    options.profile = value.clone()
                }
                "--sdk-report" => {
                    options.sdk_report = Some(fs::canonicalize(value).map_err(|e| e.to_string())?)
                }
                "--package" if options.filter.is_none() => options.filter = Some(value.clone()),
                _ => return Err(format!("unknown or repeated option {option}")),
            }
        }
        if let Some(parent) = options.output.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Ok(options)
    }
    fn compile(&self, entry: &Path, binary: &Path, report: &Path) -> Command {
        let mut command = Command::new(&self.compiler);
        command
            .arg("compile")
            .arg(entry)
            .arg("-o")
            .arg(binary)
            .arg("--report")
            .arg(report)
            .arg("--profile")
            .arg(&self.profile);
        if let Some(sdk) = &self.sdk_report {
            command.arg("--sdk-report").arg(sdk);
        }
        command
    }
}

fn compile(options: &Options, entry: &Path, directory: &Path) -> Result<Value, String> {
    let binary = directory.join("native");
    let report = directory.join("compile.json");
    let command = options.compile(entry, &binary, &report);
    let observed = observe(command, directory, options.compile_timeout)?;
    let payload = if report.exists() {
        stage(read_json(&report))
    } else {
        Value::Null
    };
    let emitted = success(&observed) && binary.is_file() && payload["status"] == "compiled";
    let identity = if emitted {
        let bytes = fs::read(&binary).map_err(|e| e.to_string())?;
        json!({"bytes":bytes.len(),"sha256":digest(&bytes),"path":binary})
    } else {
        Value::Null
    };
    Ok(
        json!({"observation":observation(&observed),"report":payload,"linked":emitted,"executable":identity}),
    )
}

fn scenario(
    options: &Options,
    entry: &Path,
    package: &str,
    executable: &Path,
    native: bool,
) -> Result<Value, String> {
    let working = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut command = Command::new(executable);
    if !native {
        command.arg(entry);
    }
    command.arg(package);
    let observed = observe(command, working.path(), options.execute_timeout)?;
    let checks = working.path().join("function-checks.json");
    let marker = format!("npm-behavior-ok:{package}\n");
    let completed =
        success(&observed) && observed.stdout == marker.as_bytes() && observed.stderr.is_empty();
    Ok(
        json!({"observation":observation(&observed),"completed_contract":completed,
        "function_checks":if checks.exists(){read_json(&checks)?}else{Value::Null},"effects":effects(working.path())?}),
    )
}

fn stage(result: Result<Value, String>) -> Value {
    result.unwrap_or_else(|error| json!({"status":"observation_error","error":error}))
}

// npm's os/cpu selectors include positive alternatives and explicit exclusions.
fn target_matches(selectors: &Value, target: &str) -> bool {
    let Some(selectors) = selectors.as_array() else {
        return selectors.is_null();
    };
    let names = selectors
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    !names
        .iter()
        .any(|name| name.strip_prefix('!') == Some(target))
        && (!names.iter().any(|name| !name.starts_with('!'))
            || names.contains(&target)
            || names.contains(&"any"))
}

fn package_platform(metadata: Option<&Value>, node: &Value) -> Value {
    let Some(metadata) = metadata else {
        return json!({"compatible":false,"reason":"installation metadata unavailable"});
    };
    let os = node["platform"].as_str().unwrap_or("");
    let arch = node["arch"].as_str().unwrap_or("");
    let libc = node["libc"].as_str();
    let libc_matches = metadata["libc"].is_null()
        || os != "linux"
        || libc.is_some_and(|target| target_matches(&metadata["libc"], target));
    json!({"compatible":target_matches(&metadata["os"],os) && target_matches(&metadata["cpu"],arch) && libc_matches,
        "os":metadata["os"],"cpu":metadata["cpu"],"libc":metadata["libc"]})
}

fn main() -> Result<(), String> {
    let options = Options::parse()?;
    let root = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .map_err(|e| e.to_string())?;
    let suite = root.join("tests/npm-top300");
    let inventory_path = suite.join("inventory.json");
    let inventory = read_json(&inventory_path)?;
    let packages = inventory["packages"]
        .as_array()
        .ok_or("missing package inventory")?;
    let mut unique = BTreeSet::new();
    if packages.len() != SELECTED_PACKAGES
        || packages
            .iter()
            .any(|p| p["name"].as_str().is_none_or(|name| !unique.insert(name)))
    {
        return Err("expected exactly 300 unique selected packages".into());
    }
    let mut command = Command::new(&options.node);
    command.args(["-p", "JSON.stringify({version:process.version,platform:process.platform,arch:process.arch,builtins:require('node:module').builtinModules,libc:process.report.getReport().header.glibcVersionRuntime?'glibc':null,resources:{parallelism:require('node:os').availableParallelism(),cpu:require('node:os').cpus()[0]?.model,memory:require('node:os').totalmem(),osRelease:require('node:os').release()}})"]);
    let node_info = command.output().map_err(|e| e.to_string())?;
    if !node_info.status.success() {
        return Err("Node oracle identification failed".into());
    }
    let node_info: Value = serde_json::from_slice(&node_info.stdout).map_err(|e| e.to_string())?;
    let builtin_names: BTreeSet<_> = node_info["builtins"]
        .as_array()
        .ok_or("missing builtin names")?
        .iter()
        .filter_map(Value::as_str)
        .map(|name| name.trim_start_matches("node:"))
        .collect();
    let source = |args: &[&str]| -> Result<String, String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err("source provenance failed".into());
        }
        String::from_utf8(output.stdout)
            .map(|s| s.trim().to_owned())
            .map_err(|e| e.to_string())
    };
    let scenario_path = suite.join("scenario.mjs");
    let shared_directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let shared = stage(compile(&options, &scenario_path, shared_directory.path()));
    let mut report = json!({"schema_version":SCHEMA_VERSION,"mode":"aot","scope":"Original behavioral scenarios and separate package-entry admission/init probes; import initialization never proves callable behavior. Declaration checks are distinct from native runtime passes.",
        "source_revision":source(&["rev-parse","HEAD"] )?,"source_dirty":!source(&["status","--porcelain"] )?.is_empty(),
        "process_environment":{"set":OBSERVATION_ENVIRONMENT,"remove":REMOVED_OBSERVATION_ENVIRONMENT},"target_host":format!("{}-{}",std::env::consts::ARCH,std::env::consts::OS),"node":node_info,"node_sha256":digest(&fs::read(&options.node).map_err(|e|e.to_string())?),
        "sdk_report":options.sdk_report.as_ref().map(|path|fs::read(path).map(|bytes|json!({"path":path,"sha256":digest(&bytes)}))).transpose().map_err(|e|e.to_string())?,"compiler":options.compiler,"compiler_sha256":digest(&fs::read(&options.compiler).map_err(|e|e.to_string())?),"profile":options.profile,
        "compile_timeout_seconds":options.compile_timeout.as_secs(),"execute_timeout_seconds":options.execute_timeout.as_secs(),
        "inventory_sha256":digest(&fs::read(&inventory_path).map_err(|e|e.to_string())?),"lock_sha256":digest(&fs::read(suite.join("package-lock.json")).map_err(|e|e.to_string())?),
        "scenario_sha256":digest(&fs::read(&scenario_path).map_err(|e|e.to_string())?),"shared_scenario_compilation":shared,"filtered":options.filter.is_some(),"records":[],"complete":false,"qualified":false});
    let save = |report: &Value| {
        fs::write(
            &options.output,
            serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    };
    save(&report)?;
    for package in packages {
        let name = package["name"].as_str().ok_or("missing package name")?;
        if options.filter.as_ref().is_some_and(|filter| filter != name) {
            continue;
        }
        let installation = package["installation"].as_str().unwrap_or(name);
        let metadata_path = suite
            .join("node_modules")
            .join(installation)
            .join("package.json");
        let metadata = read_json(&metadata_path);
        let installed = metadata
            .as_ref()
            .ok()
            .is_some_and(|metadata| metadata["version"] == package["version"]);
        let oracle = stage(scenario(
            &options,
            &scenario_path,
            name,
            &options.node,
            false,
        ));
        let platform = package_platform(metadata.as_ref().ok(), &report["node"]);
        let native = if report["shared_scenario_compilation"]["linked"] == true {
            stage(scenario(
                &options,
                &scenario_path,
                name,
                &shared_directory.path().join("native"),
                true,
            ))
        } else {
            json!({"status":"blocked","compilation_ref":"shared_scenario_compilation"})
        };
        let declarations = package["types_only"] == true;
        let probe = if declarations {
            json!({"status":"not_applicable","reason":"type-only package; declaration consumers remain separate"})
        } else if !installed {
            json!({"status":"blocked","reason":"selected installation/version unavailable"})
        } else {
            stage((|| -> Result<Value, String> {
                let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(
                    suite.join("node_modules"),
                    directory.path().join("node_modules"),
                )
                .map_err(|e| e.to_string())?;
                #[cfg(not(unix))]
                return Err("package probe isolation requires directory symlink support".into());
                let metadata = metadata.as_ref().map_err(Clone::clone)?;
                let specifier = if builtin_names.contains(name) {
                    let main = metadata["main"].as_str().unwrap_or("index.js");
                    suite
                        .join("node_modules")
                        .join(installation)
                        .join(main)
                        .to_string_lossy()
                        .into_owned()
                } else {
                    installation.to_owned()
                };
                let entry = directory.path().join("entry.mjs");
                let marker = format!("npm-aot-import-ok:{name}");
                let input = format!(
                    "import * as namespace from {};\nconsole.log({});\n",
                    serde_json::to_string(&specifier).map_err(|e| e.to_string())?,
                    serde_json::to_string(&marker).map_err(|e| e.to_string())?
                );
                fs::write(&entry, &input).map_err(|e| e.to_string())?;
                let mut result = compile(&options, &entry, directory.path())?;
                result["input"] =
                    json!({"source":input,"sha256":digest(input.as_bytes()),"specifier":specifier});
                if result["linked"] == true {
                    let oracle_directory = tempfile::tempdir().map_err(|e| e.to_string())?;
                    let native_directory = tempfile::tempdir().map_err(|e| e.to_string())?;
                    let mut node = Command::new(&options.node);
                    node.arg(&entry);
                    let oracle = observe(node, oracle_directory.path(), options.execute_timeout)?;
                    let native = observe(
                        Command::new(directory.path().join("native")),
                        native_directory.path(),
                        options.execute_timeout,
                    )?;
                    let oracle_effects = effects(oracle_directory.path())?;
                    let native_effects = effects(native_directory.path())?;
                    result["initialization_match"] = json!(
                        success(&oracle)
                            && success(&native)
                            && oracle.stdout == format!("{marker}\n").as_bytes()
                            && oracle.stdout == native.stdout
                            && oracle.stderr == native.stderr
                            && oracle_effects == native_effects
                    );
                    result["node"] = observation(&oracle);
                    result["native"] = observation(&native);
                    result["node_effects"] = oracle_effects;
                    result["native_effects"] = native_effects;
                }
                Ok(result)
            })())
        };
        let behavioral = !declarations
            && installed
            && platform["compatible"] == true
            && oracle["completed_contract"] == true
            && native["completed_contract"] == true
            && oracle["function_checks"] == native["function_checks"]
            && oracle["effects"] == native["effects"];
        let row = json!({"package":package,"installed_version_matches":installed,"installed_metadata":metadata.as_ref().ok(),"platform":platform,"installation_error":metadata.err(),
            "oracle_scenario":oracle,"native_scenario":native,"package_entry_probe":probe,"declarations_only":declarations,
            "native_behavior_pass":behavioral,"declaration_oracle_pass":declarations && installed && oracle["completed_contract"] == true});
        println!(
            "{name}: native_behavior_pass={behavioral}, probe_linked={}",
            row["package_entry_probe"]["linked"]
        );
        report["records"]
            .as_array_mut()
            .ok_or("invalid matrix records")?
            .push(row);
        save(&report)?;
    }
    let records = report["records"]
        .as_array()
        .ok_or("missing matrix records")?;
    if records.is_empty() {
        return Err("package filter selected no packages".into());
    }
    let count = records.len();
    let passes = records
        .iter()
        .filter(|row| row["native_behavior_pass"] == true)
        .count();
    report["complete"] = json!(options.filter.is_none() && count == SELECTED_PACKAGES);
    report["native_behavior_passes"] = json!(passes);
    report["declaration_oracle_passes"] = json!(report["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["declaration_oracle_pass"] == true)
        .count());
    let runtime_entries = packages
        .iter()
        .filter(|package| package["types_only"] != true)
        .count();
    let declaration_entries = packages.len() - runtime_entries;
    report["runtime_entries"] = json!(runtime_entries);
    report["declaration_entries"] = json!(declaration_entries);
    report["source_dirty_at_finish"] = json!(!source(&["status", "--porcelain"])?.is_empty());
    report["source_revision_at_finish"] = json!(source(&["rev-parse", "HEAD"])?);
    report["qualified"] = json!(
        report["complete"] == true
            && report["source_dirty"] == false
            && report["source_dirty_at_finish"] == false
            && report["source_revision"] == report["source_revision_at_finish"]
            && passes == runtime_entries
            && report["declaration_oracle_passes"] == json!(declaration_entries)
    );
    save(&report)?;
    if report["qualified"] == true {
        Ok(())
    } else {
        Err("npm AOT qualification incomplete; all observations retained".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_selectors_respect_exclusions_and_unrestricted_targets() {
        assert!(target_matches(&Value::Null, "darwin"));
        assert!(target_matches(&json!(["!linux"]), "darwin"));
        assert!(!target_matches(&json!(["any", "!darwin"]), "darwin"));
        assert!(!target_matches(&json!(["linux"]), "darwin"));
        assert!(target_matches(&json!(["linux", "darwin"]), "darwin"));
    }

    #[test]
    fn installed_cross_platform_package_does_not_qualify_on_host() {
        let node = json!({"platform":"darwin","arch":"arm64"});
        assert_eq!(
            package_platform(Some(&json!({"os":["linux"],"cpu":["x64"]})), &node)["compatible"],
            false
        );
        let linux = json!({"platform":"linux","arch":"x64","libc":null});
        assert_eq!(
            package_platform(Some(&json!({"libc":["glibc"]})), &linux)["compatible"],
            false
        );
    }
}
