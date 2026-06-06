use std::collections::HashMap;

#[test]
fn debug_ink_text_props() {
    let src = include_str!("../examples/ink-text-props/tui/app.tsx");
    // We can't access parser directly, but we can invoke runts codegen command
    let out = std::process::Command::new("cargo")
        .args(["run", "--bin", "runts", "--", "codegen", "--source", "examples/ink-text-props/tui/app.tsx"])
        .current_dir("..")
        .output()
        .expect("cargo run failed");
    eprintln!("stdout: {}", String::from_utf8_lossy(&out.stdout));
    eprintln!("stderr: {}", String::from_utf8_lossy(&out.stderr));
}
