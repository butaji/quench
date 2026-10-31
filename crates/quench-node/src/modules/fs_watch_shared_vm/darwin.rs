//! Darwin transport: kqueue for files, FSEvents for directories, as in libuv.
//! The current VM thread pumps CF once without waiting; callbacks only queue data.
use super::{EventKind, KernelEvent, WatchDescriptor};
#[path = "darwin_ffi.rs"]
mod ffi;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ffi::{c_void, CStr, CString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use std::rc::Rc;

const VNODE_EVENTS: u32 = libc::NOTE_ATTRIB
    | libc::NOTE_WRITE
    | libc::NOTE_RENAME
    | libc::NOTE_DELETE
    | libc::NOTE_EXTEND
    | libc::NOTE_REVOKE;
const VNODE_CHANGE: u32 = libc::NOTE_ATTRIB | libc::NOTE_EXTEND;
type Queue = Rc<RefCell<VecDeque<KernelEvent>>>;
struct DirectoryContext {
    descriptor: WatchDescriptor,
    path: Vec<u8>,
    pending: Queue,
}
struct Directory {
    stream: ffi::Stream,
    // Stable allocation remains live through Stop/Invalidate/Release.
    _context: Box<DirectoryContext>,
}
impl Drop for Directory {
    fn drop(&mut self) {
        // SAFETY: stream was scheduled and started on this VM thread. Invalidating
        // removes its run-loop source before the boxed callback context is freed.
        unsafe {
            ffi::FSEventStreamStop(self.stream);
            ffi::FSEventStreamInvalidate(self.stream);
            ffi::FSEventStreamRelease(self.stream);
        }
    }
}
struct FileWatch {
    file: File,
    armed: bool,
}
enum Subscription {
    File(FileWatch),
    Directory { _stream: Directory },
}
#[derive(Default)]
pub(crate) struct Kernel {
    descriptor: Option<OwnedFd>,
    subscriptions: HashMap<WatchDescriptor, Subscription>,
    next_id: i32,
    pending: Queue,
}
impl Kernel {
    fn descriptor(&mut self) -> io::Result<&OwnedFd> {
        if self.descriptor.is_none() {
            // SAFETY: kqueue returns a newly owned descriptor or -1.
            let descriptor = unsafe { libc::kqueue() };
            if descriptor < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: transfer ownership once, including failure cleanup below.
            let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
            // SAFETY: valid fd; no descriptor flags other than close-on-exec exist here.
            if unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
            self.descriptor = Some(descriptor);
        }
        Ok(self.descriptor.as_ref().expect("initialized kqueue"))
    }
    pub(crate) fn subscribe(&mut self, path: &Path) -> io::Result<WatchDescriptor> {
        let file = File::open(path)?;
        // Transport IDs do not reuse closed subscription identities. Exhaustion is
        // an ordinary registration error, never wraps onto a pending old event.
        let id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EMFILE))?;
        let descriptor = WatchDescriptor(id);
        let subscription = if file.metadata()?.is_dir() {
            Subscription::Directory {
                _stream: Directory::start(path, descriptor, self.pending.clone())?,
            }
        } else {
            let queue = self.descriptor()?.as_raw_fd();
            arm(queue, file.as_raw_fd(), descriptor)?;
            Subscription::File(FileWatch { file, armed: true })
        };
        self.next_id = id;
        self.subscriptions.insert(descriptor, subscription);
        Ok(descriptor)
    }
    pub(crate) fn unsubscribe(&mut self, descriptor: WatchDescriptor) {
        // Closing a file removes its kevent; Directory::drop invalidates its stream.
        self.subscriptions.remove(&descriptor);
        self.pending
            .borrow_mut()
            .retain(|event| event.descriptor != descriptor);
    }
    pub(crate) fn next_event(&mut self) -> io::Result<Option<KernelEvent>> {
        if let Some(event) = self.pending.borrow_mut().pop_front() {
            return Ok(Some(event));
        }
        if self
            .subscriptions
            .values()
            .any(|entry| matches!(entry, Subscription::Directory { .. }))
        {
            // SAFETY: streams are scheduled only on the VM's current thread.
            // Apple specifies seconds=0 as one nonblocking pass through ready sources.
            unsafe {
                ffi::CFRunLoopRunInMode(
                    ffi::kCFRunLoopDefaultMode,
                    ffi::POLL_WITHOUT_WAIT_SECONDS,
                    u8::from(true),
                );
            }
            if let Some(event) = self.pending.borrow_mut().pop_front() {
                return Ok(Some(event));
            }
        }
        let Some(queue) = &self.descriptor else {
            return Ok(None);
        };
        let queue = queue.as_raw_fd();
        for (id, subscription) in &mut self.subscriptions {
            if let Subscription::File(file) = subscription {
                if !file.armed {
                    arm(queue, file.file.as_raw_fd(), *id)?;
                    file.armed = true;
                }
            }
        }
        let timeout = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: zero initializes every field in the platform kevent ABI.
        let mut event: libc::kevent = unsafe { std::mem::zeroed() };
        let count = loop {
            // SAFETY: live kqueue, one writable event, nonblocking timeout.
            let count = unsafe { libc::kevent(queue, ptr::null(), 0, &mut event, 1, &timeout) };
            if count >= 0 {
                break count;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        };
        if count == 0 {
            return Ok(None);
        }
        if event.flags & libc::EV_ERROR != 0 {
            return Err(io::Error::from_raw_os_error(event.data as i32));
        }
        let id = WatchDescriptor(event.udata as isize as i32);
        let Some(Subscription::File(file)) = self.subscriptions.get_mut(&id) else {
            return Ok(None);
        };
        file.armed = false;
        let mut path = [0_u8; libc::PATH_MAX as usize];
        // SAFETY: F_GETPATH writes a PATH_MAX-sized, NUL-terminated path on success.
        let name =
            if unsafe { libc::fcntl(file.file.as_raw_fd(), libc::F_GETPATH, path.as_mut_ptr()) }
                == 0
            {
                let end = path
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(path.len());
                Some(
                    path[..end]
                        .rsplit(|byte| *byte == b'/')
                        .next()
                        .unwrap_or_default()
                        .to_vec(),
                )
            } else {
                None
            };
        Ok(Some(KernelEvent {
            descriptor: id,
            kind: if event.fflags & VNODE_CHANGE != 0 {
                EventKind::Change
            } else {
                EventKind::Rename
            },
            name,
        }))
    }
    pub(crate) fn reset(&mut self) {
        self.subscriptions.clear();
        self.pending.borrow_mut().clear();
        self.descriptor = None;
    }
    pub(crate) fn snapshot_quiescent(&self) -> bool {
        self.subscriptions.is_empty()
            && self.descriptor.is_none()
            && self.pending.borrow().is_empty()
    }
}
fn arm(queue: i32, file: i32, id: WatchDescriptor) -> io::Result<()> {
    let event = libc::kevent {
        ident: file as usize,
        filter: libc::EVFILT_VNODE,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: VNODE_EVENTS,
        data: 0,
        udata: id.0 as isize as *mut c_void,
    };
    // SAFETY: queue/file are owned, event initialized, no output events requested.
    if unsafe { libc::kevent(queue, &event, 1, ptr::null_mut(), 0, ptr::null()) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
impl Directory {
    fn start(path: &Path, descriptor: WatchDescriptor, pending: Queue) -> io::Result<Self> {
        let path = std::fs::canonicalize(path)?;
        let bytes = path.as_os_str().as_bytes();
        let c_path = CString::new(bytes).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
        let mut context = Box::new(DirectoryContext {
            descriptor,
            path: bytes.to_vec(),
            pending,
        });
        let mut abi_context = ffi::Context {
            version: ffi::CONTEXT_VERSION,
            info: ptr::from_mut(context.as_mut()).cast(),
            retain: None,
            release: None,
            description: None,
        };
        // SAFETY: terminated canonical filesystem path; CF objects are released on
        // all branches. The array retains its strings through the canonical CF type callbacks.
        let stream = unsafe {
            let string =
                ffi::CFStringCreateWithFileSystemRepresentation(ptr::null(), c_path.as_ptr());
            if string.is_null() {
                return Err(io::Error::from_raw_os_error(libc::ENOMEM));
            }
            let paths = ffi::CFArrayCreate(
                ptr::null(),
                &string,
                1,
                ptr::addr_of!(ffi::kCFTypeArrayCallBacks).cast(),
            );
            if paths.is_null() {
                ffi::CFRelease(string);
                return Err(io::Error::from_raw_os_error(libc::ENOMEM));
            }
            let stream = ffi::FSEventStreamCreate(
                ptr::null(),
                directory_callback,
                &mut abi_context,
                paths,
                ffi::SINCE_NOW,
                ffi::DELIVERY_LATENCY_SECONDS,
                ffi::CREATE_NO_DEFER | ffi::CREATE_FILE_EVENTS,
            );
            ffi::CFRelease(paths);
            ffi::CFRelease(string);
            if stream.is_null() {
                return Err(io::Error::from_raw_os_error(libc::ENOMEM));
            }
            ffi::FSEventStreamScheduleWithRunLoop(
                stream,
                ffi::CFRunLoopGetCurrent(),
                ffi::kCFRunLoopDefaultMode,
            );
            if ffi::FSEventStreamStart(stream) == 0 {
                ffi::FSEventStreamInvalidate(stream);
                ffi::FSEventStreamRelease(stream);
                return Err(io::Error::from_raw_os_error(libc::EMFILE));
            }
            stream
        };
        Ok(Self {
            stream,
            _context: context,
        })
    }
}
fn directory_event(base: &[u8], path: &[u8], mut flags: u32) -> Option<(EventKind, Vec<u8>)> {
    if flags & ffi::SYSTEM_EVENTS != 0 {
        return None;
    }
    let name = if base == b"/" {
        path
    } else if path == base {
        // libuv suppresses meaningless created/removed flags on the watched root.
        flags &= !ffi::RENAMED;
        base.rsplit(|byte| *byte == b'/').next().unwrap_or_default()
    } else {
        path.strip_prefix(base)?.strip_prefix(b"/")?
    };
    // Recursive policy already composes public per-directory watchers.
    if name.get(1..).is_some_and(|tail| tail.contains(&b'/')) {
        return None;
    }
    let kind = if flags & ffi::RENAMED == 0
        && (flags & ffi::MODIFIED != 0 || flags & ffi::IS_DIRECTORY == 0)
    {
        EventKind::Change
    } else {
        EventKind::Rename
    };
    Some((kind, name.to_vec()))
}
unsafe extern "C" fn directory_callback(
    _: ffi::Object,
    info: *mut c_void,
    count: usize,
    paths: *mut c_void,
    flags: *const u32,
    _: *const u64,
) {
    // SAFETY: FSEvents supplies these arrays for count elements and the context
    // remains boxed until the stream is invalidated on this same run-loop thread.
    let context = unsafe { &*info.cast::<DirectoryContext>() };
    let mut pending = context.pending.borrow_mut();
    for index in 0..count {
        let path = unsafe { CStr::from_ptr(*paths.cast::<*const std::ffi::c_char>().add(index)) }
            .to_bytes();
        let flags = unsafe { *flags.add(index) };
        if let Some((kind, name)) = directory_event(&context.path, path, flags) {
            pending.push_back(KernelEvent {
                descriptor: context.descriptor,
                kind,
                name: Some(name),
            });
        }
    }
}

#[cfg(test)]
#[path = "darwin_tests.rs"]
mod tests;
