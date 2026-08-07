//! Generic engine façade and replaceable runtime-component boundary.

use crate::{Context, JsError, Value};
/// Marker bound for a runtime subsystem implementation.
pub trait RuntimeComponent: 'static {}

/// Object/value storage boundary.
pub trait Heap: RuntimeComponent {}
/// Garbage-collection policy boundary.
pub trait Collector: RuntimeComponent {}
/// Allocation policy boundary.
pub trait Allocator: RuntimeComponent {}
/// Call-frame representation boundary.
pub trait Frames: RuntimeComponent {}
/// Evaluation scheduling boundary.
pub trait Executor: RuntimeComponent {}
/// Error and abrupt-completion boundary.
pub trait Exceptions: RuntimeComponent {}
/// Lexical and variable-environment boundary.
pub trait Environments: RuntimeComponent {}

macro_rules! default_component {
    ($name:ident, $trait:ident) => {
        #[derive(Debug, Default)]
        pub struct $name;
        impl RuntimeComponent for $name {}
        impl $trait for $name {}
    };
}

default_component!(DefaultHeap, Heap);
default_component!(DefaultCollector, Collector);
default_component!(DefaultAllocator, Allocator);
default_component!(DefaultFrames, Frames);
default_component!(DefaultExecutor, Executor);
default_component!(DefaultExceptions, Exceptions);
default_component!(DefaultEnvironments, Environments);

/// JavaScript runtime with replaceable execution subsystems.
pub struct Runtime<
    Heap: crate::runtime::Heap,
    Collector: crate::runtime::Collector,
    Allocator: crate::runtime::Allocator,
    Frames: crate::runtime::Frames,
    Executor: crate::runtime::Executor,
    Exceptions: crate::runtime::Exceptions,
    Environments: crate::runtime::Environments,
> {
    context: Context,
    heap: Heap,
    collector: Collector,
    allocator: Allocator,
    frames: Frames,
    executor: Executor,
    exceptions: Exceptions,
    environments: Environments,
}

impl<
        Heap: crate::runtime::Heap,
        Collector: crate::runtime::Collector,
        Allocator: crate::runtime::Allocator,
        Frames: crate::runtime::Frames,
        Executor: crate::runtime::Executor,
        Exceptions: crate::runtime::Exceptions,
        Environments: crate::runtime::Environments,
    > Runtime<Heap, Collector, Allocator, Frames, Executor, Exceptions, Environments>
{
    /// Construct a runtime with explicitly selected subsystem instances.
    pub fn with_components(
        heap: Heap,
        collector: Collector,
        allocator: Allocator,
        frames: Frames,
        executor: Executor,
        exceptions: Exceptions,
        environments: Environments,
    ) -> Result<Self, JsError> {
        Ok(Self {
            context: Context::new()?,
            heap,
            collector,
            allocator,
            frames,
            executor,
            exceptions,
            environments,
        })
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }
    pub fn collector(&self) -> &Collector {
        &self.collector
    }
    pub fn allocator(&self) -> &Allocator {
        &self.allocator
    }
    pub fn frames(&self) -> &Frames {
        &self.frames
    }
    pub fn executor(&self) -> &Executor {
        &self.executor
    }
    pub fn exceptions(&self) -> &Exceptions {
        &self.exceptions
    }
    pub fn environments(&self) -> &Environments {
        &self.environments
    }

    /// Evaluate script source through the AST → Quench IR → interpreter path.
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        self.context.eval(source)
    }

    /// Evaluate module source through the same pipeline in module mode.
    pub fn eval_es_module(&mut self, source: &str) -> Result<Value, JsError> {
        self.context.eval_es_module(source)
    }

    /// Reset the realm while retaining the runtime component selection.
    pub fn reset(&mut self) -> Result<(), JsError> {
        self.context.reset()
    }
}

impl
    Runtime<
        DefaultHeap,
        DefaultCollector,
        DefaultAllocator,
        DefaultFrames,
        DefaultExecutor,
        DefaultExceptions,
        DefaultEnvironments,
    >
{
    /// Construct a runtime with the production default subsystem set.
    pub fn new() -> Result<Self, JsError> {
        Self::with_components(
            DefaultHeap,
            DefaultCollector,
            DefaultAllocator,
            DefaultFrames,
            DefaultExecutor,
            DefaultExceptions,
            DefaultEnvironments,
        )
    }
}
