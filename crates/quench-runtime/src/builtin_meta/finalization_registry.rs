//! FinalizationRegistry intrinsic metadata.

use crate::ops::Builtin;

pub fn property(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "constructor" => Some(FinalizationRegistry),
        "register" => Some(FinalizationRegistryRegister),
        "unregister" => Some(FinalizationRegistryUnregister),
        _ => None,
    }
}

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    use Builtin::*;
    Some(match builtin {
        FinalizationRegistryRegister => "FinalizationRegistry.prototype.register",
        FinalizationRegistryUnregister => "FinalizationRegistry.prototype.unregister",
        _ => return None,
    })
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    use Builtin::*;
    Some(match builtin {
        FinalizationRegistryRegister => 2.0,
        FinalizationRegistryUnregister => 1.0,
        _ => return None,
    })
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    use Builtin::*;
    Some(match builtin {
        FinalizationRegistryRegister => "register",
        FinalizationRegistryUnregister => "unregister",
        _ => return None,
    })
}
