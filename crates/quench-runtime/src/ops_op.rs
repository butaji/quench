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
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstanceFieldInitializerOp {
    pub body: Vec<Op>,
    pub captures: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppendInstanceFieldOp {
    pub constructor: u16,
    pub key: InstanceFieldKeyOp,
    pub initializer: Option<InstanceFieldInitializerOp>,
    pub is_static: bool,
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
    MarkUninitialized {
        slot: u16,
    },
    DeclareEvalBinding {
        name: String,
        slot: u16,
    },
    DeleteEvalBinding {
        dst: u16,
        name: String,
        slot: u16,
    },
    CheckGlobalFunction {
        name: String,
    },
    CheckGlobalVar {
        name: String,
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
    GetSuperProperty {
        dst: u16,
        key: String,
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
    },
    SetPropertyDynamic {
        object: u16,
        key: u16,
        src: u16,
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
        body: Vec<Op>,
        params: u16,
        length: u16,
        captures: u16,
        strictness: FunctionStrictness,
        is_async: bool,
        mapped_arguments: bool,
    },
    MakeFunctionWithKind {
        dst: u16,
        body: Vec<Op>,
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
        bindings: Vec<(String, u16)>,
        forbidden_var_names: Vec<String>,
    },
    ParameterEnd,
    CallMethod {
        dst: u16,
        object: u16,
        key: String,
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
    },
    Branch {
        condition: u16,
        then_ops: Vec<Op>,
        else_ops: Vec<Op>,
    },
    Label {
        name: String,
        body: Vec<Op>,
    },
    With {
        object: u16,
        body: Vec<Op>,
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
