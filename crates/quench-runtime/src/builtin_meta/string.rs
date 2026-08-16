//! String builtin metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::StringAnchor => Some("String.prototype.anchor"),
        Builtin::StringBig => Some("String.prototype.big"),
        Builtin::StringIterator => Some("[Symbol.iterator]"),
        Builtin::StringIteratorNext => Some("StringIterator.prototype.next"),
        Builtin::StringFromCharCode => Some("String.fromCharCode"),
        Builtin::StringFromCodePoint => Some("String.fromCodePoint"),
        Builtin::StringRaw => Some("String.raw"),
        Builtin::StringValueOf => Some("String.prototype.valueOf"),
        Builtin::StringIncludes => Some("String.prototype.includes"),
        Builtin::StringIsWellFormed => Some("String.prototype.isWellFormed"),
        Builtin::StringToWellFormed => Some("String.prototype.toWellFormed"),
        Builtin::StringStartsWith => Some("String.prototype.startsWith"),
        Builtin::StringEndsWith => Some("String.prototype.endsWith"),
        Builtin::StringAt => Some("String.prototype.at"),
        Builtin::StringRepeat => Some("String.prototype.repeat"),
        Builtin::StringTrim => Some("String.prototype.trim"),
        Builtin::StringToLowerCase => Some("String.prototype.toLowerCase"),
        Builtin::StringToUpperCase => Some("String.prototype.toUpperCase"),
        Builtin::StringCharAt => Some("String.prototype.charAt"),
        Builtin::StringCharCodeAt => Some("String.prototype.charCodeAt"),
        Builtin::StringIndexOf => Some("String.prototype.indexOf"),
        Builtin::StringLastIndexOf => Some("String.prototype.lastIndexOf"),
        _ => fn_name_tail(b),
    }
}

const fn fn_name_tail(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::StringSlice => Some("String.prototype.slice"),
        Builtin::StringSubstring => Some("String.prototype.substring"),
        Builtin::StringConcat => Some("String.prototype.concat"),
        Builtin::StringSplit => Some("String.prototype.split"),
        Builtin::StringPadStart => Some("String.prototype.padStart"),
        Builtin::StringPadEnd => Some("String.prototype.padEnd"),
        Builtin::StringTrimStart => Some("String.prototype.trimStart"),
        Builtin::StringTrimEnd => Some("String.prototype.trimEnd"),
        Builtin::StringCodePointAt => Some("String.prototype.codePointAt"),
        Builtin::StringToString => Some("String.prototype.toString"),
        Builtin::StringReplace => Some("String.prototype.replace"),
        Builtin::StringReplaceAll => Some("String.prototype.replaceAll"),
        Builtin::StringSearch => Some("String.prototype.search"),
        Builtin::StringLocaleCompare => Some("String.prototype.localeCompare"),
        Builtin::StringMatch => Some("String.prototype.match"),
        Builtin::StringMatchAll => Some("String.prototype.matchAll"),
        Builtin::StringToLocaleLowerCase => Some("String.prototype.toLocaleLowerCase"),
        Builtin::StringToLocaleUpperCase => Some("String.prototype.toLocaleUpperCase"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::StringAnchor => Some(1.0),
        Builtin::StringBig => Some(0.0),
        Builtin::StringIterator => Some(0.0),
        Builtin::StringIteratorNext => Some(0.0),
        Builtin::StringFromCharCode => Some(1.0),
        Builtin::StringFromCodePoint => Some(1.0),
        Builtin::StringRaw => Some(1.0),
        Builtin::StringValueOf => Some(0.0),
        Builtin::StringRepeat
        | Builtin::StringTrim
        | Builtin::StringToLowerCase
        | Builtin::StringToUpperCase
        | Builtin::StringTrimStart
        | Builtin::StringTrimEnd
        | Builtin::StringToString
        | Builtin::StringIsWellFormed
        | Builtin::StringToWellFormed
        | Builtin::StringToLocaleLowerCase
        | Builtin::StringToLocaleUpperCase => Some(0.0),
        Builtin::StringIncludes
        | Builtin::StringStartsWith
        | Builtin::StringEndsWith
        | Builtin::StringIndexOf
        | Builtin::StringLastIndexOf
        | Builtin::StringCodePointAt => Some(1.0),
        _ => fn_len_tail(b),
    }
}

const fn fn_len_tail(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::StringAt => Some(1.0),
        Builtin::StringCharAt | Builtin::StringCharCodeAt => Some(1.0),
        Builtin::StringSearch
        | Builtin::StringLocaleCompare
        | Builtin::StringMatch
        | Builtin::StringMatchAll => Some(1.0),
        Builtin::StringConcat => Some(1.0),
        Builtin::StringSlice | Builtin::StringSubstring => Some(2.0),
        Builtin::StringSplit => Some(2.0),
        Builtin::StringPadStart | Builtin::StringPadEnd => Some(1.0),
        Builtin::StringReplace | Builtin::StringReplaceAll => Some(2.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::StringAnchor => Some("anchor"),
        Builtin::StringBig => Some("big"),
        Builtin::StringIteratorNext => Some("next"),
        Builtin::StringFromCharCode => Some("fromCharCode"),
        Builtin::StringFromCodePoint => Some("fromCodePoint"),
        Builtin::StringRaw => Some("raw"),
        Builtin::StringValueOf => Some("valueOf"),
        Builtin::StringIncludes => Some("includes"),
        Builtin::StringIsWellFormed => Some("isWellFormed"),
        Builtin::StringToWellFormed => Some("toWellFormed"),
        Builtin::StringStartsWith => Some("startsWith"),
        Builtin::StringEndsWith => Some("endsWith"),
        Builtin::StringAt => Some("at"),
        Builtin::StringRepeat => Some("repeat"),
        Builtin::StringTrim => Some("trim"),
        Builtin::StringToLowerCase => Some("toLowerCase"),
        Builtin::StringToUpperCase => Some("toUpperCase"),
        Builtin::StringCharAt => Some("charAt"),
        Builtin::StringCharCodeAt => Some("charCodeAt"),
        Builtin::StringIndexOf => Some("indexOf"),
        Builtin::StringLastIndexOf => Some("lastIndexOf"),
        Builtin::StringSlice => Some("slice"),
        Builtin::StringSubstring => Some("substring"),
        Builtin::StringConcat => Some("concat"),
        Builtin::StringSplit => Some("split"),
        Builtin::StringPadStart => Some("padStart"),
        Builtin::StringPadEnd => Some("padEnd"),
        Builtin::StringTrimStart => Some("trimStart"),
        Builtin::StringTrimEnd => Some("trimEnd"),
        Builtin::StringCodePointAt => Some("codePointAt"),
        Builtin::StringToString => Some("toString"),
        Builtin::StringReplace => Some("replace"),
        Builtin::StringReplaceAll => Some("replaceAll"),
        Builtin::StringSearch => Some("search"),
        Builtin::StringLocaleCompare => Some("localeCompare"),
        _ => short_name_tail(b),
    }
}

const fn short_name_tail(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::StringMatch => Some("match"),
        Builtin::StringMatchAll => Some("matchAll"),
        Builtin::StringToLocaleLowerCase => Some("toLocaleLowerCase"),
        Builtin::StringToLocaleUpperCase => Some("toLocaleUpperCase"),
        _ => None,
    }
}
