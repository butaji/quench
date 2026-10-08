use std::path::Path;

fn visit(directory: &Path, oversized: &mut Vec<(String, usize)>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            visit(&path, oversized);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let lines = std::fs::read_to_string(&path).unwrap().lines().count();
            if lines > 500 {
                oversized.push((path.display().to_string(), lines));
            }
        }
    }
}

#[test]
fn production_modules_stay_below_five_hundred_lines() {
    let mut oversized = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut oversized,
    );
    assert!(oversized.is_empty(), "oversized modules: {oversized:?}");
}
