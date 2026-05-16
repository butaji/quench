//! # Build Driver
//!
//! Main compilation orchestration.

use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;
use std::time::Duration;
use crate::{Result, parser, analyzer, codegen};
use super::config::RuneConfig;
use super::cache::CacheManager;
use crate::reload::{DylibWatcher, HostSignaler};

/// Build mode.
#[derive(Debug, Clone, Copy, Default)]
pub enum BuildMode {
    /// Development with hot reload
    #[default]
    Dev,
    /// Release build
    Release,
}

/// Options for building.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Mode to build in
    pub mode: BuildMode,
    /// Working directory
    pub workspace: PathBuf,
    /// Target crate name
    pub target_crate: Option<String>,
    /// Config file path
    pub config: Option<PathBuf>,
    /// File to transpile (for transpile command)
    pub transpile_file: Option<PathBuf>,
    /// Verbose output
    pub verbose: bool,
}

impl BuildOptions {
    /// Create new options.
    #[must_use]
    pub const fn new(workspace: PathBuf) -> Self {
        Self {
            mode: BuildMode::Dev,
            workspace,
            target_crate: None,
            config: None,
            transpile_file: None,
            verbose: false,
        }
    }

    /// Set development mode.
    #[must_use]
    pub const fn dev(mut self) -> Self {
        self.mode = BuildMode::Dev;
        self
    }

    /// Set release mode.
    #[must_use]
    pub const fn release(mut self) -> Self {
        self.mode = BuildMode::Release;
        self
    }
}

/// Main build driver.
pub struct BuildDriver {
    /// Build options
    pub(crate) options: BuildOptions,
    /// Rune configuration
    pub(crate) config: RuneConfig,
    /// Cache manager
    pub(crate) cache: CacheManager,
}

impl BuildDriver {
    /// Create a new build driver.
    pub fn new(options: BuildOptions) -> Result<Self> {
        let config = load_config(&options)?;
        let cache = CacheManager::new(&options.workspace)?;
        Ok(Self { options, config, cache })
    }

    /// Run in development mode with hot reload.
    pub fn dev(&mut self) -> Result<()> {
        init_dev_mode(self)?;
        run_watch_loop(self)?;
        Ok(())
    }

    /// Run in release mode.
    pub fn build(&mut self) -> Result<()> {
        if self.options.verbose {
            println!("Running in release mode...");
        }
        self.build_once()?;
        self.build_crate(true)?;
        if self.options.verbose {
            println!("Release build complete.");
        }
        Ok(())
    }

    /// Type check only.
    pub fn check(&mut self) -> Result<()> {
        let sources = self.find_sources()?;
        let parsed = Self::parse_sources(&sources)?;
        let analyses = Self::analyze_sources(&parsed)?;

        for analysis in &analyses {
            for warning in &analysis.warnings {
                println!("warning: {} at {}", warning.message, warning.location);
            }
        }

        println!("Type check passed for {} files.", parsed.len());
        Ok(())
    }

    /// Transpile a single file to stdout.
    pub fn transpile(&mut self) -> Result<()> {
        let file = self.options.transpile_file.as_ref()
            .ok_or_else(|| crate::RuneError::Codegen("No file specified".into()))?;

        let source = parser::parse_file(file)?;
        let analysis = analyzer::analyze(&source)?;
        let module = codegen::generate(&source, &analysis)?;

        println!("{}", module.source);
        Ok(())
    }

    /// Initialize a new project.
    pub fn init(&self) -> Result<()> {
        self.init_project_structure()?;
        self.init_protocol()?;
        self.init_host()?;
        self.init_app()?;
        println!("Initialized Rune project: {}", self.config.project.name);
        Ok(())
    }

    /// Build once without watching.
    pub fn build_once(&mut self) -> Result<()> {
        let sources = self.find_sources()?;
        if self.options.verbose {
            println!("Found {} Rune source files", sources.len());
        }

        let parsed = Self::parse_sources(&sources)?;
        let analyses = Self::analyze_sources(&parsed)?;
        let generated = self.generate_code(&parsed, &analyses)?;
        self.write_generated(&generated)?;
        self.build_crate(matches!(self.options.mode, BuildMode::Release))?;
        self.setup_hot_reload()?;

        if self.options.verbose {
            println!("Build complete.");
        }
        Ok(())
    }

    fn find_sources(&self) -> Result<Vec<PathBuf>> {
        let src_dir = self.options.workspace
            .join("crates")
            .join(&self.config.build.target_crate)
            .join("src");
        parser::scan_directory(&src_dir)
    }

    fn parse_sources(sources: &[PathBuf]) -> Result<Vec<parser::SourceFile>> {
        sources.iter().map(|s| parser::parse_file(s.as_path())).collect()
    }

    fn analyze_sources(sources: &[parser::SourceFile]) -> Result<Vec<analyzer::AnalysisResult>> {
        sources.iter().map(analyzer::analyze).collect()
    }

    fn generate_code(
        &self,
        sources: &[parser::SourceFile],
        analyses: &[analyzer::AnalysisResult],
    ) -> Result<Vec<codegen::GeneratedModule>> {
        sources.iter()
            .zip(analyses.iter())
            .map(|(s, a)| codegen::generate(s, a))
            .collect()
    }

    fn build_crate(&self, release: bool) -> Result<()> {
        let manifest = self.cache.generated_cargo_toml();
        let mut cmd = ProcCommand::new("cargo");
        cmd.arg("build");
        if release {
            cmd.arg("--release");
        }
        cmd.arg("--manifest-path").arg(&manifest);

        if !self.options.verbose {
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
        }

        let output = cmd.output()
            .map_err(|e| crate::RuneError::Cargo(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(crate::RuneError::Cargo(stderr.to_string()));
        }
        Ok(())
    }

    fn setup_hot_reload(&self) -> Result<()> {
        let hot_dir = self.options.workspace.join("target/hot");
        std::fs::create_dir_all(&hot_dir)?;

        let profile = match self.options.mode {
            BuildMode::Dev => "debug",
            BuildMode::Release => "release",
        };

        let target_crate = &self.config.build.target_crate;
        let artifact_name = format!("lib{target_crate}.so");
        let artifact = self.options.workspace
            .join("target")
            .join(profile)
            .join(&artifact_name);

        if artifact.exists() {
            copy_artifact_to_hot_dir(&hot_dir, &artifact, target_crate)?;
        }
        Ok(())
    }
}

fn load_config(options: &BuildOptions) -> Result<RuneConfig> {
    let config_path = options.config.clone()
        .or_else(|| Some(options.workspace.join("rune.toml")));

    if let Some(ref path) = config_path {
        if path.exists() {
            return Ok(RuneConfig::load(path)?);
        }
    }
    Ok(RuneConfig::default())
}

fn init_dev_mode(driver: &mut BuildDriver) -> Result<()> {
    if driver.options.verbose {
        println!("Running in development mode with hot reload...");
    }
    driver.build_once()?;
    Ok(())
}

fn run_watch_loop(driver: &mut BuildDriver) -> Result<()> {
    let src_dir = find_src_dir(driver);
    let hot_dir = driver.options.workspace.join("target/hot");
    std::fs::create_dir_all(&hot_dir)?;

    let signaler = HostSignaler::new(&hot_dir)
        .map_err(|e| crate::RuneError::Reload(e.to_string()))?;

    let debounce = driver.config.dev.debounce;
    let watcher = DylibWatcher::new(&src_dir, debounce)
        .map_err(|e| crate::RuneError::Reload(e.to_string()))?;

    if driver.options.verbose {
        println!("Watching for changes in {}...", src_dir.display());
        println!("Press Ctrl+C to stop.");
    }

    loop {
        match watcher.wait_for_event(Duration::from_millis(500)) {
            Some(crate::reload::ReloadEvent::FilesChanged(_)) => handle_file_change(driver, &signaler),
            Some(crate::reload::ReloadEvent::ProtocolChanged) => {
                eprintln!("Protocol changed, full restart required.");
                let _ = signaler.mark_restart_needed();
                break;
            }
            Some(crate::reload::ReloadEvent::Error(e)) => {
                eprintln!("Watcher error: {}", e);
            }
            None => {}
        }
    }

    if driver.options.verbose {
        println!("Development server stopped.");
    }
    Ok(())
}

fn find_src_dir(driver: &BuildDriver) -> PathBuf {
    driver.options.workspace
        .join("crates")
        .join(&driver.config.build.target_crate)
        .join("src")
}

fn handle_file_change(driver: &mut BuildDriver, signaler: &HostSignaler) {
    if driver.options.verbose {
        println!("File changed, rebuilding...");
    }
    match driver.build_once() {
        Ok(()) => {
            if driver.options.verbose {
                println!("Build successful, hot reload ready.");
            }
            let _ = signaler.signal();
        }
        Err(e) => {
            eprintln!("Build failed: {}", e);
        }
    }
}

fn copy_artifact_to_hot_dir(
    hot_dir: &Path,
    artifact: &PathBuf,
    target_crate: &str,
) -> Result<()> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let safe_name = target_crate.replace('-', "_");
    let hot_name = format!("{safe_name}_{timestamp}.so");
    let hot_path = hot_dir.join(&hot_name);
    std::fs::copy(artifact, &hot_path)?;

    let current = hot_dir.join(".current");
    if current.exists() {
        std::fs::remove_file(&current)?;
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(&hot_path, &current)?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&hot_path, &current)?;

    println!("Hot reload ready: {hot_name}");
    Ok(())
}
