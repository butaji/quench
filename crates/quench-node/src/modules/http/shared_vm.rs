//! Shared-VM HTTP projection over the shared TCP transport.

mod operations;
mod poll;
mod state;

use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) use operations::{
    agent_create, agent_destroy, create_server, get, response_end, response_set_header,
    server_address, server_close, server_listen,
};
pub(crate) use state::State;

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    operations::module(context)
}

pub(crate) fn poll(
    runtime: &mut rqj::Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    state: &Rc<RefCell<crate::host::HostState>>,
) -> Result<bool, String> {
    poll::poll(runtime, program, state)
}

pub(crate) fn has_work(state: &Rc<RefCell<crate::host::HostState>>) -> bool {
    let host = state.borrow();
    host.http.shared.servers.values().any(|server| {
        server.listener.is_some() || !server.connections.is_empty()
    })
        || !host.http.shared.clients.is_empty()
        || crate::modules::net::shared_vm::has_work(&host.net)
}

pub(crate) fn cleanup(runtime: &mut rqj::Runtime<NodeHost>, state: &Rc<RefCell<crate::host::HostState>>) {
    poll::cleanup(runtime, state)
}
