//! The Intl data the VM core keeps (the Intl and Temporal behaviour is the
//! `quench-runtime-intl` layer crate): the realm's default collation, which the lane's
//! in-place `localeCompare` reads, and each Intl constructor's per-realm prototypes.
use super::*;

/// A collator's resolved collation, which each comparison reuses (`quench_intl::ResolvedCollator`).
pub struct Collation(pub quench_intl::ResolvedCollator);

impl Collation {
    pub fn compare(&self, left: &str, right: &str) -> f64 {
        self.0.compare(left, right)
    }
}

impl Vm {
    pub fn intl_constructor_prototypes(
        &self,
        constructor: Native,
    ) -> Option<&FxHashMap<Value, Value>> {
        Some(match constructor {
            Native::IntlCollator => &self.realm.intrinsics.intl_collator_prototypes,
            Native::IntlNumberFormat => &self.realm.intrinsics.intl_number_format_prototypes,
            Native::IntlDateTimeFormat => &self.realm.intrinsics.intl_datetime_format_prototypes,
            Native::IntlListFormat => &self.realm.intrinsics.intl_list_format_prototypes,
            Native::IntlSegmenter => &self.realm.intrinsics.intl_segmenter_prototypes,
            Native::IntlDisplayNames => &self.realm.intrinsics.intl_display_names_prototypes,
            Native::IntlPluralRules => &self.realm.intrinsics.intl_plural_rules_prototypes,
            Native::IntlRelativeTimeFormat => {
                &self.realm.intrinsics.intl_relative_time_format_prototypes
            }
            Native::IntlDurationFormat => &self.realm.intrinsics.intl_duration_format_prototypes,
            Native::IntlLocale => &self.realm.intrinsics.intl_locale_prototypes,
            _ => return None,
        })
    }
}
