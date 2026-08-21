// Real-metadata `fs.Stats` construction. The legacy `fs_stats(mode)` in
// `js_runtime_fs_a.rs` only carries the file type; this builds a Stats
// object from the actual `std::fs::Metadata` so timestamp/size fields match
// the filesystem (number or bigint variant), as `fs.stat*`/`lstat*` do.
fn system_time_millis(time: std::io::Result<std::time::SystemTime>) -> Option<f64> {
    time.ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| {
            duration.as_secs() as f64 * 1000.0 + f64::from(duration.subsec_nanos()) / 1_000_000.0
        })
}

fn stat_time_value(requested: Option<f64>, bigint: bool) -> Value {
    let millis = requested.unwrap_or(0.0);
    if bigint {
        Value::BigInt((millis.floor() as i64).to_string())
    } else {
        Value::Number(millis)
    }
}

fn stat_size_value(size: u64, bigint: bool) -> Value {
    if bigint {
        Value::BigInt(size.to_string())
    } else {
        Value::Number(size as f64)
    }
}

fn stat_date_value(requested: Option<f64>) -> Value {
    quench_runtime::date::instance(requested.unwrap_or(0.0))
}

fn stat_bigint_requested(arguments: &[Value]) -> bool {
    arguments.get(1..).unwrap_or_default().iter().any(|value| {
        quench_runtime::execute::get_property_result(value, "bigint")
            == Ok(Value::Boolean(true))
    })
}

fn fs_stats_full(metadata: &std::fs::Metadata, bigint: bool) -> Value {
    let is_directory = metadata.is_dir();
    let directory_method = if is_directory {
        CapabilityName::FsStatsIsDirectory
    } else {
        CapabilityName::FsStatsIsDirectoryFile
    };
    let file_method = if metadata.file_type().is_file() {
        CapabilityName::FsDirentFile
    } else {
        CapabilityName::FsStatsIsFile
    };
    let atime = system_time_millis(metadata.accessed());
    let mtime = system_time_millis(metadata.modified());
    let ctime = system_time_millis(metadata.created());
    Value::object(vec![
        (
            "mode".into(),
            Value::Number(metadata_mode(metadata, is_directory) as f64),
        ),
        ("size".into(), stat_size_value(metadata.len(), bigint)),
        ("atime".into(), stat_date_value(atime)),
        ("mtime".into(), stat_date_value(mtime)),
        ("ctime".into(), stat_date_value(ctime)),
        ("birthtime".into(), stat_date_value(ctime)),
        ("atimeMs".into(), stat_time_value(atime, bigint)),
        ("mtimeMs".into(), stat_time_value(mtime, bigint)),
        ("ctimeMs".into(), stat_time_value(ctime, bigint)),
        ("birthtimeMs".into(), stat_time_value(ctime, bigint)),
        (
            "isDirectory".into(),
            capability_function(HostCapabilityKind::Custom(directory_method)),
        ),
        (
            "isFile".into(),
            capability_function(HostCapabilityKind::Custom(file_method)),
        ),
        (
            "isSymbolicLink".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::FsStatsIsNotSymbolicLink,
            )),
        ),
    ])
}

#[cfg(unix)]
fn metadata_mode(metadata: &std::fs::Metadata, is_directory: bool) -> u32 {
    std::os::unix::fs::PermissionsExt::mode(&metadata.permissions())
        | if is_directory { 0o40000 } else { 0o100000 }
}

#[cfg(not(unix))]
fn metadata_mode(_metadata: &std::fs::Metadata, is_directory: bool) -> u32 {
    if is_directory { 0o40777 } else { 0o100666 }
}