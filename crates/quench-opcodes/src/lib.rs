//! The opcode table (`table.rs`) and its rows as data. The runtime expands the table into `Op`
//! (`with_opcode_table!`); build scripts (a build-dependency) and the lint read [`ROWS`], which
//! the same expansion writes, so no consumer reads the table as text.

mod table;

/// One opcode of the table: its name and the idents each column of its row declares.
#[derive(Clone, Copy, Debug)]
pub struct Row {
    /// The opcode's name, the `Op` variant.
    pub name: &'static str,
    /// The effect expression, its tokens space-separated (`stringify!`).
    pub effect: &'static str,
    /// The `ImmediateLayout` (`layout`).
    pub layout: &'static str,
    /// The `ImmediateRole` (`meaning`).
    pub meaning: Option<&'static str>,
    /// The `ControlFlowLayout` (`flow`).
    pub flow: Option<&'static str>,
    /// The `JsDenotation` a lowering consumes (`js`).
    pub js: Option<&'static str>,
    /// The operation family, a `Class` (`class`).
    pub class: Option<&'static str>,
    /// The decoder of the staged operation, a `Decoder` (`staged`).
    pub staged: Option<&'static str>,
    /// The generic step a lane or a body makes in place, a `GenericStep` (`step`).
    pub step: Option<&'static str>,
    /// The owned step, an `OwnedStep` (`owned`).
    pub owned: Option<&'static str>,
    /// The value step, a `ValueStep` (`value`).
    pub value: Option<&'static str>,
    /// The `Traits` the row declares (`traits`).
    pub traits: &'static [&'static str],
    /// The `ResultLayout` (`@ result`).
    pub result: &'static str,
    /// The `FieldLayout` of fields a, b and c (`@ fields(a, b, c)`).
    pub fields: [&'static str; 3],
}

/// The table's rows as data, in table (`Op`) order.
macro_rules! table_rows {
    ($($name:ident => $effect:expr; layout $layout:ident $(; meaning $meaning:ident)? $(; flow $flow:ident)? $(; js $js:ident)? $(; class $class:ident)? $(; staged $staged:ident)? $(; step $step:ident)? $(; owned $owned:ident)? $(; value $value:ident)? $(; traits $($trait:ident)+)?, @ $result:ident, @ fields($a:ident, $b:ident, $c:ident)),+ $(,)?) => {
        pub const ROWS: &[Row] = &[$(Row {
            name: stringify!($name),
            effect: stringify!($effect),
            layout: stringify!($layout),
            meaning: table_rows!(@column $($meaning)?),
            flow: table_rows!(@column $($flow)?),
            js: table_rows!(@column $($js)?),
            class: table_rows!(@column $($class)?),
            staged: table_rows!(@column $($staged)?),
            step: table_rows!(@column $($step)?),
            owned: table_rows!(@column $($owned)?),
            value: table_rows!(@column $($value)?),
            traits: &[$($(stringify!($trait)),+)?],
            result: stringify!($result),
            fields: [stringify!($a), stringify!($b), stringify!($c)],
        }),+];
    };
    (@column $value:ident) => { Some(stringify!($value)) };
    (@column) => { None };
}

with_opcode_table!(table_rows);

/// Whether `name` is an opcode of the table.
pub fn is_opcode(name: &str) -> bool {
    ROWS.iter().any(|row| row.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_carry_their_columns() {
        let nop = ROWS[0];
        assert_eq!((nop.name, nop.effect, nop.layout, nop.meaning), ("Nop", "Effect :: PURE", "Scalar", Some("Unused")));
        assert_eq!((nop.class, nop.staged, nop.step, nop.result), (Some("Nop"), Some("Flow"), None, "Register"));
        assert_eq!(nop.traits, ["LEAF_VALUE", "SCALAR_REGION", "FRAME_ONLY", "NO_OP", "NO_STEP"]);
        assert!(is_opcode("LoadConst") && !is_opcode("NoSuchOpcode"));
    }
}
