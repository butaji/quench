//! RegExp method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::RegExpCompile => Some("RegExp.prototype.compile"),
        Builtin::RegExpPrototypeToString => Some("RegExp.prototype.toString"),
        Builtin::RegExpEscape => Some("escape"),
        Builtin::RegExpTest => Some("RegExp.prototype.test"),
        Builtin::RegExpExec => Some("RegExp.prototype.exec"),
        Builtin::RegExpSymbolMatch => Some("RegExp.prototype[Symbol.match]"),
        Builtin::RegExpSymbolSearch => Some("RegExp.prototype[Symbol.search]"),
        Builtin::RegExpSymbolReplace => Some("RegExp.prototype[Symbol.replace]"),
        Builtin::RegExpSymbolSplit => Some("RegExp.prototype[Symbol.split]"),
        Builtin::RegExpSymbolMatchAll => Some("RegExp.prototype[Symbol.matchAll]"),
        Builtin::RegExpStringIteratorNext => Some("RegExpStringIterator.prototype.next"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::RegExpCompile => Some(2.0),
        Builtin::RegExpPrototypeToString => Some(0.0),
        Builtin::RegExpEscape | Builtin::RegExpTest | Builtin::RegExpExec => Some(1.0),
        Builtin::RegExpSymbolSearch | Builtin::RegExpSymbolMatchAll => Some(1.0),
        Builtin::RegExpSymbolReplace | Builtin::RegExpSymbolSplit => Some(2.0),
        Builtin::RegExpSymbolMatch => Some(1.0),
        Builtin::RegExpStringIteratorNext => Some(0.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::RegExpCompile => Some("compile"),
        Builtin::RegExpPrototypeToString => Some("toString"),
        Builtin::RegExpEscape => Some("escape"),
        Builtin::RegExpTest => Some("test"),
        Builtin::RegExpExec => Some("exec"),
        Builtin::RegExpSymbolMatch => Some("[Symbol.match]"),
        Builtin::RegExpSymbolSearch => Some("[Symbol.search]"),
        Builtin::RegExpSymbolReplace => Some("[Symbol.replace]"),
        Builtin::RegExpSymbolSplit => Some("[Symbol.split]"),
        Builtin::RegExpSymbolMatchAll => Some("[Symbol.matchAll]"),
        Builtin::RegExpStringIteratorNext => Some("next"),
        _ => None,
    }
}
