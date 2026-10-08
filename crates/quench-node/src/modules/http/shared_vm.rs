//! Shared-VM HTTP projection over the shared TCP transport.

mod client;
mod operations;
mod poll;
mod protocol;
mod state;

use crate::host::NodeHost;
use quench_runtime_next::{NativeContext, RootId, RootedError};
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) use client::{
    create as request_create, destroy as request_destroy,
    destroy_response as client_response_destroy, end as request_end,
    normalize_url as request_normalize_url, remove_header as request_remove_header,
    write as request_write,
};
pub(crate) use operations::{
    agent_create, agent_destroy, create_server, response_destroy, response_finish,
    response_get_header, response_remove_header, response_set_header, response_write,
    response_write_head, server_address, server_close, server_listen,
};
pub(crate) use state::State;

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    operations::module(context)
}

pub(crate) fn poll(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    program: &quench_runtime_next::ResidualProgram,
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
) -> Result<bool, String> {
    poll::poll(runtime, program, shared_state)
}

pub(crate) fn has_work(shared_state: &Rc<RefCell<crate::host::SharedNodeState>>) -> bool {
    let host = shared_state.borrow();
    host.http
        .servers
        .values()
        .any(|server| server.listener.is_some() || !server.connections.is_empty())
        || !host.http.clients.is_empty()
        || crate::modules::net::shared_vm::has_work(&host.tcp)
}

pub(crate) fn cleanup(
    runtime: &mut quench_runtime_next::Runtime<NodeHost>,
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
) {
    poll::cleanup(runtime, shared_state)
}
