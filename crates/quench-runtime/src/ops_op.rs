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
    Const {
        dst: u16,
        value: Constant,
    },
    StoreLocal {
        slot: u16,
        src: u16,
    },
    LoadCurrentGlobal {
        dst: u16,
    },
    MarkUninitialized {
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
    InitializeResolvedBinding {
        target: u16,
        slot: u16,
        name: String,
        src: u16,
    },
    LoadBinding {
        dst: u16,
        slot: u16,
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
    BuildArray {
        dst: u16,
        elements: Vec<ArrayElement>,
    },
    MakeObject {
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
    GetSuperProperty {
        dst: u16,
        key: String,
    },
    GetSuperPropertyDynamic {
        dst: u16,
        key: u16,
    },
    SetSuperProperty {
        key: String,
        src: u16,
    },
    SetSuperPropertyDynamic {
        key: u16,
        src: u16,
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
    },
    Call {
        dst: u16,
        callee: u16,
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
        source: u16,
        strict: bool,
        global: bool,
        direct: bool,
        bindings: Vec<(String, u16)>,
        forbidden_var_names: Vec<String>,
    },
    ParameterEnd,
    CallMethod {
        dst: u16,
        object: u16,
        key: String,
        callee: Option<u16>,
        args: Vec<u16>,
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
        body: crate::machine::FunctionCode,
    },
    Try {
        body: Vec<Op>,
        handler: Option<Vec<Op>>,
        finalizer: Option<Vec<Op>>,
        catch_slot: Option<u16>,
    },
    /// Execute a binding pattern and close its iterator with the body's exact completion.
    IteratorBinding {
        iterator: u16,
        body: Vec<Op>,
        close_normal: bool,
    },
    Loop {
        label: Option<String>,
        init: Vec<Op>,
        test: Vec<Op>,
        body: Vec<Op>,
        update: Vec<Op>,
        post_test: bool,
    },
    ForIn {
        label: Option<String>,
        object: u16,
        slot: u16,
        body: Vec<Op>,
        per_iteration: bool,
    },
    ForOf {
        label: Option<String>,
        iterable: u16,
        slot: u16,
        body: Vec<Op>,
        per_iteration: bool,
    },
    Switch {
        discriminant: u16,
        cases: Vec<(Option<Constant>, Vec<Op>)>,
    },
    Conditional {
        dst: u16,
        condition: u16,
        consequent: Vec<Op>,
        alternate: Vec<Op>,
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
    },
    Continue {
        label: Option<String>,
    },
    Throw {
        src: u16,
    },
}
