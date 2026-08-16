use crate::value::Value;

pub(crate) fn supported_calendars() -> Vec<Value> {
    strings(&[
        "buddhist",
        "chinese",
        "coptic",
        "dangi",
        "ethioaa",
        "ethiopic",
        "gregory",
        "hebrew",
        "indian",
        "islamic-civil",
        "islamic-tbla",
        "islamic-umalqura",
        "iso8601",
        "japanese",
        "persian",
        "roc",
    ])
}

pub(crate) fn supported_collations() -> Vec<Value> {
    strings(&["default"])
}

const CURRENCIES: &[&str] = &[
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

pub(crate) fn supported_currencies() -> Vec<Value> {
    strings(CURRENCIES)
}

pub(crate) fn supported_numbering_systems() -> Vec<Value> {
    strings(NUMBERING_SYSTEMS)
}

pub(crate) const NUMBERING_SYSTEMS: &[&str] = &[
    "adlm", "ahom", "arab", "arabext", "bali", "beng", "bhks", "brah", "cakm", "cham", "deva",
    "diak", "fullwide", "gara", "gong", "gonm", "gujr", "gukh", "guru", "hanidec", "hmng", "hmnp",
    "java", "kali", "kawi", "khmr", "knda", "krai", "lana", "lanatham", "laoo", "latn", "lepc",
    "limb", "mathbold", "mathdbl", "mathmono", "mathsanb", "mathsans", "mlym", "modi", "mong",
    "mroo", "mtei", "mymr", "mymrepka", "mymrpao", "mymrshan", "mymrtlng", "nagm", "newa", "nkoo",
    "olck", "onao", "orya", "osma", "outlined", "rohg", "saur", "segment", "shrd", "sind", "sinh",
    "sora", "sund", "sunu", "takr", "talu", "tamldec", "telu", "thai", "tibt", "tirh", "tnsa",
    "tols", "vaii", "wara", "wcho",
];

pub(crate) fn supported_time_zones() -> Vec<Value> {
    strings(&[
        "Etc/GMT+1",
        "Etc/GMT+10",
        "Etc/GMT+11",
        "Etc/GMT+12",
        "Etc/GMT+2",
        "Etc/GMT+3",
        "Etc/GMT+4",
        "Etc/GMT+5",
        "Etc/GMT+6",
        "Etc/GMT+7",
        "Etc/GMT+8",
        "Etc/GMT+9",
        "Etc/GMT-1",
        "Etc/GMT-10",
        "Etc/GMT-11",
        "Etc/GMT-12",
        "Etc/GMT-13",
        "Etc/GMT-14",
        "Etc/GMT-2",
        "Etc/GMT-3",
        "Etc/GMT-4",
        "Etc/GMT-5",
        "Etc/GMT-6",
        "Etc/GMT-7",
        "Etc/GMT-8",
        "Etc/GMT-9",
        "UTC",
    ])
}

pub(crate) const UNITS: &[&str] = &[
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

pub(crate) fn supported_units() -> Vec<Value> {
    strings(UNITS)
}

fn strings(values: &[&str]) -> Vec<Value> {
    values
        .iter()
        .map(|value| Value::String(value.to_string()))
        .collect()
}
