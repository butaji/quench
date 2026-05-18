//! # Path Utilities
//!
//! Path manipulation utilities for code generation.

use std::path::{Path, PathBuf};

/// Calculate relative path from a file to a target directory.
#[must_use]
pub fn relative_path(from_file: &Path, to_target: &Path) -> String {
    let Some(from_dir) = from_file.parent() else {
        return to_target.to_string_lossy().to_string();
    };

    let (ups, common_base) = count_ups_to_common_base(from_dir, to_target);
    let mut parts: Vec<String> = vec![String::from(".."); ups];

    if let Ok(rest) = to_target.strip_prefix(&common_base) {
        parts.extend(extract_path_components(rest));
    }

    parts.join("/")
}

fn count_ups_to_common_base(from_dir: &Path, to_target: &Path) -> (usize, PathBuf) {
    let mut ups = 0;
    let mut current = from_dir.to_path_buf();
    while !to_target.starts_with(&current) {
        if let Some(parent) = current.parent() {
            ups += 1;
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    (ups, current)
}

fn extract_path_components(rest: &Path) -> Vec<String> {
    rest.components()
        .filter_map(|c| {
            if let std::path::Component::Normal(s) = c {
                Some(s.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect()
}

/// Clean a module name by removing .r.ts/.r.tsx extension.
#[must_use]
pub fn clean_module_name(name: &str) -> String {
    if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
        && name.len() > 1
    {
        String::from(&name[..name.len() - 2])
    } else {
        String::from(name)
    }
}

/// Copy directory recursively.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::fs;

    fs::create_dir_all(dst)?;

    for entry in walkdir::WalkDir::new(src)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let rel_path = path
            .strip_prefix(src)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let dest_path = dst.join(rel_path);

        if path.is_dir() {
            fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &dest_path)?;
        }
    }

    Ok(())
}
