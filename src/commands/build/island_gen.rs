//! Island generation

use std::path::Path;
use walkdir::WalkDir;

use crate::commands::build::IslandEntry;

/// Scan islands directory for island files
pub fn scan_islands(project_root: &Path) -> Vec<IslandEntry> {
    let islands_dir = project_root.join("islands");
    let mut islands = Vec::new();

    if !islands_dir.exists() {
        return islands;
    }

    for entry in WalkDir::new(&islands_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "tsx" || ext == "ts" {
                let name = path.file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Island")
                    .to_string();

                islands.push(IslandEntry {
                    name,
                    file: path.to_path_buf(),
                    props: vec![],
                });
            }
        }
    }

    islands
}

/// Generate islands manifest
pub fn generate_manifest(islands: &[IslandEntry]) -> String {
    let mut output = String::new();
    output.push_str("//! Auto-generated islands manifest\n\n");
    output.push_str("use serde::{Serialize, Deserialize};\n\n");

    output.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    output.push_str("pub struct Island {\n");
    output.push_str("    pub name: String,\n");
    output.push_str("    pub props: Vec<Prop>,\n");
    output.push_str("}\n\n");

    output.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    output.push_str("pub struct Prop {\n");
    output.push_str("    pub name: String,\n");
    output.push_str("    pub type_: String,\n");
    output.push_str("}\n\n");

    output.push_str("pub fn islands() -> Vec<Island> {\n");
    output.push_str("    vec![\n");

    for island in islands {
        output.push_str(&format!(
            "        Island {{ name: \"{}\".to_string(), props: vec![] }},\n",
            island.name
        ));
    }

    output.push_str("    ]\n");
    output.push_str("}\n");

    output
}
