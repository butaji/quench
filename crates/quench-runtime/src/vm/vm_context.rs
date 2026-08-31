pub use scope::ExecutionScope;
pub type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;
pub trait Host: 'static {
    fn call(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError>;

    fn construct(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let _ = (capability, arguments);
        Err(VmError::NotCallable)
    }

    fn construct_with_new_target(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
        _new_target: &Value,
    ) -> Result<Value, VmError> {
        self.construct(capability, arguments)
    }
}
pub(crate) fn with_realm<T>(realm: RealmId, callback: impl FnOnce() -> T) -> Option<T> {
    realm::with_realm(realm, callback)
}

pub(crate) fn global_builtin_exists(key: &str) -> bool {
    (current_context_or_default().host_value(key).is_some()
        || current_context_or_default().host_binding(key).is_some()
        || (realm::global_builtin_exists(key) && !is_legacy_global(key)))
        || crate::globals::immutable_value(key).is_some()
}

pub(crate) fn global_builtin_exists_for_object(
    object: &std::rc::Rc<crate::value::ObjectData>,
    key: &str,
) -> bool {
    if realm::id_for_global(object).is_none() {
        return false;
    }
    realm::global_builtin_exists(key) && !is_legacy_global(key)
}

pub(crate) fn is_legacy_global(_key: &str) -> bool {
    false
}

pub(crate) fn global_builtin_value(key: &str) -> Option<Value> {
    crate::globals::builtin(key)
        .map(|builtin| realm_intrinsic_for(current_context_or_default().realm(), builtin))
}

pub(crate) fn realm_token(realm: RealmId) -> Option<Value> {
    realm::token(realm).map(Value::HostCapability)
}

pub(crate) fn realm_global_value(realm: RealmId) -> Option<Value> {
    realm::global(realm)
}

pub(crate) fn realm_id_for_global_value(value: &Value) -> Option<RealmId> {
    let Value::Object(object) = value else {
        return None;
    };
    realm::id_for_global(object)
}

pub(crate) fn realm_intrinsic(builtin: Builtin) -> Value {
    realm_intrinsic_for(current_context_or_default().realm(), builtin)
}

pub(crate) fn realm_intrinsic_for(realm: RealmId, builtin: Builtin) -> Value {
    realm::intrinsic(realm, builtin).unwrap_or(Value::Builtin(builtin))
}

pub(crate) fn is_intrinsic_bound(bound: &crate::value::BoundFunctionValue) -> bool {
    realm::is_intrinsic(bound)
}

pub(crate) fn value_realm(value: &Value, builtin: Builtin) -> Option<RealmId> {
    if matches!(value, Value::Builtin(candidate) if *candidate == builtin) {
        return Some(RealmId::ROOT);
    }
    let Value::BoundFunction(bound) = value else {
        return None;
    };
    let Value::HostCapability(token) = &bound.receiver else {
        return None;
    };
    if token.realm() != bound.realm {
        return None;
    }
    Some(bound.realm)
}

pub(crate) fn intrinsic_realm(value: &Value, builtin: Builtin) -> Option<RealmId> {
    if matches!(value, Value::Builtin(candidate) if *candidate == builtin) {
        return Some(RealmId::ROOT);
    }
    value_realm(value, builtin)
}

pub(crate) fn intl_fallback_symbol(realm: RealmId) -> Option<Value> {
    realm::intl_fallback_symbol(realm)
}

type ObjectProperties = Rc<crate::value::ObjectData>;
#[derive(Clone)]
pub struct VmContext {
    output_sink: Option<OutputSink>,
    host: Option<Rc<dyn Host>>,
    realm: RealmId,
    capabilities: Vec<HostCapabilityRef>,
    host_bindings: Vec<(String, HostCapabilityRef)>,
    host_values: Vec<(String, Value)>,
    persistent_host_values: Vec<String>,
    can_block: bool,
    source_text: Option<Rc<str>>,
    /// Optional cooperative execution budget for host re-entry. The normal
    /// VM has no budget; shared-realm hosts can bound one logical process and
    /// return control to their state machine instead of blocking forever.
    execution_budget: Option<Rc<std::cell::Cell<usize>>>,
}
impl Default for VmContext {
    fn default() -> Self {
        Self {
            output_sink: None,
            host: None,
            realm: RealmId::ROOT,
            capabilities: Vec::new(),
            host_bindings: Vec::new(),
            host_values: Vec::new(),
            persistent_host_values: Vec::new(),
            can_block: false,
            source_text: None,
            execution_budget: None,
        }
    }
}
thread_local! {
    static CURRENT_CONTEXT: RefCell<Option<Rc<VmContext>>> = const { RefCell::new(None) };
    static GLOBAL_OBJECT: RefCell<Option<ObjectProperties>> = const { RefCell::new(None) };
}
struct ContextGuard {
    previous: Option<Rc<VmContext>>,
    installed: bool,
}
impl ContextGuard {
    fn install(context: &VmContext) -> Self {
        let already_installed = CURRENT_CONTEXT.with(|current| {
            current
                .borrow()
                .as_ref()
                .is_some_and(|installed| std::ptr::eq(installed.as_ref(), context))
        });
        if already_installed {
            return Self {
                previous: None,
                installed: false,
            };
        }
        let previous = CURRENT_CONTEXT.with(|current| {
            current.replace(Some(Rc::new(context.clone())))
        });
        Self {
            previous,
            installed: true,
        }
    }
}
impl Drop for ContextGuard {
    fn drop(&mut self) {
        if self.installed {
            CURRENT_CONTEXT.with(|current| current.replace(self.previous.take()));
        }
    }
}

/// Run `f` with `context` installed as this thread's current context.
pub fn with_current_context<T>(context: &VmContext, f: impl FnOnce() -> T) -> T {
    let _guard = ContextGuard::install(context);
    f()
}

impl VmContext {
    pub fn with_output_sink(output_sink: OutputSink) -> Self {
        Self {
            output_sink: Some(output_sink),
            ..Self::default()
        }
    }

    pub fn with_host(mut self, host: Rc<dyn Host>) -> Self {
        self.host = Some(host);
        self
    }

    pub fn with_can_block(mut self, can_block: bool) -> Self {
        self.can_block = can_block;
        self
    }

    pub fn with_source_text(mut self, source: impl Into<Rc<str>>) -> Self {
        self.source_text = Some(source.into());
        self
    }

    /// Bound one top-level execution without changing ordinary VM semantics.
    /// The counter is shared by cloned contexts so nested calls consume the
    /// same residual budget.
    pub fn with_execution_budget(mut self, budget: usize) -> Self {
        self.execution_budget = Some(Rc::new(std::cell::Cell::new(budget)));
        self
    }

    pub(crate) fn consume_execution_budget(&self) -> bool {
        let Some(budget) = &self.execution_budget else {
            return true;
        };
        let remaining = budget.get();
        if remaining == 0 {
            return false;
        }
        budget.set(remaining - 1);
        true
    }

    pub fn source_text(&self) -> Option<&str> {
        self.source_text.as_deref()
    }

    pub(crate) fn can_block(&self) -> bool {
        self.can_block
    }

    pub(crate) fn host_handle(&self) -> Option<Rc<dyn Host>> {
        self.host.clone()
    }

    pub(crate) fn construct_host_with_new_target(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
        new_target: &Value,
    ) -> Option<Result<Value, VmError>> {
        self.host
            .as_ref()
            .map(|host| host.construct_with_new_target(capability, arguments, new_target))
    }

    pub fn with_host_capability(
        mut self,
        name: impl Into<String>,
        value: HostCapabilityRef,
    ) -> Self {
        self.host_bindings.push((name.into(), value));
        self
    }

    pub fn with_host_value(mut self, name: impl Into<String>, value: Value) -> Self {
        self.host_values.push((name.into(), value));
        self
    }

    /// Install a host value that must remain observable after the installing
    /// VM frame yields to a callback. The value still lives in the same host
    /// table; this flag only derives its first-read materialization policy.
    pub fn with_persistent_host_value(mut self, name: impl Into<String>, value: Value) -> Self {
        let name = name.into();
        self.persistent_host_values.push(name.clone());
        self.host_values.push((name, value));
        self
    }

    pub(crate) fn host_value(&self, name: &str) -> Option<Value> {
        self.host_values
            .iter()
            .rev()
            .find_map(|(key, value)| (key == name).then_some(value.clone()))
    }

    pub(crate) fn host_value_is_persistent(&self, name: &str) -> bool {
        self.persistent_host_values.iter().any(|key| key == name)
    }

    pub(crate) fn host_binding(&self, name: &str) -> Option<HostCapabilityRef> {
        self.host_bindings
            .iter()
            .rev()
            .find_map(|(key, value)| (key == name).then_some(*value))
    }

    pub fn for_realm(realm: RealmId, capabilities: Vec<HostCapabilityKind>) -> Self {
        let capabilities = capabilities
            .into_iter()
            .map(|kind| HostCapabilityRef { realm, kind })
            .collect();
        Self {
            realm,
            capabilities,
            ..Self::default()
        }
    }

    /// A new realm and host bindings for one top-level evaluation.
    pub fn isolated() -> Self {
        let parent = Self::for_realm(
            RealmId::ROOT,
            vec![
                HostCapabilityKind::GetGlobal,
                HostCapabilityKind::CreateRealm,
                HostCapabilityKind::EvalScript,
                HostCapabilityKind::DetachArrayBuffer,
                HostCapabilityKind::IsHTMLDDA,
            ],
        );
        let realm = realm::create(&parent);
        realm::context(realm).unwrap_or(parent)
    }

    /// Create a child realm inheriting host capabilities and bindings while
    /// giving intrinsic constructors their own identity.
    pub fn child_realm(&self) -> Self {
        let realm = realm::create(self);
        realm::context(realm).unwrap_or_else(|| self.clone())
    }

    pub fn realm(&self) -> RealmId {
        self.realm
    }

    pub fn has_capability(&self, kind: HostCapabilityKind) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability.kind == kind)
    }

    pub(crate) fn permits(&self, capability: HostCapabilityRef) -> bool {
        capability.realm == self.realm
            && (self.has_capability(capability.kind)
                || (self.host.is_some()
                    && matches!(capability.kind, HostCapabilityKind::Custom(_))))
    }

    pub fn emit_output(&self, text: &str) {
        if let Some(output_sink) = &self.output_sink {
            output_sink(text);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VmError {
    RegisterOutOfBounds(u16),
    MissingReturn,
    Break(Option<String>),
    Continue(Option<String>),
    NotCallable,
    EvalError(String),
    Thrown(Value),
    Suspended(Rc<crate::value::PromiseData>),
    Interrupted,
}
