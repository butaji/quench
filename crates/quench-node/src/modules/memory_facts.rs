//! Process memory limits from the same OS sources used by libuv.
use std::path::{Path, PathBuf};

#[derive(Clone)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum Controller {
    Unified(PathBuf),
    Legacy(PathBuf),
}
fn number(path: impl AsRef<Path>) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|value| {
            let value = value.trim();
            if value == "max" {
                Some(u64::MAX)
            } else {
                value.parse().ok()
            }
        })
        .unwrap_or(0)
}
impl Controller {
    fn discover() -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            let source = std::fs::read_to_string("/proc/self/cgroup").ok()?;
            if let Some(path) = source
                .strip_prefix("0::/")
                .and_then(|value| value.lines().next())
            {
                return Some(Self::Unified(Path::new("/sys/fs/cgroup").join(path)));
            }
            let path = source
                .lines()
                .find_map(|line| {
                    let mut fields = line.splitn(3, ':');
                    fields.next()?;
                    (fields.next()? == "memory")
                        .then(|| fields.next())
                        .flatten()
                })
                .unwrap_or("/");
            Some(Self::Legacy(
                Path::new("/sys/fs/cgroup/memory").join(path.trim_start_matches('/')),
            ))
        }
        #[cfg(not(target_os = "linux"))]
        None
    }
    fn limits(&self) -> Option<u64> {
        let (mut high, mut maximum) = match self {
            Self::Unified(path) => (
                number(path.join("memory.high")),
                number(path.join("memory.max")),
            ),
            Self::Legacy(path) => {
                let mut high = number(path.join("memory.soft_limit_in_bytes"));
                let mut maximum = number(path.join("memory.limit_in_bytes"));
                if high == 0 || maximum == 0 {
                    high = number("/sys/fs/cgroup/memory/memory.soft_limit_in_bytes");
                    maximum = number("/sys/fs/cgroup/memory/memory.limit_in_bytes");
                }
                (high, maximum)
            }
        };
        if matches!(self, Self::Legacy(_)) {
            #[cfg(unix)]
            {
                // cgroup v1's unlimited sentinel is page-aligned LONG_MAX.
                let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
                if page > 0 {
                    let unlimited = (libc::c_long::MAX as u64) & !((page as u64) - 1);
                    if high == unlimited {
                        high = u64::MAX;
                    }
                    if maximum == unlimited {
                        maximum = u64::MAX;
                    }
                }
            }
        }
        (high != 0 && maximum != 0).then_some(high.min(maximum))
    }
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn current(&self) -> u64 {
        match self {
            Self::Unified(path) => number(path.join("memory.current")),
            Self::Legacy(path) => {
                let value = number(path.join("memory.usage_in_bytes"));
                if value == 0 {
                    number("/sys/fs/cgroup/memory/memory.usage_in_bytes")
                } else {
                    value
                }
            }
        }
    }
}
fn resource_limit() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut smallest = 0;
        for resource in [libc::RLIMIT_AS, libc::RLIMIT_DATA] {
            let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
            // SAFETY: getrlimit initializes this correctly sized output on success.
            if unsafe { libc::getrlimit(resource, limit.as_mut_ptr()) } == 0 {
                let limit = unsafe { limit.assume_init() }.rlim_cur;
                if limit != libc::RLIM_INFINITY && (smallest == 0 || (limit as u64) < smallest) {
                    smallest = limit as u64;
                }
            }
        }
        (smallest != 0).then_some(smallest)
    }
    #[cfg(not(unix))]
    None
}
pub(crate) fn constrained() -> u64 {
    let cgroup = Controller::discover().and_then(|controller| controller.limits());
    match (cgroup, resource_limit()) {
        (Some(group), Some(resource)) => group.min(resource),
        (Some(limit), None) | (None, Some(limit)) => limit,
        (None, None) => 0,
    }
}
pub(crate) fn available() -> u64 {
    #[cfg(target_os = "linux")]
    {
        // libuv reports zero if the controller inventory itself cannot be read.
        let Some(controller) = Controller::discover() else {
            return 0;
        };
        let physical = super::os_facts::memory_info();
        if let Some(limit) = controller.limits().filter(|limit| *limit <= physical.total) {
            return limit.saturating_sub(controller.current());
        }
        physical.available
    }
    #[cfg(not(target_os = "linux"))]
    super::os_facts::memory_info().available
}
