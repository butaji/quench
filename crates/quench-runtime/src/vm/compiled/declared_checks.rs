//! The declared parameter types a tier checks at entry: one set (`DECLARED_CHECKED`) that the u-jit
//! (`lane_region/scalar.rs`) and the b-jit (`base_region/declared.rs`) both read. A TypeScript
//! annotation is a fact only where some tier's entry code tests it, so a member is in this set when
//! at least one tier tests it, and each tier's own mask names the members its entry code can test
//! (`LANE_TESTED`, `BASE_TESTED`). A declared set is checked by a tier only when every one of its
//! members is in that tier's mask; any other set gets no entry fact in that tier and stays `ANY`.
//!
//! The members are language bits (`typed_ir::language_bit`, the order of `ValueType`), plus the two
//! object-family words the b-jit tags (`ARRAY`, `OBJECT_PLAIN`, `staged::value::language`), which sit
//! past `ValueType`'s discriminants and so name no language a `Type` can hold. Number is in the set
//! but neither mask's tag test covers it: the u-jit proves a Number set exactly (`Type::NUMBER`), and
//! the b-jit proves it through the register's representation (`DeclaredFact::Number`).

use crate::typed_ir::language_bit;
use crate::ValueType;

/// A Number (an int32 or a binary64 word): proved exactly by the u-jit and by representation in the b-jit.
pub(crate) const NUMBER: u16 = language_bit(ValueType::Number);
/// `true` and `false`: one tag test in both tiers.
pub(crate) const BOOLEAN: u16 = language_bit(ValueType::Boolean);
/// `null`: one word test in both tiers.
pub(crate) const NULL: u16 = language_bit(ValueType::Null);
/// `undefined`: one word test in both tiers.
pub(crate) const UNDEFINED: u16 = language_bit(ValueType::Undefined);
/// A direct object family word (the object family's bit, which also admits arrays and plain objects): one tag test in both tiers.
pub(crate) const OBJECT: u16 = language_bit(ValueType::Object);
/// A direct function word: one tag test in both tiers.
pub(crate) const FUNCTION: u16 = language_bit(ValueType::Function);
/// A string. The u-jit accepts a direct string by its tag and any other string by the VM's `typeof`
/// on a slow path; the b-jit has no tag test for it yet (a direct string is a family word its
/// `tag_languages` does not set), so a String set gets no b-jit entry check.
pub(crate) const STRING: u16 = language_bit(ValueType::String);
/// A direct array word (`DIRECT_ARRAY_TAG`): one tag test in the b-jit. The u-jit has no `Type` for
/// an array, so an array set gets no u-jit entry check.
pub(crate) const ARRAY: u16 = crate::staged::value::language::ARRAY;
/// A plain direct object word (`DIRECT_OBJECT_TAG`): one tag test in the b-jit. The u-jit has no
/// `Type` for a plain object, so a plain-object set gets no u-jit entry check.
pub(crate) const OBJECT_PLAIN: u16 = crate::staged::value::language::OBJECT_PLAIN;

/// The one set of declared language types a tier checks at entry: the union of every member some
/// tier's entry code tests. Both tiers read this set, through their own masks below.
pub(crate) const DECLARED_CHECKED: u16 = NUMBER | BOOLEAN | NULL | UNDEFINED | OBJECT | FUNCTION | STRING | ARRAY | OBJECT_PLAIN;

/// The members of `DECLARED_CHECKED` the u-jit's entry conformance (`lane_region.rs`, `conform_language`)
/// tests exactly: the immediates, the direct object and function families, and a string. A Number is
/// not in it: the u-jit proves a Number set through its own exact Number path, not a tag test.
pub(crate) const LANE_TESTED: u16 = BOOLEAN | NULL | UNDEFINED | OBJECT | FUNCTION | STRING;

/// The members of `DECLARED_CHECKED` the b-jit's entry test decides with a tag (`Heap::Declared`,
/// `staged::value::tag_languages`): a Number, the immediates, the direct object, array, plain and
/// function families. A String is not in it, for the reason `STRING` gives.
pub(crate) const BASE_TESTED: u16 = NUMBER | BOOLEAN | NULL | UNDEFINED | OBJECT | FUNCTION | ARRAY | OBJECT_PLAIN;
