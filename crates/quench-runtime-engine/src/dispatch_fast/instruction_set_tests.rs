#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

#[test]
fn instruction_membership_crosses_word_and_inline_boundaries() {
    for count in [0, 1, INSTRUCTION_WORD_BITS, INSTRUCTION_WORD_BITS + 1,
        INLINE_INSTRUCTION_WORDS * INSTRUCTION_WORD_BITS,
        INLINE_INSTRUCTION_WORDS * INSTRUCTION_WORD_BITS + 1]
    {
        let mut set = InstructionSet::new(count);
        assert_eq!(set.0.len(), count.div_ceil(INSTRUCTION_WORD_BITS));
        assert_eq!(set.0.spilled(), count > INLINE_INSTRUCTION_WORDS * INSTRUCTION_WORD_BITS);
        for pc in 0..count {
            assert!(!set.contains(pc));
            set.insert(pc);
            assert!(set.contains(pc));
            if pc + 1 < count { assert!(!set.contains(pc + 1)); }
        }
        for pc in 0..count { assert!(set.contains(pc)); }
    }
}

#[test]
fn every_named_own_site_constant_is_pairwise_distinct() {
    // Each named site fact selects its own row; two constants with one value would dispatch one row for two reads.
    let mut sites: Vec<(String, u8)> = [
        ("TRUSTED_OWN_THIS", TRUSTED_OWN_THIS),
        ("TRUSTED_OWN_REGISTER", TRUSTED_OWN_REGISTER),
        ("TRUSTED_RUN_THIS", TRUSTED_RUN_THIS),
        ("TRUSTED_RUN_REGISTER", TRUSTED_RUN_REGISTER),
        ("TRUSTED_LOCAL", TRUSTED_LOCAL),
        ("RETURNING_TRUSTED_THIS", RETURNING_TRUSTED_THIS),
        ("RETURNING_TRUSTED_REGISTER", RETURNING_TRUSTED_REGISTER),
        ("NESTED_TRUSTED", NESTED_TRUSTED),
        ("CLOSURE_CELL_SITE", CLOSURE_CELL_SITE),
        ("MATH_CALL_SITE", MATH_CALL_SITE),
        ("MATH_CALL2_SITE", MATH_CALL2_SITE),
        ("MATH_CALL2_CONST_SITE", MATH_CALL2_CONST_SITE),
        ("SINK_SITE", SINK_SITE),
        ("PRIVATE_FIELD_THIS", PRIVATE_FIELD_THIS),
        ("PRIVATE_FIELD_REGISTER", PRIVATE_FIELD_REGISTER),
        ("GENERAL_THIS", GENERAL_THIS),
        ("GENERAL_REGISTER", GENERAL_REGISTER),
        ("TRACKED_SITE", TRACKED_SITE),
        ("GENERAL_SITE", GENERAL_SITE),
        ("ARRAY_LENGTH_SITE", ARRAY_LENGTH_SITE),
        ("STRING_LENGTH_SITE", STRING_LENGTH_SITE),
        ("TYPED_LENGTH_SITE", TYPED_LENGTH_SITE),
    ]
    .into_iter()
    .map(|(name, site)| (name.to_string(), site))
    .collect();
    sites.extend(TYPED_KIND_SITES.iter().enumerate().map(|(kind, &site)| (format!("TYPED_KIND_SITES[{kind}]"), site)));
    for (index, (name, site)) in sites.iter().enumerate() {
        for (other, other_site) in &sites[index + 1..] {
            assert_ne!(site, other_site, "{name} and {other} are both site {site}");
        }
    }
}
