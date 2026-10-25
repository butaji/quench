//! The host most unit tests run under.
//!
//! The VM is generic over its `Host`, so every host type compiles the interpreter, its lane
//! handlers and the native API once more. With a type per test the unit-test build reached
//! 13.7 GB of compiler memory (49 host types, TOD-182). Tests instead describe their host as
//! data on this one type: the natives it installs, its install hook, the globals and modules
//! it provides, and any state its natives keep. Hosts whose behavior is a trait method this
//! type does not carry (promise lifecycle observers, compiled-program caches, Wasm imports,
//! snapshot resume) keep their own types.

use std::any::Any;
use std::cell::{Ref, RefCell};
use std::rc::Rc;

use crate::host::{Host, HostGlobal, ModuleLoadError, ModuleSource};
use crate::{CapabilityId, HostFunction, NativeContext, RootedError};

/// A test host's install step (`Host::initialize`).
pub(crate) type Install = fn(&mut NativeContext<'_, TestHost>) -> Result<(), RootedError>;

/// A test host's dynamic-import resolution (`Host::resolve_dynamic_import`), by specifier.
pub(crate) type Resolve = Rc<dyn Fn(&str) -> Result<Option<ModuleSource>, ModuleLoadError>>;

/// The `$262` global the Test262 harness creates realms through.
const TEST262_GLOBALS: &[HostGlobal] = &[HostGlobal {
    name: "$262",
    capability: CapabilityId::CreateRealm,
}];

/// A unit test's host. Clones share what it wrote and its state, so a test keeps a clone
/// to read them after the runtime takes the host.
#[derive(Clone)]
pub(crate) struct TestHost {
    lines: Rc<RefCell<Vec<String>>>,
    globals: &'static [HostGlobal],
    functions: &'static [HostFunction<TestHost>],
    install: Option<Install>,
    resolve: Option<Resolve>,
    module_source: Option<fn(&ModuleSource) -> bool>,
    state: Rc<dyn Any>,
}

impl Default for TestHost {
    fn default() -> Self {
        Self {
            lines: Rc::default(),
            globals: &[],
            functions: &[],
            install: None,
            resolve: None,
            module_source: None,
            state: Rc::new(()),
        }
    }
}

impl TestHost {
    /// A host that writes into `lines`.
    pub(crate) fn writing_to(lines: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            lines,
            ..Self::default()
        }
    }

    /// Also provide `$262`, through which the Test262 harness creates realms.
    pub(crate) fn with_test262(self) -> Self {
        Self {
            globals: TEST262_GLOBALS,
            ..self
        }
    }

    /// Provide no host globals.
    pub(crate) fn without_globals(self) -> Self {
        Self {
            globals: &[],
            ..self
        }
    }

    /// Install `functions` as the host's natives (`Host::functions`).
    pub(crate) fn with_natives(self, functions: &'static [HostFunction<TestHost>]) -> Self {
        Self { functions, ..self }
    }

    /// Run `install` when the runtime initializes the host (`Host::initialize`).
    pub(crate) fn with_install(self, install: Install) -> Self {
        Self {
            install: Some(install),
            ..self
        }
    }

    /// Resolve dynamic imports by specifier through `resolve`.
    pub(crate) fn with_modules(
        self,
        resolve: impl Fn(&str) -> Result<Option<ModuleSource>, ModuleLoadError> + 'static,
    ) -> Self {
        Self {
            resolve: Some(Rc::new(resolve)),
            ..self
        }
    }

    /// Answer `Host::has_module_source` through `module_source`.
    pub(crate) fn with_module_sources(self, module_source: fn(&ModuleSource) -> bool) -> Self {
        Self {
            module_source: Some(module_source),
            ..self
        }
    }

    /// Share `state` with the host's natives (`state`).
    pub(crate) fn with_state<T: 'static>(self, state: Rc<T>) -> Self {
        Self { state, ..self }
    }

    /// The state `with_state` shared, as the type it was given.
    pub(crate) fn state<T: 'static>(&self) -> Rc<T> {
        Rc::clone(&self.state)
            .downcast::<T>()
            .expect("test host state of the type its natives read")
    }

    /// The lines the host has written.
    pub(crate) fn lines(&self) -> Ref<'_, Vec<String>> {
        self.lines.borrow()
    }
}

impl Host for TestHost {
    fn write_line(&mut self, text: &str) {
        self.lines.borrow_mut().push(text.to_owned());
    }

    fn clock_millis(&mut self) -> f64 {
        0.0
    }

    fn globals(&self) -> &'static [HostGlobal] {
        self.globals
    }

    fn functions(&self) -> &[HostFunction<Self>] {
        self.functions
    }

    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        match context.host_mut().install {
            Some(install) => install(context),
            None => Ok(()),
        }
    }

    fn resolve_dynamic_import(
        &mut self,
        _referrer: &str,
        specifier: &str,
    ) -> Result<Option<ModuleSource>, ModuleLoadError> {
        match &self.resolve {
            Some(resolve) => resolve(specifier),
            None => Ok(None),
        }
    }

    fn has_module_source(&self, module: &ModuleSource) -> bool {
        self.module_source
            .is_some_and(|module_source| module_source(module))
    }
}
