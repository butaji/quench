fn promise_method(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "prototype" => Some(PromisePrototype),
        "resolve" => Some(PromiseResolve),
        "reject" => Some(PromiseReject),
        "all" => Some(PromiseAll),
        "allSettled" => Some(PromiseAllSettled),
        "any" => Some(PromiseAny),
        "race" => Some(PromiseRace),
        _ => None,
    }
}

fn promise_prototype_method(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "constructor" => Some(Promise),
        "then" => Some(PromiseThen),
        "catch" => Some(PromiseCatch),
        "finally" => Some(PromiseFinally),
        _ => None,
    }
}
