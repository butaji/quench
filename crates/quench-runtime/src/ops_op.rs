#[derive(Debug, Clone, PartialEq)]
pub enum ArrayElement {
    Value(u16),
    Elision,
    Spread(u16),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstanceFieldKeyOp {
    Static(String),
    Dynamic(u16),
    Private(crate::facts::PrivateNameId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstanceFieldInitializerOp {
    pub body: crate::machine::FunctionCode,
    pub captures: u16,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateAccessorOp {
    pub get: Option<u16>,
    pub set: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppendInstanceFieldOp {
    pub constructor: u16,
    pub key: InstanceFieldKeyOp,
    pub initializer: Option<InstanceFieldInitializerOp>,
    pub is_static: bool,
    /// Register holding the element value directly (private methods), bypassing
    /// an initializer executable.
    pub value: Option<u16>,
    /// Accessor functions for a private accessor element.
    pub accessor: Option<PrivateAccessorOp>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    TraceSite {
        source: u32,
    },
    Const {
        dst: u16,
        value: Constant,
    },
    StoreLocal {
        slot: u16,
        src: u16,
    },
    Move {
        dst: u16,
        src: u16,
    },
    MakeRest {
        slot: u16,
        arguments: u16,
        skip: u16,
    },
    AliasLocal {
        slot: u16,
        source: u16,
    },
    StoreFunctionName {
        slot: u16,
        src: u16,
        strict: bool,
    },
    LoadCurrentGlobal {
        dst: u16,
    },
    MarkUninitialized {
        slot: u16,
        shared: bool,
    },
    MarkImmutable {
        slot: u16,
    },
    CheckInitialized {
        slot: u16,
        name: String,
    },
    InitializeLocal {
        slot: u16,
    },
    LoadParameter {
        dst: u16,
        slot: u16,
    },
    DeclareEvalBinding {
        name: String,
        slot: u16,
    },
    DeclareGlobalLexicalBinding {
        name: String,
        slot: u16,
        immutable: bool,
    },
    DeleteEvalBinding {
        dst: u16,
        name: String,
        slot: u16,
    },
    DeleteName {
        dst: u16,
        name: String,
        strict: bool,
    },
    CheckGlobalFunction {
        name: String,
    },
    CheckGlobalVar {
        name: String,
        is_lexical: bool,
        is_eval: bool,
    },
    CreateGlobalFunction {
        name: String,
        slot: u16,
        deletable: bool,
    },
    CreateGlobalVar {
        name: String,
        slot: u16,
        deletable: bool,
        is_lexical: bool,
    },
    ResolveBindingTarget {
        dst: u16,
        name: String,
    },
    ResolveActiveBindingTarget {
        dst: u16,
        name: String,
    },
    InitializeResolvedBinding {
        target: u16,
        slot: u16,
        name: String,
        src: u16,
    },
    SetResolvedLocalBinding {
        target: u16,
        slot: u16,
        name: String,
        strict: bool,
        src: u16,
    },
    LoadResolvedLocalBinding {
        dst: u16,
        target: u16,
        slot: u16,
        name: String,
    },
    LoadBinding {
        dst: u16,
        slot: u16,
        name: String,
        dynamic: bool,
    },
    LoadResolvedBinding {
        dst: u16,
        target: u16,
        name: String,
    },
    LoadLocal {
        dst: u16,
        slot: u16,
    },
    MakeArray {
        dst: u16,
        elements: Vec<u16>,
    },
    TemplateObject {
        dst: u16,
        cooked: u16,
        raw: u16,
        site: u64,
    },
    BuildArray {
        dst: u16,
        elements: Vec<ArrayElement>,
    },
    MakeObject {
        dst: u16,
        properties: Vec<(crate::value::PropertyName, u16)>,
    },
    /// Per-script global view: an isolated property vector retaining the
    /// realm-global semantic owner for copy-on-write writes.
    MakeGlobalObjectView {
        dst: u16,
        properties: Vec<(String, u16)>,
    },
    MakeBuiltin {
        dst: u16,
        builtin: Builtin,
    },
    GetProperty {
        dst: u16,
        object: u16,
        key: String,
    },
    OptionalGet {
        dst: u16,
        object: u16,
        key: String,
    },
    OptionalGetPrivate {
        dst: u16,
        object: u16,
        name: crate::facts::PrivateNameId,
    },
    OptionalGetDynamic {
        dst: u16,
        object: u16,
        key: u16,
    },
    GetPrivate {
        dst: u16,
        object: u16,
        name: crate::facts::PrivateNameId,
    },
    HasPrivate {
        dst: u16,
        object: u16,
        name: crate::facts::PrivateNameId,
    },
    GetSuperProperty {
        dst: u16,
        key: String,
    },
    GetSuperPropertyDynamic {
        dst: u16,
        key: u16,
        base: Option<u16>,
    },
    SetSuperProperty {
        key: String,
        src: u16,
    },
    SetSuperPropertyDynamic {
        key: u16,
        src: u16,
        base: Option<u16>,
    },
    CaptureSuperBase {
        dst: u16,
    },
    ResolveGlobal {
        dst: u16,
        object: u16,
        key: String,
    },
    ResolveName {
        dst: u16,
        key: String,
    },
    ResolveStrictName {
        dst: u16,
        key: String,
    },
    ResolveNameOrUndefined {
        dst: u16,
        name: String,
    },
    SetName {
        key: String,
        src: u16,
        strict: bool,
    },
    SetResolvedBinding {
        target: u16,
        name: String,
        src: u16,
        strict: bool,
    },
    CheckStrictName {
        key: String,
    },
    SetFunctionName {
        function: u16,
        name: String,
    },
    SetFunctionNameDynamic {
        function: u16,
        key: u16,
        prefix: Option<String>,
    },
    GetPropertyDynamic {
        dst: u16,
        object: u16,
        key: u16,
    },
    HasPropertyDynamic {
        dst: u16,
        object: u16,
        key: u16,
    },
    ToPropertyKey {
        dst: u16,
        src: u16,
    },
    RequireObjectCoercible {
        src: u16,
    },
    GetIterator {
        dst: u16,
        iterable: u16,
    },
    IteratorStep {
        dst: u16,
        iterator: u16,
    },
    IteratorRest {
        dst: u16,
        iterator: u16,
    },
    ValidateClassHeritage {
        src: u16,
    },
    GetClassPrototype {
        dst: u16,
        constructor_dst: u16,
        heritage: u16,
    },
    CheckSuperThis,
    AppendInstanceField(AppendInstanceFieldOp),
    SetProperty {
        object: u16,
        key: String,
        src: u16,
        strict: bool,
    },
    SetPrototype {
        object: u16,
        prototype: u16,
    },
    SetPrivate {
        object: u16,
        name: crate::facts::PrivateNameId,
        src: u16,
    },
    DefinePrivate {
        object: u16,
        name: crate::facts::PrivateNameId,
        src: u16,
    },
    SetPropertyDynamic {
        object: u16,
        key: u16,
        src: u16,
        strict: bool,
    },
    DefineProperty {
        object: u16,
        key: u16,
        value: u16,
        kind: PropertyDefinitionKind,
        enumerable: bool,
    },
    CopyDataProperties {
        target: u16,
        source: u16,
        excluded: Vec<u16>,
    },
    DeleteProperty {
        dst: u16,
        object: u16,
        key: u16,
        strict: bool,
    },
    MakeFunction {
        dst: u16,
        body: crate::machine::FunctionCode,
        params: u16,
        length: u16,
        captures: u16,
        strictness: FunctionStrictness,
        is_async: bool,
        mapped_arguments: bool,
    },
    MakeFunctionWithKind {
        dst: u16,
        body: crate::machine::FunctionCode,
        params: u16,
        length: u16,
        captures: u16,
        kind: FunctionKind,
        strictness: FunctionStrictness,
        is_async: bool,
        mapped_arguments: bool,
        source: Option<String>,
    },
    Call {
        dst: u16,
        callee: u16,
        /// `this` register for the call. `None` means undefined (legacy
        /// bare-call behavior); private method calls set this to the
        /// enclosing frame's `this` slot so the method receives the
        /// correct receiver.
        receiver: Option<u16>,
        args: Vec<u16>,
        spreads: Vec<bool>,
    },
    OptionalCall {
        dst: u16,
        callee: u16,
        receiver: Option<u16>,
        guard_receiver: bool,
        args: Vec<u16>,
        spreads: Vec<bool>,
    },
    CallSuperConstructor {
        dst: u16,
        args: Vec<u16>,
        spreads: Vec<bool>,
    },
    TailCall {
        callee: u16,
        args: Vec<u16>,
        spreads: Vec<bool>,
    },
    Eval {
        dst: u16,
        callee: u16,
        source: u16,
        strict: bool,
        global: bool,
        direct: bool,
        tail: bool,
        bindings: Vec<(String, u16)>,
        reusable_var_names: Vec<String>,
        forbidden_var_names: Vec<String>,
    },
    ParameterEnd,
    CallMethod {
        dst: u16,
        object: u16,
        key: String,
        callee: Option<u16>,
        args: Vec<u16>,
        spreads: Vec<bool>,
    },
    CallSuperMethod {
        dst: u16,
        key: String,
        args: Vec<u16>,
    },
    /// Suspend async execution until the source completion settles.
    Await {
        dst: u16,
        src: u16,
    },
    Yield {
        src: u16,
    },
    YieldStar {
        dst: u16,
        source: u16,
        iterator: u16,
    },
    Construct {
        dst: u16,
        callee: u16,
        args: Vec<u16>,
        spreads: Vec<bool>,
    },
    Branch {
        condition: u16,
        then_ops: crate::machine::FunctionCode,
        else_ops: crate::machine::FunctionCode,
    },
    Label {
        name: String,
        body: crate::machine::FunctionCode,
    },
    With {
        object: u16,
        body: crate::machine::FunctionCode,
    },
    PrivateScope {
        names: Vec<crate::facts::PrivateNameId>,
        labels: Vec<String>,
        class_name: Option<String>,
        body: crate::machine::FunctionCode,
    },
    StaticBlock {
        constructor: u16,
        captures: u16,
        body: crate::machine::FunctionCode,
    },
    Try {
        body: crate::machine::FunctionCode,
        handler: Option<crate::machine::FunctionCode>,
        finalizer: Option<crate::machine::FunctionCode>,
        catch_slot: Option<u16>,
        dst: u16,
        finally_dst: Option<u16>,
    },
    /// Execute a binding pattern and close its iterator with the body's exact completion.
    IteratorBinding {
        iterator: u16,
        body: crate::machine::FunctionCode,
        close_normal: bool,
    },
    Loop {
        label: Option<String>,
        init: crate::machine::FunctionCode,
        test: crate::machine::FunctionCode,
        body: crate::machine::FunctionCode,
        update: crate::machine::FunctionCode,
        post_test: bool,
        dst: u16,
        per_iteration: Vec<u16>,
    },
    ForIn {
        label: Option<String>,
        object: u16,
        slot: u16,
        body: crate::machine::FunctionCode,
        per_iteration: bool,
        iteration_slots: Vec<u16>,
        dst: u16,
    },
    ForOf {
        label: Option<String>,
        iterable: u16,
        slot: u16,
        body: crate::machine::FunctionCode,
        per_iteration: bool,
        iteration_slots: Vec<u16>,
        r#await: bool,
        dst: u16,
    },
    Switch {
        discriminant: u16,
        cases: Vec<(
            Option<crate::machine::FunctionCode>,
            crate::machine::FunctionCode,
        )>,
        dst: u16,
    },
    Conditional {
        dst: u16,
        condition: u16,
        consequent: crate::machine::FunctionCode,
        alternate: crate::machine::FunctionCode,
    },
    Unary {
        dst: u16,
        operator: UnaryOp,
        src: u16,
    },
    Binary {
        dst: u16,
        operator: BinaryOp,
        lhs: u16,
        rhs: u16,
    },
    Return {
        src: u16,
    },
    Break {
        label: Option<String>,
        value: Option<u16>,
    },
    Continue {
        label: Option<String>,
        value: Option<u16>,
    },
    Throw {
        src: u16,
    },
    WithDispose {
        body: crate::machine::FunctionCode,
        stack: u16,
        await_using: bool,
    },
    DynamicImport {
        dst: u16,
        specifier: u16,
        options: Option<u16>,
        deferred: bool,
    },
}
