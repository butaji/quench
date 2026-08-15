fn promise_method(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "prototype" => Some(PromisePrototype),
        "resolve" => Some(PromiseResolve),
        "reject" => Some(PromiseReject),
        "all" => Some(PromiseAll),
        "allSettled" => Some(PromiseAllSettled),
        "allSettledKeyed" => Some(PromiseAllSettledKeyed),
        "any" => Some(PromiseAny),
        "race" => Some(PromiseRace),
        "withResolvers" => Some(PromiseWithResolvers),
        "try" => Some(PromiseTry),
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

fn promise_builtin_method(builtin: Builtin, key: &str) -> Option<Builtin> {
    match builtin {
        Builtin::Promise => promise_method(key),
        Builtin::PromisePrototype => promise_prototype_method(key),
        _ => None,
    }
}

include!("props_names.rs");
