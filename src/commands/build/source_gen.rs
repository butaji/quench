//! Source file generation

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::commands::build::{ComponentEntry, GeneratedFile, IslandEntry, RouteEntry};

/// Scan components directory for component files
pub fn scan_components(project_root: &Path) -> Vec<ComponentEntry> {
    let components_dir = project_root.join("components");
    let mut components = Vec::new();

    if !components_dir.exists() {
        return components;
    }

    for entry in WalkDir::new(&components_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "tsx" || ext == "ts" {
                let name = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Component")
                    .to_string();

                components.push(ComponentEntry {
                    name,
                    file: path.to_path_buf(),
                });
            }
        }
    }

    components
}

/// Generate source files from TypeScript
pub fn generate_all(files: &[PathBuf]) -> Result<Vec<GeneratedFile>, anyhow::Error> {
    let mut generated = Vec::new();

    for file in files {
        let relative = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("mod.rs")
            .replace(".tsx", ".rs")
            .replace(".ts", ".rs");

        generated.push(GeneratedFile {
            path: PathBuf::from(format!("src/gen/{}", relative)),
            content: format!("// Generated from {}\n", file.display()),
        });
    }

    Ok(generated)
}

/// Generate lib.rs
pub fn generate_lib(
    _routes: &[RouteEntry],
    islands: &[IslandEntry],
    components: &[ComponentEntry],
) -> String {
    let mut output = String::new();
    output.push_str("//! Auto-generated library\n\n");

    output.push_str("pub mod routes;\n");
    output.push_str("pub mod islands;\n");
    output.push_str("pub mod gen;\n\n");

    output.push_str(&format!("// {} routes\n", _routes.len()));
    output.push_str(&format!("// {} islands\n", islands.len()));
    output.push_str(&format!("// {} components\n", components.len()));

    output
}

/// Generate main.rs
pub fn generate_main() -> String {
    let mut output = String::new();
    output.push_str("//! Auto-generated main\n\n");
    output.push_str("use runts_lib::runtime::prelude::*;\n\n");
    output.push_str("#[tokio::main]\n");
    output.push_str("async fn main() {\n");
    output.push_str("    println!(\"Starting server...\");\n");
    output.push_str("}\n");
    output
}

/// Generate mod files
pub fn generate_mod_files(build_dir: &Path) -> Result<(), anyhow::Error> {
    use std::fs;

    fs::create_dir_all(build_dir.join("src/gen"))?;
    fs::write(build_dir.join("src/gen/mod.rs"), "//! Generated modules\n")?;

    Ok(())
}
