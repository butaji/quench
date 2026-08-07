//! Generic engine façade and replaceable runtime-component boundary.

use crate::{Context, JsError, Value};
use std::marker::PhantomData;

/// Marker bound for a runtime subsystem implementation.
pub trait RuntimeComponent: 'static {}

macro_rules! default_component {
    ($name:ident) => {
        #[derive(Debug, Default)]
        pub struct $name;
        impl RuntimeComponent for $name {}
    };
}

default_component!(DefaultHeap);
default_component!(DefaultCollector);
default_component!(DefaultAllocator);
default_component!(DefaultFrames);
default_component!(DefaultExecutor);
default_component!(DefaultExceptions);
default_component!(DefaultEnvironments);

/// JavaScript runtime with replaceable execution subsystems.
pub struct Runtime<
    Heap: RuntimeComponent,
    Collector: RuntimeComponent,
    Allocator: RuntimeComponent,
    Frames: RuntimeComponent,
    Executor: RuntimeComponent,
    Exceptions: RuntimeComponent,
    Environments: RuntimeComponent,
> {
    context: Context,
    components: PhantomData<(
        Heap,
        Collector,
        Allocator,
        Frames,
        Executor,
        Exceptions,
        Environments,
    )>,
}

impl<
        Heap: RuntimeComponent,
        Collector: RuntimeComponent,
        Allocator: RuntimeComponent,
        Frames: RuntimeComponent,
        Executor: RuntimeComponent,
        Exceptions: RuntimeComponent,
        Environments: RuntimeComponent,
    > Runtime<Heap, Collector, Allocator, Frames, Executor, Exceptions, Environments>
{
    /// Construct a runtime with a fresh realm and default builtins.
    pub fn new() -> Result<Self, JsError> {
        Ok(Self {
            context: Context::new()?,
            components: PhantomData,
        })
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
