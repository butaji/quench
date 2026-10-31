use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "quench-darwin-watch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn event(kernel: &mut Kernel, descriptor: WatchDescriptor, name: &[u8], kind: Option<EventKind>) {
    // Failure deadline for real host delivery, not a production scheduling policy.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(event) = kernel.next_event().unwrap() {
            if event.descriptor == descriptor
                && event.name.as_deref() == Some(name)
                && kind.is_none_or(|kind| event.kind == kind)
            {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "missing watch event {name:?} {kind:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn directory_filter_matches_libuv_path_and_flag_policy() {
    assert_eq!(
        directory_event(b"/tmp/a", b"/tmp/a/file", ffi::RENAMED),
        Some((EventKind::Rename, b"file".to_vec()))
    );
    assert_eq!(
        directory_event(b"/tmp/a", b"/tmp/ab/file", ffi::MODIFIED),
        None
    );
    assert_eq!(
        directory_event(b"/tmp/a", b"/tmp/a/child/file", ffi::MODIFIED),
        None
    );
    assert_eq!(
        directory_event(b"/tmp/a", b"/tmp/a", ffi::RENAMED | ffi::IS_DIRECTORY),
        Some((EventKind::Rename, b"a".to_vec()))
    );
    assert_eq!(
        directory_event(b"/", b"/file", ffi::MODIFIED),
        Some((EventKind::Change, b"/file".to_vec()))
    );
    assert_eq!(
        directory_event(b"/tmp/a", b"/tmp/a/file", ffi::SYSTEM_EVENTS),
        None
    );
}
#[test]
fn kqueue_file_subscription_rearms_and_stops() {
    let fixture = Fixture::new();
    let path = fixture.0.join("file");
    std::fs::write(&path, b"first").unwrap();
    let mut kernel = Kernel::default();
    let watch = kernel.subscribe(&path).unwrap();
    for data in [b"second", b"third!"] {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(data).unwrap();
        event(&mut kernel, watch, b"file", Some(EventKind::Change));
        // A subsequent poll rearms the one-shot subscription before another write.
        assert!(kernel.next_event().unwrap().is_none());
    }
    kernel.unsubscribe(watch);
    std::fs::write(&path, b"closed").unwrap();
    assert!(kernel.next_event().unwrap().is_none());
    kernel.reset();
    assert!(kernel.snapshot_quiescent());
}
#[test]
fn fsevents_directory_creation_modification_and_close() {
    let fixture = Fixture::new();
    let mut kernel = Kernel::default();
    let watch = kernel.subscribe(&fixture.0).unwrap();
    let path = fixture.0.join("file");
    std::fs::write(&path, b"first").unwrap();
    event(&mut kernel, watch, b"file", Some(EventKind::Rename));
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"second")
        .unwrap();
    // FSEvents can coalesce a rewrite with creation and retain ItemCreated;
    // libuv then reports Rename. Both must deliver the actual filename.
    event(&mut kernel, watch, b"file", None);
    kernel.unsubscribe(watch);
    std::fs::remove_file(&path).unwrap();
    assert!(kernel.next_event().unwrap().is_none());
    kernel.reset();
    assert!(kernel.snapshot_quiescent());
}
