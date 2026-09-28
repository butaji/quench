pub(super) fn locale_case(value: &str, locale: &str, upper: bool) -> String {
    quench_intl::locale_case(value, locale, upper)
}
