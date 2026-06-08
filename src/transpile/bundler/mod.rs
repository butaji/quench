//! Module bundler for TSX → JS transpilation.

mod bundler;
mod imports;
mod transform;

pub use bundler::Bundler;
#[allow(unused_imports)]
pub use bundler::{ModuleData, parse_tsx, transform_and_codegen};
#[allow(unused_imports)]
pub use transform::{prefix_declarations, rename_default_export, rename_module_declarations, rewrite_import_to_global};

use std::path::Path;

use crate::transpile::js_bundle::react_shim::REACT_SHIM;
use crate::transpile::js_bundle::runtime_shim::{POST_SHIM, PRE_SHIM};

pub fn transpile_to_js_bundled(entry_path: &Path) -> anyhow::Result<String> {
    let mut bundler = Bundler::new();
    let from_dir = entry_path.parent().unwrap_or(Path::new("."));

    bundler.resolve_modules(entry_path, from_dir)?;

    let entry_canonical = entry_path.canonicalize().unwrap_or_else(|_| entry_path.to_path_buf());
    let mut ordered: Vec<_> = bundler.module_index.keys().cloned().collect();
    ordered.sort();

    for path in &ordered {
        bundler.transpile_modules(path)?;
    }

    bundler.rewrite_imports();

    build_bundle_output(&bundler, &entry_canonical)
}

fn build_bundle_output(bundler: &Bundler, entry_canonical: &Path) -> anyhow::Result<String> {
    let mut output = REACT_SHIM.to_string();
    output.push_str(PRE_SHIM);
    output.push_str("\n// Bundled modules\n");

    for module in &bundler.modules {
        output.push_str(&module.js);
        output.push('\n');
    }

    let entry_module = bundler.modules.iter().find(|m| m.path == *entry_canonical);
    if let Some(module) = entry_module {
        if let Some(default_fn) = module.exports.get("default") {
            let original = default_fn.strip_prefix("__m0_").unwrap_or(default_fn);
            output.push_str(&format!(
                "\nvar __runts_default = React._withHooks(typeof {0} !== 'undefined' ? {0} : {1});",
                default_fn, original
            ));
        }
    }

    output.push_str(POST_SHIM);
    Ok(output)
}
