//! DisposableStack intrinsic metadata.

use crate::ops::Builtin;

pub fn property(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "constructor" => Some(DisposableStack),
        "use" => Some(DisposableStackUse),
        "adopt" => Some(DisposableStackAdopt),
        "defer" => Some(DisposableStackDefer),
        "move" => Some(DisposableStackMove),
        "dispose" => Some(DisposableStackDispose),
        _ => None,
    }
}

pub fn async_property(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "constructor" => Some(AsyncDisposableStack),
        "use" => Some(AsyncDisposableStackUse),
        "adopt" => Some(AsyncDisposableStackAdopt),
        "defer" => Some(AsyncDisposableStackDefer),
        "move" => Some(AsyncDisposableStackMove),
        "disposeAsync" | "Symbol.asyncDispose" => Some(AsyncDisposableStackDisposeAsync),
        _ => None,
    }
}

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    use Builtin::*;
    Some(match builtin {
        DisposableStackUse => "DisposableStack.prototype.use",
        DisposableStackAdopt => "DisposableStack.prototype.adopt",
        DisposableStackDefer => "DisposableStack.prototype.defer",
        DisposableStackMove => "DisposableStack.prototype.move",
        DisposableStackDispose => "DisposableStack.prototype.dispose",
        DisposableStackDisposed => "get DisposableStack.prototype.disposed",
        AsyncDisposableStackUse => "AsyncDisposableStack.prototype.use",
        AsyncDisposableStackAdopt => "AsyncDisposableStack.prototype.adopt",
        AsyncDisposableStackDefer => "AsyncDisposableStack.prototype.defer",
        AsyncDisposableStackMove => "AsyncDisposableStack.prototype.move",
        AsyncDisposableStackDisposeAsync => "AsyncDisposableStack.prototype.disposeAsync",
        AsyncDisposableStackDisposed => "get AsyncDisposableStack.prototype.disposed",
        _ => return None,
    })
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    use Builtin::*;
    Some(match builtin {
        DisposableStackUse => 1.0,
        DisposableStackAdopt => 2.0,
        DisposableStackDefer => 1.0,
        DisposableStackMove | DisposableStackDispose | DisposableStackDisposed => 0.0,
        AsyncDisposableStackUse => 1.0,
        AsyncDisposableStackAdopt => 2.0,
        AsyncDisposableStackDefer => 1.0,
        AsyncDisposableStackMove
        | AsyncDisposableStackDisposeAsync
        | AsyncDisposableStackDisposed => 0.0,
        _ => return None,
    })
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    use Builtin::*;
    Some(match builtin {
        DisposableStackUse => "use",
        DisposableStackAdopt => "adopt",
        DisposableStackDefer => "defer",
        DisposableStackMove => "move",
        DisposableStackDispose => "dispose",
        DisposableStackDisposed => "get disposed",
        AsyncDisposableStackUse => "use",
        AsyncDisposableStackAdopt => "adopt",
        AsyncDisposableStackDefer => "defer",
        AsyncDisposableStackMove => "move",
        AsyncDisposableStackDisposeAsync => "disposeAsync",
        AsyncDisposableStackDisposed => "get disposed",
        _ => return None,
    })
}
