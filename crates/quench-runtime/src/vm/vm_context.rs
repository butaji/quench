pub use scope::ExecutionScope;
pub type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;
pub(crate) fn with_realm<T>(realm: RealmId, callback: impl FnOnce() -> T) -> Option<T> {
    realm::with_realm(realm, callback)
}

pub(crate) fn global_builtin_exists(key: &str) -> bool {
    realm::global_builtin_exists(key) && !is_legacy_global(key)
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

pub(crate) fn is_legacy_global(key: &str) -> bool {
    matches!(
        key,
        "parseFloat"
            | "parseInt"
            | "decodeURI"
            | "decodeURIComponent"
            | "encodeURI"
            | "encodeURIComponent"
    )
}

pub(crate) fn global_builtin_value(key: &str) -> Option<Value> {
    crate::globals::builtin(key).map(Value::Builtin)
}

pub(crate) fn realm_token(realm: RealmId) -> Option<Value> {
    realm::token(realm).map(Value::HostCapability)
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

pub(crate) fn realm_is_intrinsic(bound: &crate::value::BoundFunctionValue) -> bool {
    is_intrinsic_bound(bound)
}
type ObjectProperties = Rc<crate::value::ObjectData>;
#[derive(Clone)]
pub struct VmContext {
    output_sink: Option<OutputSink>,
    realm: RealmId,
    capabilities: Vec<HostCapabilityRef>,
    host_bindings: Vec<(String, HostCapabilityRef)>,
}
impl Default for VmContext {
    fn default() -> Self {
        Self {
            output_sink: None,
            realm: RealmId::ROOT,
            capabilities: Vec::new(),
            host_bindings: Vec::new(),
        }
    }
}
thread_local! {
    static CURRENT_CONTEXT: RefCell<Option<VmContext>> = const { RefCell::new(None) };
    static GLOBAL_OBJECT: RefCell<Option<ObjectProperties>> = const { RefCell::new(None) };
}
struct ContextGuard {
    previous: Option<VmContext>,
}
impl ContextGuard {
    fn install(context: &VmContext) -> Self {
        let previous = CURRENT_CONTEXT.with(|current| current.replace(Some(context.clone())));
        Self { previous }
    }
}
impl Drop for ContextGuard {
    fn drop(&mut self) {
        CURRENT_CONTEXT.with(|current| current.replace(self.previous.take()));
    }
}

impl VmContext {
    pub fn with_output_sink(output_sink: OutputSink) -> Self {
        Self {
            output_sink: Some(output_sink),
            ..Self::default()
        }
    }

    pub fn with_host_capability(
        mut self,
        name: impl Into<String>,
        value: HostCapabilityRef,
    ) -> Self {
        self.host_bindings.push((name.into(), value));
        self
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

    pub fn realm(&self) -> RealmId {
        self.realm
    }

    pub fn has_capability(&self, kind: HostCapabilityKind) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability.kind == kind)
    }

    pub(crate) fn permits(&self, capability: HostCapabilityRef) -> bool {
        capability.realm == self.realm && self.has_capability(capability.kind)
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
}
