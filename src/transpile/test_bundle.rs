use std::path::Path;

pub fn test_bundler() {
    let entry = Path::new("examples/tui-counter/tui/app.tsx");
    match crate::transpile::bundler::transpile_to_js_bundled(entry) {
        Ok(js) => {
            eprintln!("=== Generated JS (first 3000 chars) ===");
            for (i, line) in js.lines().take(100).enumerate() {
                eprintln!("{:3}: {}", i+1, line);
            }
        }
        Err(e) => eprintln!("Error: {:?}", e),
    }
}
