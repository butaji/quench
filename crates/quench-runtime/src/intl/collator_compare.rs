pub(crate) fn compare(
    left: &str,
    right: &str,
    locale: &str,
    ignore_punctuation: bool,
    sensitivity: &str,
) -> f64 {
    compare_with_options(
        left,
        right,
        locale,
        &CompareSpec {
            ignore_punctuation,
            sensitivity,
            usage: "sort",
            numeric: false,
            case_first: "false",
        },
    )
}

pub(crate) struct CompareSpec<'a> {
    pub(crate) ignore_punctuation: bool,
    pub(crate) sensitivity: &'a str,
    pub(crate) usage: &'a str,
    pub(crate) numeric: bool,
    pub(crate) case_first: &'a str,
}

pub(crate) fn compare_with_options(
    left: &str,
    right: &str,
    locale: &str,
    options: &CompareSpec<'_>,
) -> f64 {
    quench_intl::compare_collator(
        left,
        right,
        locale,
        &quench_intl::CollatorOptions {
            ignore_punctuation: options.ignore_punctuation,
            sensitivity: options.sensitivity,
            usage: options.usage,
            numeric: options.numeric,
            case_first: options.case_first,
        },
    )
}
