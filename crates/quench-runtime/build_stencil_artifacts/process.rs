fn rustc_path() -> String {
    env::var_os("QUENCH_RUSTC")
        .or_else(|| env::var_os("RUSTC"))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rustc".to_owned())
}

fn effective_rustflags() -> Vec<String> {
    env::var("CARGO_ENCODED_RUSTFLAGS")
        .unwrap_or_default()
        .split('\u{1f}')
        .filter(|flag| !flag.is_empty())
        .map(str::to_owned)
        .collect()
}

fn unique_directory() -> OwnedDirectory {
    let base = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .or_else(|| cfg!(test).then(env::temp_dir))
        .expect("OUT_DIR for Rust stencil artifacts");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    for attempt in 0..8u8 {
        let directory = base.join(format!(
            "stencil-objects-{stamp}-{}-{attempt}",
            std::process::id()
        ));
        if fs::create_dir(&directory).is_ok() {
            return OwnedDirectory { path: directory };
        }
    }
    panic!("cannot create unique Rust stencil object directory")
}

#[cfg(test)]
fn rustc_host_target(compiler: &str) -> String {
    command_output(Command::new(compiler).arg("-vV"), "read rustc host")
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc host target")
        .to_owned()
}

fn run(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("{description} failed: {error}"));
    assert!(status.success(), "{description} exited with {status}");
}

fn command_output(command: &mut Command, description: &str) -> String {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{description} failed: {error}"));
    assert!(
        output.status.success(),
        "{description} exited with {}",
        output.status
    );
    String::from_utf8(output.stdout).expect("Rust tool output is UTF-8")
}
