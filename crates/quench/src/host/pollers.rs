//! The event loop's polled I/O sources, each behind the host capability that owns it. A
//! compiled image's catalog names only the sources its program can reach (`aot::cli`), so a
//! program that cannot create a socket links no transport, and with it no `openssl`.

use crate::host::NodeHost;
use quench_runtime::{ResidualProgram, Runtime};

/// Every polled source, for a host that runs arbitrary source (the CLI, an embedding, tests).
pub fn all() -> super::HostPollers {
    super::HostPollers {
        child_ipc: Some(child_ipc),
        child_process: Some(child_process),
        carriers: Some(carriers),
        transport: Some(transport),
        worker: Some(worker),
        napi: Some(napi),
        dns: Some(dns),
    }
}

fn shared(runtime: &mut Runtime<NodeHost>) -> std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>> {
    runtime.host_mut().shared_state()
}

pub fn child_ipc(runtime: &mut Runtime<NodeHost>, program: &ResidualProgram) -> Result<bool, String> {
    crate::modules::child_process_ipc_shared_vm::poll(runtime, program)
}

pub fn child_process(runtime: &mut Runtime<NodeHost>, program: &ResidualProgram) -> Result<bool, String> {
    let state = shared(runtime);
    crate::modules::child_process_shared_vm::poll(runtime, program, &state)
}

/// The c-ares request queue, used by `dns.lookup`'s resolver when it is not the system one.
pub fn carriers(runtime: &mut Runtime<NodeHost>, program: &ResidualProgram) -> Result<bool, String> {
    crate::modules::cares_binding::poll(runtime, program)
}

/// UDP and TCP sockets: one turn of both sources, as `poll_io` settles them with one job drain.
pub fn transport(runtime: &mut Runtime<NodeHost>, program: &ResidualProgram) -> Result<bool, String> {
    let state = shared(runtime);
    let udp = crate::modules::udp_binding::poll(runtime, &state)?;
    let tcp = crate::modules::net_shared_vm::transport_poll::poll(runtime, program, &state)?;
    Ok(tcp || udp)
}

pub fn worker(runtime: &mut Runtime<NodeHost>, program: &ResidualProgram) -> Result<bool, String> {
    let state = shared(runtime);
    crate::modules::worker_shared_vm::poll(runtime, program, &state)
}

pub fn napi(runtime: &mut Runtime<NodeHost>, program: &ResidualProgram) -> Result<bool, String> {
    crate::modules::napi::poll(runtime, program)
}

pub fn dns(runtime: &mut Runtime<NodeHost>, program: &ResidualProgram) -> Result<bool, String> {
    let state = shared(runtime);
    crate::modules::dns_shared_vm::poll(runtime, program, &state)
}
