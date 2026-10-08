pub(crate) fn type_str() -> String {
    if cfg!(target_os = "macos") {
        "Darwin".into()
    } else if cfg!(target_os = "linux") {
        "Linux".into()
    } else if cfg!(target_os = "windows") {
        "Windows_NT".into()
    } else {
        "Unknown".into()
    }
}

pub(crate) fn total_memory_bytes() -> u64 {
    sysinfo_total()
}

fn sysinfo_total() -> u64 {
    sysinfo::System::new_all().total_memory()
}
