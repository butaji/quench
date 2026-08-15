#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PromiseContinuation {
    AsyncGenerator {
        generator: Rc<GeneratorData>,
        result: Rc<PromiseData>,
    },
    AsyncGeneratorYield {
        generator: Rc<GeneratorData>,
        result: Rc<PromiseData>,
    },
    AsyncGeneratorQueue {
        generator: Rc<GeneratorData>,
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
    pub(crate) result: Rc<PromiseData>,
    pub(crate) resolve: Value,
    pub(crate) reject: Value,
    pub(crate) remaining: RefCell<usize>,
    pub(crate) values: RefCell<Vec<Value>>,
    pub(crate) settled: RefCell<bool>,
}
