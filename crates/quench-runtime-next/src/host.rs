use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

/// Stable capability identifiers used at the VM/host boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum CapabilityId {
    WriteLine = 1,
    ClockMillis = 2,
    Done = 3,
    CreateRealm = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostGlobal {
    pub name: &'static str,
    pub capability: CapabilityId,
}

/// A host-resolved source unit. The VM owns parsing, module semantics, and
/// execution; the host supplies only source identity and bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleSource {
    pub name: String,
    pub source: String,
    pub bytes: Vec<u8>,
}

pub trait Host {
    fn write_line(&mut self, text: &str);
    fn clock_millis(&mut self) -> f64;

    /// Optional host-owned globals. The evaluator installs these only when
    /// the host explicitly advertises them; ordinary production hosts remain
    /// free of conformance or embedding-specific names.
    fn globals(&self) -> &'static [HostGlobal] {
        &[]
    }

    /// Resolve one dynamic-import request relative to its active source unit.
    /// `Ok(None)` means this host does not provide module loading.
    fn resolve_dynamic_import(
        &mut self,
        _referrer: &str,
        _specifier: &str,
    ) -> Result<Option<ModuleSource>, String> {
        Ok(None)
    }

    fn done(&mut self, _text: Option<&str>) {}

    /// Whether the active embedding permits a synchronous Atomics.wait.
    fn can_block(&self) -> bool {
        false
    }
}

/// Borrowed capability context. It exposes typed host effects without
/// handing heap cells or guest `Value` handles to the host.
pub struct HostContext<'a, H: Host> {
    host: &'a mut H,
}

impl<'a, H: Host> HostContext<'a, H> {
    pub(crate) fn new(host: &'a mut H) -> Self {
        Self { host }
    }

    pub(crate) fn invoke(&mut self, capability: CapabilityId, text: Option<&str>) -> f64 {
        match capability {
            CapabilityId::WriteLine => {
                self.host.write_line(text.unwrap_or_default());
                0.0
            }
            CapabilityId::ClockMillis => self.host.clock_millis(),
            CapabilityId::Done => {
                self.host.done(text);
                0.0
            }
            CapabilityId::CreateRealm => 0.0,
        }
    }
}

#[derive(Default)]
pub struct SystemHost;

impl Host for SystemHost {
    fn write_line(&mut self, text: &str) {
        let _ = writeln!(io::stdout().lock(), "{text}");
    }

    fn clock_millis(&mut self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
            * 1000.0
    }
}
