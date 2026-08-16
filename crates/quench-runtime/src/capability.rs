//! Small, engine-neutral platform capability algebra.
//!
//! These traits describe what a host can do, not how Node exposes it. Node
//! modules, filesystem policy, and error compatibility belong in adapters.

use crate::{
    operation::{Operation, OperationKind},
    resource::ResourceId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    Read,
    Write,
    Open,
    Close,
    Spawn,
    Wait,
    Sleep,
    Resolve,
    Random,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityRequest {
    pub capability: Capability,
    pub resource: Option<ResourceId>,
}

impl CapabilityRequest {
    pub const fn new(capability: Capability, resource: Option<ResourceId>) -> Self {
        Self {
            capability,
            resource,
        }
    }

    pub const fn operation(self) -> Option<Operation> {
        let Some(resource) = self.resource else {
            return None;
        };
        let kind = match self.capability {
            Capability::Read => OperationKind::Read,
            Capability::Write => OperationKind::Write,
            Capability::Open => OperationKind::Open,
            Capability::Close => OperationKind::Close,
            Capability::Spawn => OperationKind::Wait,
            Capability::Wait => OperationKind::Wait,
            Capability::Sleep => OperationKind::Sleep,
            Capability::Resolve => OperationKind::Resolve,
            Capability::Random => OperationKind::Random,
        };
        Some(Operation::pending(resource, kind))
    }
}

pub trait Io {
    fn submit(&self, request: CapabilityRequest) -> Operation;
}

pub trait Clock {
    fn now_millis(&self) -> u64;
    fn sleep(&self, millis: u64) -> Operation;
}

pub trait Tasks {
    fn submit(&self, operation: Operation) -> Operation;
}

pub trait Process {
    fn spawn(&self, resource: ResourceId) -> Operation;
}

pub trait Crypto {
    fn random(&self, resource: ResourceId) -> Operation;
}

pub trait Runtime {
    fn io(&self) -> &dyn Io;
    fn clock(&self) -> &dyn Clock;
    fn tasks(&self) -> &dyn Tasks;
    fn process(&self) -> &dyn Process;
    fn crypto(&self) -> &dyn Crypto;
}
