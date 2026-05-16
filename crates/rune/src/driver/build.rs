//! # Build Driver
//!
//! Main compilation orchestration.

use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;
use crate::{Result, parser, analyzer, codegen};
use super::config::{RuneConfig, TargetCrate};
use super::cache::CacheManager;

/// Build mode.
#[derive(Debug, Clone, Copy)]
pub enum BuildMode {
    /// Development with hot reload
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
    pub fn new(workspace: PathBuf) -> Self {
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
    pub fn dev(mut self) -> Self {
        self.mode = BuildMode::Dev;
        self
    }

    /// Set release mode.
    pub fn release(mut self) -> Self {
        self.mode = BuildMode::Release;
        self
    }
}

/// Main build driver.
pub struct BuildDriver {
    options: BuildOptions,
    config: RuneConfig,
    cache: CacheManager,
}

impl BuildDriver {
    /// Create a new build driver.
    pub fn new(options: BuildOptions) -> Result<Self> {
        let config_path = options.config.clone()
            .or_else(|| options.workspace.join("rune.toml"));

        let config = if config_path.exists() {
            RuneConfig::load(&config_path)?
        } else {
            RuneConfig::default()
        };

        let cache = CacheManager::new(&options.workspace)?;

        Ok(Self {
            options,
            config,
            cache,
        })
    }

    /// Run in development mode with hot reload.
    pub fn dev(&mut self) -> Result<()> {
        if self.options.verbose {
            println!("Running in development mode with hot reload...");
        }

        // Scan for source files
        let sources = self.find_sources()?;
        if self.options.verbose {
            println!("Found {} Rune source files", sources.len());
        }

        // Parse and validate all sources
        let parsed = self.parse_sources(&sources)?;

        // Analyze all sources
        let analyses = self.analyze_sources(&parsed)?;

        // Generate Rust code
        let generated = self.generate_code(&parsed, &analyses)?;

        // Write to cache
        self.write_generated(&generated)?;

        // Build the crate
        self.build_crate(false)?;

        // Copy to hot directory
        self.setup_hot_reload()?;

        if self.options.verbose {
            println!("Development build complete. Dylib ready.");
        }

        Ok(())
    }

    /// Run in release mode.
    pub fn build(&mut self) -> Result<()> {
        if self.options.verbose {
            println!("Running in release mode...");
        }

        // Scan for source files
        let sources = self.find_sources()?;

        // Parse and analyze
        let parsed = self.parse_sources(&sources)?;
        let analyses = self.analyze_sources(&parsed)?;

        // Generate code
        let generated = self.generate_code(&parsed, &analyses)?;

        // Write to cache
        self.write_generated(&generated)?;

        // Build release
        self.build_crate(true)?;

        if self.options.verbose {
            println!("Release build complete.");
        }

        Ok(())
    }

    /// Type check only.
    pub fn check(&mut self) -> Result<()> {
        let sources = self.find_sources()?;
        let parsed = self.parse_sources(&sources)?;
        let analyses = self.analyze_sources(&parsed)?;

        for (source, analysis) in parsed.iter().zip(analyses.iter()) {
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
    pub fn init(&mut self) -> Result<()> {
        let project_name = self.config.project.name.clone();
        let target_crate = self.config.build.target_crate.clone();

        // Create directory structure
        let base = self.options.workspace.join("crates").join(&target_crate);
        std::fs::create_dir_all(base.join("src/native"))?;
        std::fs::create_dir_all(base.join("src/views"))?;

        // Create Cargo.toml
        let cargo = format!(r#"# Auto-generated by rune
[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[lib]
name = "{}"
path = "src/lib.rs"

[dependencies]
protocol = {{ path = "../protocol" }}
ratatui = "0.26"

[build-dependencies]
rune = {{ path = "../../.." }}
"#, target_crate, target_crate);

        std::fs::write(base.join("Cargo.toml"), cargo)?;

        // Create lib.rs
        let lib = r#"mod native;

pub struct AppState {
    pub tasks: Vec<Task>,
}

#[derive(Clone)]
pub struct Task {
    pub id: i32,
    pub title: String,
    pub done: bool,
}

#[no_mangle]
pub extern "C" fn create_app() -> *mut dyn protocol::App {
    Box::into_raw(Box::new(AppImpl))
}

struct AppImpl;

impl protocol::App for AppImpl {
    fn update(&mut self, _state: &mut AppState) {
        // TODO: Add update logic
    }

    fn render(&self, _term: &mut impl ratatui::backend::Backend, _state: &AppState) {
        // TODO: Add render logic
    }
}
"#;

        std::fs::write(base.join("src/lib.rs"), lib)?;

        // Create main.r.ts
        let main_ts = r#"// Main entry point
import { AppState, Task } from "./state.r.ts";

export function update(state: AppState): void {
    // Update logic here
}

export function render(state: AppState): void {
    // Render logic here
}
"#;

        std::fs::write(base.join("src/main.r.ts"), main_ts)?;

        // Create state.r.ts
        let state_ts = r#"// Application state types

export type Task = {
    id: number,
    title: string,
    done: boolean,
};

export type AppState = {
    tasks: Task[],
    selected: number,
};

export function createTask(title: string): Task {
    return {
        id: Date.now(),
        title,
        done: false,
    };
}

export function toggleTask(task: Task): Task {
    return { ...task, done: !task.done };
}
"#;

        std::fs::write(base.join("src/state.r.ts"), state_ts)?;

        // Create native module
        let native_mod = r#"pub mod fast_math;

pub fn batch_toggle<T>(items: &mut [T]) where T: Clone {
    for item in items.iter_mut() {
        // Toggle logic
    }
}
"#;

        std::fs::write(base.join("src/native/mod.rs"), native_mod)?;

        let fast_math = r#"/// Fast square root using native Rust
pub fn fast_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Batch operation on numbers
pub fn batch_add(values: &[f64], n: f64) -> Vec<f64> {
    values.iter().map(|v| v + n).collect()
}
"#;

        std::fs::write(base.join("src/native/fast_math.rs"), fast_math)?;

        println!("Initialized Rune project: {}", project_name);
        println!("Created structure in crates/{}/", target_crate);

        Ok(())
    }

    /// Find all Rune source files.
    fn find_sources(&self) -> Result<Vec<PathBuf>> {
        let src_dir = self.options.workspace
            .join("crates")
            .join(&self.config.build.target_crate)
            .join("src");

        parser::scan_directory(&src_dir)
    }

    /// Parse all source files.
    fn parse_sources(&self, sources: &[PathBuf]) -> Result<Vec<parser::SourceFile>> {
        sources.iter().map(|s| parser::parse_file(s)).collect()
    }

    /// Analyze all sources.
    fn analyze_sources(&self, sources: &[parser::SourceFile]) -> Result<Vec<analyzer::AnalysisResult>] {
        sources.iter().map(|s| analyzer::analyze(s)).collect()
    }

    /// Generate Rust code from analyzed sources.
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

    /// Write generated code to cache.
    fn write_generated(&self, modules: &[codegen::GeneratedModule]) -> Result<()> {
        let cache_dir = self.cache.generated_dir();

        for module in modules {
            let rel_path = module.name.replace(".r", "");
            let out_path = cache_dir.join(format!("{}.rs", rel_path));
            std::fs::create_dir_all(out_path.parent().unwrap())?;
            std::fs::write(&out_path, &module.source)?;
        }

        Ok(())
    }

    /// Build the target crate.
    fn build_crate(&self, release: bool) -> Result<()> {
        let manifest = self.cache.generated_cargo_toml();
        let mut cmd = ProcCommand::new("cargo");
        cmd.arg("build");
        if release {
            cmd.arg("--release");
        }
        cmd.arg("--manifest-path").arg(&manifest);

        if !self.options.verbose {
            cmd.stdout(ProcCommand::Stdio::piped());
            cmd.stderr(ProcCommand::Stdio::piped());
        }

        let output = cmd.output()
            .map_err(|e| crate::RuneError::Cargo(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(crate::RuneError::Cargo(stderr.to_string()));
        }

        Ok(())
    }

    /// Set up hot reload directory.
    fn setup_hot_reload(&self) -> Result<()> {
        let hot_dir = self.options.workspace.join("target/hot");
        std::fs::create_dir_all(&hot_dir)?;

        // Find the built dylib
        let profile = if matches!(self.options.mode, BuildMode::Dev) {
            "debug"
        } else {
            "release"
        };

        let target_crate = &self.config.build.target_crate;
        let artifact_name = format!("lib{}.{}", target_crate, std::env::consts::DLIB_EXT);

        let artifact = self.options.workspace
            .join("target")
            .join(profile)
            .join(&artifact_name);

        if artifact.exists() {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis();

            let hot_name = format!("{}_{}.{}", artifact_name.trim_start_matches("lib"), timestamp, std::env::consts::DLIB_EXT);
            let hot_path = hot_dir.join(&hot_name);

            std::fs::copy(&artifact, &hot_path)?;

            // Update symlink
            let current = hot_dir.join(".current");
            if current.exists() {
                std::fs::remove_file(&current)?;
            }

            #[cfg(unix)]
            std::os::unix::fs::symlink(&hot_path, &current)?;

            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&hot_path, &current)?;

            if self.options.verbose {
                println!("Hot reload ready: {}", hot_name);
            }
        }

        Ok(())
    }
}
