use std::collections::BTreeSet;

pub const COLLATIONS: &[&str] = &[
    "big5han", "compat", "dict", "emoji", "eor", "gb2312", "phonebk", "phonetic", "pinyin",
    "searchjl", "stroke", "trad", "unihan", "zhuyin",
];

pub fn collation_supported(locale: &str, collation: &str) -> bool {
    let language = locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match collation {
        "compat" => language == "ar",
        "dict" => language == "si",
        "emoji" | "eor" => true,
        "phonebk" => language == "de",
        "phonetic" => language == "ln",
        "searchjl" => language == "ko",
        "pinyin" | "big5han" | "gb2312" | "stroke" | "trad" | "unihan" | "zhuyin" => {
            language == "zh"
        }
        _ => false,
    }
}

pub const CURRENCIES: &[&str] = &[
    "ADP", "AED", "AFA", "AFN", "ALL", "AMD", "ANG", "AOA", "AOK", "AON", "AOR", "ARA", "ARP",
    "ARS", "ATS", "AUD", "AWG", "AZM", "AZN", "BAM", "BBD", "BDT", "BEF", "BGL", "BGN", "BHD",
    "BIF", "BMD", "BND", "BOB", "BOP", "BOV", "BRB", "BRC", "BRE", "BRL", "BRN", "BRR", "BSD",
    "BTN", "BUK", "BWP", "BYB", "BYN", "BYR", "BZD", "CAD", "CDF", "CHF", "CLF", "CLP", "CNH",
    "CNY", "COP", "CRC", "CSD", "CSK", "CUC", "CUP", "CVE", "CYP", "CZK", "DDM", "DEM", "DJF",
    "DKK", "DOP", "DZD", "ECS", "ECV", "EEK", "EGP", "ERN", "ESA", "ESB", "ESP", "ETB", "EUR",
    "FIM", "FJD", "FKP", "FRF", "GBP", "GEL", "GHC", "GHS", "GIP", "GMD", "GNF", "GNS", "GQE",
    "GRD", "GTQ", "GWE", "GWP", "GYD", "HKD", "HNL", "HRD", "HRK", "HTG", "HUF", "IDR", "IEP",
    "ILP", "ILR", "ILS", "INR", "IQD", "IRR", "ISK", "ITL", "JMD", "JOD", "JPY", "KES", "KGS",
    "KHR", "KMF", "KPW", "KRW", "KWD", "KYD", "KZT", "LAK", "LBP", "LKR", "LRD", "LSL", "LTL",
    "LTT", "LUC", "LUF", "LUL", "LVL", "LVR", "LWD", "LYD", "MAD", "MAF", "MDL", "MGA", "MGF",
    "MKD", "MKN", "MLF", "MMK", "MNT", "MOP", "MRO", "MRU", "MTL", "MTP", "MUR", "MVR", "MWK",
    "MXN", "MXP", "MXV", "MYR", "MZE", "MZM", "MZN", "NAD", "NGN", "NIO", "NLG", "NOK", "NPR",
    "NZD", "OMR", "PAB", "PEI", "PEN", "PES", "PGK", "PHP", "PKR", "PLN", "PLZ", "PTE", "PYG",
    "QAR", "RHD", "ROL", "RON", "RSD", "RUB", "RUR", "RWF", "SAR", "SBD", "SCR", "SDD", "SDG",
    "SDP", "SEK", "SGD", "SHP", "SIT", "SKK", "SLL", "SOS", "SRD", "SRG", "SSP", "STD", "STN",
    "SUR", "SVC", "SYP", "SZL", "THB", "TJR", "TJS", "TMM", "TMT", "TND", "TOP", "TPE", "TRL",
    "TRY", "TTD", "TWD", "TZS", "UAH", "UAK", "UGS", "UGX", "USD", "USN", "USS", "UYI", "UYP",
    "UYU", "UYW", "UZS", "VEB", "VED", "VEF", "VES", "VND", "VNN", "VUV", "WST", "XAF", "XAG",
    "XAU", "XBA", "XBB", "XBC", "XBD", "XCD", "XDR", "XEU", "XFO", "XFU", "XOF", "XPD", "XPF",
    "XPT", "XRE", "XSU", "XTS", "XUA", "XXX", "YDD", "YER", "YUD", "YUM", "YUN", "ZAL", "ZAR",
    "ZMK", "ZMW", "ZRN", "ZRZ", "ZWD", "ZWL", "ZWR",
];

pub const UNITS: &[&str] = &[
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

pub fn currency_fraction_digits(currency: &str) -> usize {
    match currency {
        "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG"
        | "RWF" | "UGX" | "UYI" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
        "CLF" => 4,
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
        _ => 2,
    }
}

pub fn supported_time_zones() -> Vec<String> {
    let names = chrono_tz::TZ_VARIANTS
        .iter()
        .map(|timezone| quench_temporal::timezone_primary_name(timezone.name()).to_owned())
        .collect::<BTreeSet<_>>();
    names.into_iter().collect()
}

pub fn canonical_time_zone_name(value: &str) -> Option<String> {
    if value.eq_ignore_ascii_case("utc") {
        return Some("UTC".into());
    }
    chrono_tz::TZ_VARIANTS
        .iter()
        .find(|timezone| timezone.name().eq_ignore_ascii_case(value))
        .map(|timezone| quench_temporal::timezone_primary_name(timezone.name()).to_owned())
}
