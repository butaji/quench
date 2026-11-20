//! What a gate run was asked, and where the repository and its build output are.

use crate::build::AOT_TARGET;
use std::{
    env,
    ffi::OsStr,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process::Command,
    thread,
};

/// The cargo profile every gate build and test uses: the repository's dev-loop profile.
pub const PROFILE: &str = "experiment";
const TARGET_VARIABLE: &str = "CARGO_TARGET_DIR";
const DEFAULT_TARGET: &str = "target";
const QUICK_FLAG: &str = "--quick";
const RECORD_FLAG: &str = "--record";
const JOBS_FLAG: &str = "--jobs";
const PASSED_FLAG: &str = "--passed";
const ONLY_FLAG: &str = "--only";
const STRICT_FLAG: &str = "--strict";
const SUITES_FLAG: &str = "--suites";
const JETSTREAM_FLAG: &str = "--jetstream";
/// The JetStream checkout (a submodule) the AOT step reads its rows from, relative to the root.
const DEFAULT_JETSTREAM: &str = "quench-bench/jetstream";
/// How many directories above this crate's manifest the workspace root lies (`crates/quench-gate`).
const ROOT_DEPTH: usize = 2;

#[derive(Clone, Debug)]
pub struct Config {
    /// Parallel jobs for cargo and for the conformance runs.
    pub jobs: usize,
    /// Only the lint, census and C-parity steps.
    pub quick: bool,
    /// Write ratchet entries that have no value yet (a recorded value is never changed).
    pub record: bool,
    /// Only answer whether the checked-out tree has a full pass record.
    pub passed: bool,
    /// Run only the named step (`--only ir-budget`, or a bare name such as `aot`), on the binaries already built.
    pub only: Option<String>,
    /// Hold the AOT step to its absolute targets, not only to its ratchets (`--strict`).
    pub strict: bool,
    /// Run the AOT conformance sub-steps as well (`--suites`).
    pub suites: bool,
    pub root: PathBuf,
    /// The JetStream checkout the rows' files and the C analogues' data come from (`--jetstream`).
    pub jetstream: PathBuf,
    /// Cargo's target directory.
    pub target: PathBuf,
}

impl Config {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(ROOT_DEPTH).map(Path::to_path_buf).ok_or("the gate crate lies outside its workspace")?;
        let target = root.join(env::var_os(TARGET_VARIABLE).unwrap_or_else(|| DEFAULT_TARGET.into()));
        let jobs = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        let jetstream = root.join(DEFAULT_JETSTREAM);
        let mut config = Self { jobs, quick: false, record: false, passed: false, only: None, strict: false, suites: false, root, target, jetstream };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                QUICK_FLAG => config.quick = true,
                RECORD_FLAG => config.record = true,
                PASSED_FLAG => config.passed = true,
                STRICT_FLAG => config.strict = true,
                SUITES_FLAG => config.suites = true,
                name if !name.starts_with('-') => config.only = Some(name.to_owned()),
                ONLY_FLAG => config.only = Some(args.next().ok_or_else(|| format!("{ONLY_FLAG} needs a step name"))?),
                JETSTREAM_FLAG => config.jetstream = config.root.join(args.next().ok_or_else(|| format!("{JETSTREAM_FLAG} needs a directory"))?),
                JOBS_FLAG => config.jobs = args.next().and_then(|jobs| jobs.parse().ok()).filter(|jobs| *jobs > 0).ok_or_else(|| format!("{JOBS_FLAG} needs a positive number"))?,
                other => return Err(format!("unknown argument `{other}`; usage: quench-gate [{QUICK_FLAG}] [{JOBS_FLAG} N] [{RECORD_FLAG}] [{PASSED_FLAG}] [{ONLY_FLAG} STEP] [{STRICT_FLAG}] [{SUITES_FLAG}] [{JETSTREAM_FLAG} DIR]")),
            }
        }
        Ok(config)
    }

    /// The binary `name` the profile builds, or what to do when it is not built.
    pub fn binary(&self, name: &str) -> Result<PathBuf, String> {
        self.built(&self.target, name)
    }

    /// The binary `name` the AOT-enabled build produces, or what to do when it is not built.
    pub fn aot_binary(&self, name: &str) -> Result<PathBuf, String> {
        self.built(&self.target.join(AOT_TARGET), name)
    }

    fn built(&self, target: &Path, name: &str) -> Result<PathBuf, String> {
        let path = target.join(PROFILE).join(name);
        if path.exists() {
            return Ok(path);
        }
        Err(format!("{} is not built: run the gate without --quick, or `cargo build --profile {PROFILE} --bin {name}`", path.display()))
    }

    /// A command that runs in the repository root.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        command.current_dir(&self.root);
        command
    }

    /// `cargo <subcommand> --profile experiment -j<jobs>` in the repository root.
    pub fn cargo(&self, subcommand: &str) -> Command {
        self.cargo_in(subcommand, PROFILE)
    }

    /// `cargo <subcommand> --profile <profile> -j<jobs>` in the repository root.
    pub fn cargo_in(&self, subcommand: &str, profile: &str) -> Command {
        let mut command = self.command("cargo");
        command.arg(subcommand).args(["--profile", profile]).arg(format!("-j{}", self.jobs));
        command
    }
}
