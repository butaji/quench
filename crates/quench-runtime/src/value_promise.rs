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
    pub(crate) resolve: Value,
    pub(crate) reject: Value,
    pub(crate) remaining: RefCell<usize>,
    pub(crate) values: RefCell<Vec<Value>>,
    pub(crate) keys: Option<Vec<String>>,
    pub(crate) settled: RefCell<bool>,
}
