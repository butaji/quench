//! ECMAScript identifier characters. This crate's ICU property data (`ID_Start`, `ID_Continue`)
//! is the one identifier table: group names and the runtime's identifier checks read it here.

use super::binary_property_matches;
use icu_properties::props::{IdContinue, IdStart};

/// ECMAScript's `IdentifierStartChar`: Unicode `ID_Start`, `$` and `_`.
pub fn is_identifier_start(character: char) -> bool {
    matches!(character, '$' | '_') || binary_property_matches::<IdStart>(character)
}

/// ECMAScript's `IdentifierPartChar`: `IdentifierStartChar`, Unicode `ID_Continue`, ZWNJ and ZWJ.
pub fn is_identifier_part(character: char) -> bool {
    is_identifier_start(character)
        || matches!(character, '\u{200C}' | '\u{200D}')
        || binary_property_matches::<IdContinue>(character)
}

/// ECMAScript's `IdentifierName`: a start character then part characters. The empty name is not one.
pub fn is_identifier_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(is_identifier_start) && characters.all(is_identifier_part)
}
