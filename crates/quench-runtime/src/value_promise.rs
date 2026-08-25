#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PromiseContinuation {
    AsyncGenerator {
        generator: Rc<GeneratorData>,
        result: Rc<PromiseData>,
        async_function: bool,
    },
    AsyncGeneratorYield {
        generator: Rc<GeneratorData>,
        result: Rc<PromiseData>,
    },
    Aggregate {
        aggregate: Rc<PromiseAggregate>,
        index: usize,
    },
    Thenable {
        target: Rc<PromiseData>,
        thenable: Value,
        then: Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PromiseAggregateCallback {
    Resolve {
        aggregate: Rc<PromiseAggregate>,
        index: usize,
        called: Rc<Cell<bool>>,
    },
    Reject {
        aggregate: Rc<PromiseAggregate>,
        index: usize,
        called: Rc<Cell<bool>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PromiseCapabilityExecutor {
    pub resolve: RefCell<Option<Value>>,
    pub reject: RefCell<Option<Value>>,
    pub called: Cell<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromiseAggregateKind {
    All,
    AllSettled,
    Any,
    Race,
}

#[derive(Debug, PartialEq)]
pub(crate) struct PromiseAggregate {
    pub(crate) kind: PromiseAggregateKind,
    pub(crate) result: Rc<PromiseData>,
    pub(crate) capability: Rc<PromiseData>,
    pub(crate) remaining: RefCell<usize>,
    pub(crate) values: RefCell<Vec<Value>>,
    pub(crate) keys: Option<Vec<String>>,
    pub(crate) settled: RefCell<bool>,
}
