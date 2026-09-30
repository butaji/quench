//! Runtime-independent calendrical algorithms shared by both Quench engines.

mod duration;
mod duration_total;
mod offset;
mod rounding;

pub use duration::parse_duration;
pub use duration_total::{relative_duration_nanoseconds, total_duration};
pub use offset::{
    offset_minutes, offset_seconds, valid_date_time_offset, valid_string_offset,
    valid_timezone_offset,
};
pub use rounding::round_temporal_nanoseconds;

pub fn timezone_primary_name(text: &str) -> &str {
    match text {
        "Africa/Asmera" => "Africa/Asmara",
        "Europe/Nicosia" => "Asia/Nicosia",
        "America/Atka" => "America/Adak",
        "America/Knox_IN" => "America/Indiana/Knox",
        "Asia/Ashkhabad" => "Asia/Ashgabat",
        "Asia/Calcutta" => "Asia/Kolkata",
        "Asia/Choibalsan" => "Asia/Ulaanbaatar",
        "Asia/Chongqing" | "Asia/Chungking" | "Asia/Harbin" => "Asia/Shanghai",
        "Asia/Dacca" => "Asia/Dhaka",
        "Asia/Istanbul" => "Europe/Istanbul",
        "Asia/Kashgar" => "Asia/Urumqi",
        "Asia/Katmandu" => "Asia/Kathmandu",
        "Asia/Macao" => "Asia/Macau",
        "Asia/Rangoon" => "Asia/Yangon",
        "Asia/Saigon" => "Asia/Ho_Chi_Minh",
        "Asia/Tel_Aviv" => "Asia/Jerusalem",
        "Asia/Thimbu" => "Asia/Thimphu",
        "Asia/Ujung_Pandang" => "Asia/Makassar",
        "Asia/Ulan_Bator" => "Asia/Ulaanbaatar",
        "Africa/Timbuktu" => "Africa/Bamako",
        "Antarctica/South_Pole" => "Antarctica/McMurdo",
        "Australia/ACT" | "Australia/Canberra" | "Australia/NSW" => "Australia/Sydney",
        "Australia/Currie" | "Australia/Tasmania" => "Australia/Hobart",
        "Australia/LHI" => "Australia/Lord_Howe",
        "Australia/North" => "Australia/Darwin",
        "Australia/Queensland" => "Australia/Brisbane",
        "Australia/South" => "Australia/Adelaide",
        "Australia/Victoria" => "Australia/Melbourne",
        "Australia/West" => "Australia/Perth",
        "Australia/Yancowinna" => "Australia/Broken_Hill",
        "Pacific/Enderbury" => "Pacific/Kanton",
        "Pacific/Johnston" => "Pacific/Honolulu",
        "Pacific/Ponape" => "Pacific/Pohnpei",
        "Pacific/Samoa" => "Pacific/Pago_Pago",
        "Pacific/Truk" | "Pacific/Yap" => "Pacific/Chuuk",
        "Europe/Belfast" => "Europe/London",
        "Europe/Kiev" | "Europe/Uzhgorod" | "Europe/Zaporozhye" => "Europe/Kyiv",
        "Europe/Tiraspol" => "Europe/Chisinau",
        "America/Argentina/ComodRivadavia" => "America/Argentina/Catamarca",

        "America/Buenos_Aires" => "America/Argentina/Buenos_Aires",
        "America/Catamarca" => "America/Argentina/Catamarca",
        "America/Coral_Harbour" => "America/Atikokan",
        "America/Cordoba" => "America/Argentina/Cordoba",
        "America/Ensenada" => "America/Tijuana",
        "America/Fort_Wayne" | "America/Indianapolis" => "America/Indiana/Indianapolis",
        "America/Godthab" => "America/Nuuk",
        "America/Jujuy" => "America/Argentina/Jujuy",
        "America/Louisville" => "America/Kentucky/Louisville",
        "America/Mendoza" => "America/Argentina/Mendoza",
        "America/Montreal" | "America/Nipigon" => "America/Toronto",
        "America/Pangnirtung" => "America/Iqaluit",
        "America/Porto_Acre" => "America/Rio_Branco",
        "America/Rainy_River" => "America/Winnipeg",
        "America/Rosario" => "America/Argentina/Cordoba",
        "America/Santa_Isabel" => "America/Tijuana",
        "America/Shiprock" => "America/Denver",
        "America/Thunder_Bay" => "America/Toronto",
        "America/Virgin" => "America/St_Thomas",
        "America/Yellowknife" => "America/Edmonton",
        "US/Alaska" => "America/Anchorage",
        "US/Aleutian" => "America/Adak",
        "US/Arizona" => "America/Phoenix",
        "US/Central" => "America/Chicago",
        "US/East-Indiana" => "America/Indiana/Indianapolis",
        "US/Eastern" => "America/New_York",
        "US/Hawaii" => "Pacific/Honolulu",
        "US/Indiana-Starke" => "America/Indiana/Knox",
        "US/Michigan" => "America/Detroit",
        "US/Mountain" => "America/Denver",
        "US/Pacific" => "America/Los_Angeles",
        "US/Samoa" => "Pacific/Pago_Pago",
        "Atlantic/Faeroe" => "Atlantic/Faroe",
        "Atlantic/Jan_Mayen" => "Arctic/Longyearbyen",
        "Brazil/Acre" => "America/Rio_Branco",
        "Brazil/DeNoronha" => "America/Noronha",
        "Brazil/East" => "America/Sao_Paulo",
        "Brazil/West" => "America/Manaus",
        "CET" => "Europe/Brussels",
        "CST6CDT" => "America/Chicago",
        "Canada/Atlantic" => "America/Halifax",
        "Canada/Central" => "America/Winnipeg",
        "Canada/Eastern" => "America/Toronto",
        "Canada/Mountain" => "America/Edmonton",
        "Canada/Newfoundland" => "America/St_Johns",

        "Canada/Pacific" => "America/Vancouver",
        "Canada/Saskatchewan" => "America/Regina",
        "Canada/Yukon" => "America/Whitehorse",
        "Chile/Continental" => "America/Santiago",
        "Chile/EasterIsland" => "Pacific/Easter",
        "Cuba" => "America/Havana",
        "EET" => "Europe/Athens",
        "EST" => "America/Panama",
        "EST5EDT" => "America/New_York",
        "Egypt" => "Africa/Cairo",
        "Eire" => "Europe/Dublin",
        "Etc/GMT+0" | "Etc/GMT-0" | "Etc/GMT0" | "Etc/Greenwich" | "Etc/UCT" | "Etc/UTC"
        | "Etc/Universal" | "Etc/Zulu" | "GMT+0" | "GMT-0" | "GMT0" | "Greenwich" | "UCT"
        | "Universal" | "Zulu" | "Etc/GMT" | "GMT" => "UTC",
        "GB" | "GB-Eire" => "Europe/London",
        "HST" => "Pacific/Honolulu",
        "Hongkong" => "Asia/Hong_Kong",
        "Iceland" => "Atlantic/Reykjavik",
        "Iran" => "Asia/Tehran",
        "Israel" => "Asia/Jerusalem",
        "Jamaica" => "America/Jamaica",
        "Japan" => "Asia/Tokyo",
        "Kwajalein" => "Pacific/Kwajalein",
        "Libya" => "Africa/Tripoli",
        "MET" => "Europe/Brussels",
        "MST" => "America/Phoenix",
        "MST7MDT" => "America/Denver",
        "Mexico/BajaNorte" => "America/Tijuana",
        "Mexico/BajaSur" => "America/Mazatlan",
        "Mexico/General" => "America/Mexico_City",
        "NZ" => "Pacific/Auckland",
        "NZ-CHAT" => "Pacific/Chatham",
        "Navajo" => "America/Denver",
        "PRC" => "Asia/Shanghai",
        "Poland" => "Europe/Warsaw",
        "Portugal" | "WET" => "Europe/Lisbon",
        "PST8PDT" => "America/Los_Angeles",
        "ROC" => "Asia/Taipei",
        "ROK" => "Asia/Seoul",
        "Singapore" => "Asia/Singapore",
        "Turkey" => "Europe/Istanbul",
        "W-SU" => "Europe/Moscow",
        value => value,
    }
}

const DAYS_PER_400_YEAR_CYCLE: i64 = 146_097;
const YEARS_PER_GREGORIAN_CYCLE: i64 = 400;
const ISO_EPOCH_OFFSET_DAYS: i64 = 719_468;
const DAYS_PER_COMMON_YEAR: i64 = 365;
const DAYS_PER_4_YEAR_CYCLE: i64 = 1_460;
const DAYS_PER_CENTURY: i64 = 36_524;
const DAYS_BEFORE_LAST_400_YEAR_DAY: i64 = 146_096;
const DAYS_PER_MONTH_TRANSFORM_CYCLE: i64 = 153;
const MONTH_TRANSFORM_DIVISOR: i64 = 5;
const ISO_MONTHS_PER_YEAR: i64 = 12;
const ISO_DAYS_PER_WEEK: i64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsoDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

pub fn is_leap_year(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

pub fn days_in_month(year: i32, month: u32) -> Option<u32> {
    Some(match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    })
}

/// Add calendar units, regulate the day, then add weeks and days. Bounds on
/// Temporal values belong to the caller, rather than intermediate ISO dates.
pub fn add_iso_date(
    date: IsoDate,
    (years, months, weeks, days): (i64, i64, i64, i64),
    constrain: bool,
) -> Option<IsoDate> {
    if !(1..=days_in_month(date.year, date.month)?).contains(&date.day) {
        return None;
    }
    let month_index = i64::from(date.year)
        .checked_add(years)?
        .checked_mul(ISO_MONTHS_PER_YEAR)?
        .checked_add(i64::from(date.month) - 1)?
        .checked_add(months)?;
    let year = i32::try_from(month_index.div_euclid(ISO_MONTHS_PER_YEAR)).ok()?;
    let month = u32::try_from(month_index.rem_euclid(ISO_MONTHS_PER_YEAR)).ok()? + 1;
    let last_day = days_in_month(year, month)?;
    let day = if constrain {
        date.day.min(last_day)
    } else if date.day <= last_day {
        date.day
    } else {
        return None;
    };
    let days = weeks.checked_mul(ISO_DAYS_PER_WEEK)?.checked_add(days)?;
    civil_from_days(days_from_civil(IsoDate { year, month, day }).checked_add(days)?)
}

pub fn days_from_civil(date: IsoDate) -> i64 {
    let year = i64::from(date.year) - i64::from(date.month <= 2);
    let era = year.div_euclid(YEARS_PER_GREGORIAN_CYCLE);
    let year_of_era = year - era * YEARS_PER_GREGORIAN_CYCLE;
    let adjusted_month = i64::from(date.month) + if date.month > 2 { -3 } else { 9 };
    let day_of_year = (DAYS_PER_MONTH_TRANSFORM_CYCLE * adjusted_month + 2)
        / MONTH_TRANSFORM_DIVISOR
        + i64::from(date.day)
        - 1;
    let day_of_era =
        year_of_era * DAYS_PER_COMMON_YEAR + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * DAYS_PER_400_YEAR_CYCLE + day_of_era - ISO_EPOCH_OFFSET_DAYS
}

pub fn civil_from_days(days: i64) -> Option<IsoDate> {
    let adjusted_days = days.checked_add(ISO_EPOCH_OFFSET_DAYS)?;
    let era = adjusted_days.div_euclid(DAYS_PER_400_YEAR_CYCLE);
    let day_of_era = adjusted_days - era * DAYS_PER_400_YEAR_CYCLE;
    let year_of_era = (day_of_era - day_of_era / DAYS_PER_4_YEAR_CYCLE
        + day_of_era / DAYS_PER_CENTURY
        - day_of_era / DAYS_BEFORE_LAST_400_YEAR_DAY)
        / DAYS_PER_COMMON_YEAR;
    let year = i32::try_from(year_of_era + era * YEARS_PER_GREGORIAN_CYCLE).ok()?;
    let day_of_year =
        day_of_era - (DAYS_PER_COMMON_YEAR * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (MONTH_TRANSFORM_DIVISOR * day_of_year + 2) / DAYS_PER_MONTH_TRANSFORM_CYCLE;
    let day = u32::try_from(
        day_of_year - (DAYS_PER_MONTH_TRANSFORM_CYCLE * month_part + 2) / MONTH_TRANSFORM_DIVISOR
            + 1,
    )
    .ok()?;
    let month = u32::try_from(month_part + if month_part < 10 { 3 } else { -9 }).ok()?;
    let year = year + i32::from(month <= 2);
    Some(IsoDate { year, month, day })
}
