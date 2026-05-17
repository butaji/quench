//! # Build Driver
//!
//! Main compilation orchestration with clean separation of concerns.

use super::cache::CacheManager;
use super::config::RuneConfig;
use crate::{analyzer, codegen, parser, Result};
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

/// Build mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
    /// Watch transpile file for changes
    pub watch_transpile: bool,
    /// Verbose output
    pub verbose: bool,
    /// Output JSON format
    pub json: bool,
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
            watch_transpile: false,
            verbose: false,
            json: false,
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
    pub options: BuildOptions,
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
        Ok(Self {
            options,
            config,
            cache,
        })
    }

    /// Run in development mode with hot reload.
    pub fn dev(&mut self) -> Result<()> {
        init_dev_mode(self)?;
        self.watch()
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
    pub fn transpile(&self) -> Result<()> {
        let file = self
            .options
            .transpile_file
            .as_ref()
            .ok_or_else(|| crate::RuneError::Codegen("No file specified".into()))?;

        if self.options.watch_transpile {
            self.transpile_watch(file)
        } else {
            self.transpile_once(file)
        }
    }

    fn transpile_once(&self, file: &Path) -> Result<()> {
        let source = parser::parse_file(file)?;
        let analysis = analyzer::analyze(&source)?;
        let module = codegen::generate(&source, &analysis)?;

        println!("{}", module.source);
        Ok(())
    }

    fn transpile_watch(&self, file: &Path) -> Result<()> {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        println!(
            "Watching {} for changes... (Ctrl+C to exit)",
            file.display()
        );
        let mut last_modified = std::fs::metadata(file).and_then(|m| m.modified()).ok();

        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let _ = ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        });

        while running.load(Ordering::SeqCst) {
            // Check for file changes
            if let Ok(current_modified) = std::fs::metadata(file).and_then(|m| m.modified()) {
                if last_modified != Some(current_modified) {
                    last_modified = Some(current_modified);
                    println!("\n--- Re-transpiling {} ---\n", file.display());
                    if let Err(e) = self.transpile_once(file) {
                        eprintln!("Error: {}", e);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        println!("\nWatch stopped.");
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
        let src_dir = self
            .options
            .workspace
            .join("crates")
            .join(&self.config.build.target_crate)
            .join("src");
        parser::scan_directory(&src_dir)
    }

    fn parse_sources(sources: &[PathBuf]) -> Result<Vec<parser::SourceFile>> {
        sources
            .iter()
            .map(|s| parser::parse_file(s.as_path()))
            .collect()
    }

    fn analyze_sources(sources: &[parser::SourceFile]) -> Result<Vec<analyzer::AnalysisResult>> {
        sources.iter().map(analyzer::analyze).collect()
    }

    fn generate_code(
        &self,
        sources: &[parser::SourceFile],
        analyses: &[analyzer::AnalysisResult],
    ) -> Result<Vec<codegen::GeneratedModule>> {
        sources
            .iter()
            .zip(analyses.iter())
            .map(|(s, a)| codegen::generate(s, a))
            .collect()
    }

    fn build_crate(&self, release: bool) -> Result<()> {
        let output = self.run_cargo_build(release)?;
        if !output.status.success() {
            self.handle_build_failure(&output);
            return Err(crate::RuneError::Cargo(
                "cargo build failed - see stderr above".into(),
            ));
        }
        Ok(())
    }

    fn run_cargo_build(&self, release: bool) -> std::io::Result<std::process::Output> {
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
        cmd.output()
    }

    fn handle_build_failure(&self, output: &std::process::Output) {
        use crate::reload::ErrorTranslator;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let translator = ErrorTranslator::new();
        let translated_errors = translator.translate_all(&stderr);

        for err in &translated_errors {
            eprintln!("{}", err);
        }

        if translated_errors.is_empty() {
            eprintln!("Compilation failed: {}", stderr);
        } else {
            eprintln!("Compilation failed. See errors above.");
        }
    }

    fn setup_hot_reload(&self) -> Result<()> {
        use super::artifacts;
        let hot_dir = self.options.workspace.join("target/hot");
        std::fs::create_dir_all(&hot_dir)?;

        let target_crate = &self.config.build.target_crate;
        let artifact = self
            .options
            .workspace
            .join("target")
            .join(self.get_profile())
            .join(format!("lib{target_crate}.so"));

        if artifact.exists() {
            artifacts::copy_artifact_to_hot_dir(&hot_dir, &artifact, target_crate)?;
        }
        Ok(())
    }

    fn get_profile(&self) -> &'static str {
        match self.options.mode {
            BuildMode::Dev => "debug",
            BuildMode::Release => "release",
        }
    }
}

fn load_config(options: &BuildOptions) -> Result<RuneConfig> {
    let config_path = options
        .config
        .clone()
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
