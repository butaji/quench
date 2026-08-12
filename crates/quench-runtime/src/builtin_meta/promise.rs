use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::PromiseResolve => Some("resolve"),
        Builtin::PromiseReject => Some("reject"),
        Builtin::PromiseAll => Some("all"),
        Builtin::PromiseAllSettled => Some("allSettled"),
        Builtin::PromiseAny => Some("any"),
        Builtin::PromiseRace => Some("race"),
        Builtin::PromiseWithResolvers => Some("withResolvers"),
        Builtin::PromiseTry => Some("try"),
        Builtin::PromiseThen => Some("then"),
        Builtin::PromiseCatch => Some("catch"),
        Builtin::PromiseFinally => Some("finally"),
        _ => None,
    }
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::PromiseResolve
        | Builtin::PromiseReject
        | Builtin::PromiseAll
        | Builtin::PromiseAllSettled
        | Builtin::PromiseAny
        | Builtin::PromiseRace
        | Builtin::PromiseFinally
        | Builtin::PromiseCatch => Some(1.0),
        Builtin::PromiseThen => Some(2.0),
        Builtin::PromiseWithResolvers => Some(0.0),
        Builtin::PromiseTry => Some(1.0),
        Builtin::PromiseFinallyOnFulfilled
        | Builtin::PromiseFinallyOnRejected
        | Builtin::PromiseFinallyFulfilled
        | Builtin::PromiseFinallyRejected => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    fn_name(builtin)
}
