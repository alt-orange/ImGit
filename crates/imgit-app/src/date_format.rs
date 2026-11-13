//! The Date/Time preferences' formatting: moment.js format tokens with month and weekday names in the chosen
//! locale.

use std::fmt::Write as _;

/// A day of the week, Sunday first (as `tm_wday` and moment.js count them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Weekday {
    #[default]
    Sunday,
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
}

impl Weekday {
    pub const ALL: [Self; 7] = [
        Self::Sunday,
        Self::Monday,
        Self::Tuesday,
        Self::Wednesday,
        Self::Thursday,
        Self::Friday,
        Self::Saturday,
    ];

    /// The weekday `days` after Sunday (any number: it wraps around the week).
    #[must_use]
    pub const fn from_days_since_sunday(days: u32) -> Self {
        Self::ALL[(days % 7) as usize]
    }

    /// The weekday of a civil date (proleptic Gregorian calendar; `month` and `day` are 1-based).
    #[must_use]
    pub const fn of_date(year: i32, month: u8, day: u8) -> Self {
        // Sakamoto's method.
        const OFFSETS: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let month_index = if month == 0 {
            0
        } else if month > 12 {
            11
        } else {
            (month - 1) as usize
        };
        let y = year as i64 - if month_index < 2 { 1 } else { 0 };
        let days = y + y.div_euclid(4) - y.div_euclid(100)
            + y.div_euclid(400)
            + OFFSETS[month_index]
            + day as i64;
        Self::ALL[days.rem_euclid(7) as usize]
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// A broken-down local time: what a format string is filled in from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DateTime {
    /// The full year (2026).
    pub year: i32,
    /// 1 (January) to 12.
    pub month: u8,
    /// The day of the month, from 1.
    pub day: u8,
    pub weekday: Weekday,
    /// 0 to 23.
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl DateTime {
    /// The time on a civil date, with its weekday worked out.
    #[must_use]
    pub const fn new(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Self {
        Self {
            year,
            month,
            day,
            weekday: Weekday::of_date(year, month, day),
            hour,
            minute,
            second,
        }
    }

    /// The time in a moment.js `format` with `locale`'s names: [`moment_format`].
    #[must_use]
    pub fn format(&self, format: &str, locale: &str) -> String {
        moment_format(format, self, locale)
    }
}

impl From<imgit_platform::system::LocalTime> for DateTime {
    fn from(time: imgit_platform::system::LocalTime) -> Self {
        Self {
            year: time.year,
            month: time.month,
            day: time.day,
            weekday: Weekday::from_days_since_sunday(u32::from(time.weekday)),
            hour: time.hour,
            minute: time.minute,
            second: time.second,
        }
    }
}

struct Names {
    language: &'static str,
    months: [&'static str; 12],
    months_short: [&'static str; 12],
    /// Sunday first.
    weekdays: [&'static str; 7],
    weekdays_short: [&'static str; 7],
}

/// The languages of the Locale list (moment's names for them); English first.
#[rustfmt::skip]
static NAMES: [Names; 8] = [
    Names {
        language: "en",
        months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
                 "November", "December"],
        months_short: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
        weekdays: ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
        weekdays_short: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    },
    Names {
        language: "fr",
        months: ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre",
                 "novembre", "décembre"],
        months_short: ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
                       "déc."],
        weekdays: ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"],
        weekdays_short: ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."],
    },
    Names {
        language: "de",
        months: ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober",
                 "November", "Dezember"],
        months_short: ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sep.", "Okt.", "Nov.", "Dez."],
        weekdays: ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"],
        weekdays_short: ["So.", "Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa."],
    },
    Names {
        language: "es",
        months: ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre",
                 "noviembre", "diciembre"],
        months_short: ["ene.", "feb.", "mar.", "abr.", "may.", "jun.", "jul.", "ago.", "sept.", "oct.", "nov.",
                       "dic."],
        weekdays: ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"],
        weekdays_short: ["dom.", "lun.", "mar.", "mié.", "jue.", "vie.", "sáb."],
    },
    Names {
        language: "it",
        months: ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre",
                 "ottobre", "novembre", "dicembre"],
        months_short: ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"],
        weekdays: ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"],
        weekdays_short: ["dom", "lun", "mar", "mer", "gio", "ven", "sab"],
    },
    Names {
        language: "ja",
        months: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
        months_short: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
        weekdays: ["日曜日", "月曜日", "火曜日", "水曜日", "木曜日", "金曜日", "土曜日"],
        weekdays_short: ["日", "月", "火", "水", "木", "金", "土"],
    },
    Names {
        language: "ru",
        months: ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября",
                 "ноября", "декабря"],
        months_short: ["янв.", "февр.", "мар.", "апр.", "мая", "июня", "июля", "авг.", "сент.", "окт.", "нояб.",
                       "дек."],
        weekdays: ["воскресенье", "понедельник", "вторник", "среда", "четверг", "пятница", "суббота"],
        weekdays_short: ["вс", "пн", "вт", "ср", "чт", "пт", "сб"],
    },
    Names {
        language: "zh",
        months: ["一月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "十一月", "十二月"],
        months_short: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
        weekdays: ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"],
        weekdays_short: ["周日", "周一", "周二", "周三", "周四", "周五", "周六"],
    },
];

/// The names for a locale code's language ("de-de" -> German); English for one ImGit has no names for.
fn names_for(locale: &str) -> &'static Names {
    let language = locale.split('-').next().unwrap_or(locale);
    NAMES
        .iter()
        .find(|names| names.language == language)
        .unwrap_or(&NAMES[0])
}

/// "1st", "2nd", "3rd", "4th", "11th"...
fn ordinal(day: u8) -> String {
    let suffix = match (day % 10, day) {
        (1, d) if d != 11 => "st",
        (2, d) if d != 12 => "nd",
        (3, d) if d != 13 => "rd",
        _ => "th",
    };
    format!("{day}{suffix}")
}

/// The language a Locale option names: `French [fr]` -> "fr", `English (United Kingdom) [en-gb]` -> "en-gb";
/// `System [system]` -> `system_locale` ("en-us", "de_DE"...; "en" when it is unknown). Lower case, with `-`
/// between the parts.
#[must_use]
pub fn locale_code(option: &str, system_locale: Option<&str>) -> String {
    let code = match (option.rfind('['), option.rfind(']')) {
        (Some(open), Some(close)) if close > open => &option[open + 1..close],
        _ => option,
    };
    let code = if code == "system" {
        system_locale
            .filter(|locale| !locale.is_empty())
            .unwrap_or("en")
    } else {
        code
    };
    code.chars()
        .map(|c| {
            if c == '_' {
                '-'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

/// `t` in a moment.js `format`: `YYYY YY`, `MMMM MMM MM M`, `DD D Do`, `dddd ddd`, `HH H`, `hh h`, `mm m`, `ss s`,
/// `A a`, and `[literal text]`; anything else is copied. Month and weekday names follow `locale` (a
/// [`locale_code`]; English for one ImGit has no names for).
#[must_use]
pub fn moment_format(format: &str, t: &DateTime, locale: &str) -> String {
    /// The tokens, longest first where one starts another.
    #[derive(Clone, Copy)]
    enum Token {
        Year,
        YearShort,
        MonthName,
        MonthShortName,
        MonthPadded,
        Month,
        DayPadded,
        DayOrdinal,
        Day,
        WeekdayName,
        WeekdayShortName,
        HourPadded,
        Hour,
        Hour12Padded,
        Hour12,
        MinutePadded,
        Minute,
        SecondPadded,
        Second,
        MeridiemUpper,
        MeridiemLower,
    }
    const TOKENS: [(&str, Token); 21] = [
        ("YYYY", Token::Year),
        ("YY", Token::YearShort),
        ("MMMM", Token::MonthName),
        ("MMM", Token::MonthShortName),
        ("MM", Token::MonthPadded),
        ("M", Token::Month),
        ("DD", Token::DayPadded),
        ("Do", Token::DayOrdinal),
        ("D", Token::Day),
        ("dddd", Token::WeekdayName),
        ("ddd", Token::WeekdayShortName),
        ("HH", Token::HourPadded),
        ("H", Token::Hour),
        ("hh", Token::Hour12Padded),
        ("h", Token::Hour12),
        ("mm", Token::MinutePadded),
        ("m", Token::Minute),
        ("ss", Token::SecondPadded),
        ("s", Token::Second),
        ("A", Token::MeridiemUpper),
        ("a", Token::MeridiemLower),
    ];

    let names = names_for(locale);
    let month = t.month.clamp(1, 12);
    let month_index = usize::from(month - 1);
    let weekday = t.weekday.index();
    let hour12 = match t.hour % 12 {
        0 => 12,
        hour => hour,
    };
    let morning = t.hour < 12;

    let mut out = String::with_capacity(format.len() + 8);
    let mut rest = format;
    while let Some(first) = rest.chars().next() {
        if first == '['
            && let Some(close) = rest.find(']')
        {
            out.push_str(&rest[1..close]);
            rest = &rest[close + 1..];
            continue;
        }
        let token = TOKENS
            .iter()
            .find_map(|&(text, token)| Some((rest.strip_prefix(text)?, token)));
        let Some((after, token)) = token else {
            out.push(first);
            rest = &rest[first.len_utf8()..];
            continue;
        };
        rest = after;
        // Writing to a String cannot fail.
        let _ = match token {
            Token::Year => write!(out, "{}", t.year),
            Token::YearShort => write!(out, "{:02}", t.year.rem_euclid(100)),
            Token::MonthName => write!(out, "{}", names.months[month_index]),
            Token::MonthShortName => write!(out, "{}", names.months_short[month_index]),
            Token::MonthPadded => write!(out, "{month:02}"),
            Token::Month => write!(out, "{month}"),
            Token::DayPadded => write!(out, "{:02}", t.day),
            Token::DayOrdinal if names.language == "en" => write!(out, "{}", ordinal(t.day)),
            Token::DayOrdinal => write!(out, "{}.", t.day),
            Token::Day => write!(out, "{}", t.day),
            Token::WeekdayName => write!(out, "{}", names.weekdays[weekday]),
            Token::WeekdayShortName => write!(out, "{}", names.weekdays_short[weekday]),
            Token::HourPadded => write!(out, "{:02}", t.hour),
            Token::Hour => write!(out, "{}", t.hour),
            Token::Hour12Padded => write!(out, "{hour12:02}"),
            Token::Hour12 => write!(out, "{hour12}"),
            Token::MinutePadded => write!(out, "{:02}", t.minute),
            Token::Minute => write!(out, "{}", t.minute),
            Token::SecondPadded => write!(out, "{:02}", t.second),
            Token::Second => write!(out, "{}", t.second),
            Token::MeridiemUpper => write!(out, "{}", if morning { "AM" } else { "PM" }),
            Token::MeridiemLower => write!(out, "{}", if morning { "am" } else { "pm" }),
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thursday, March 5, 2026 14:07:09
    fn sample() -> DateTime {
        DateTime {
            year: 2026,
            month: 3,
            day: 5,
            weekday: Weekday::Thursday,
            hour: 14,
            minute: 7,
            second: 9,
        }
    }

    fn english(format: &str, t: &DateTime) -> String {
        moment_format(format, t, "en")
    }

    #[test]
    fn moment_format_defaults() {
        assert_eq!(english("MM/DD/YYYY", &sample()), "03/05/2026");
        assert_eq!(english("MMMM D, YYYY", &sample()), "March 5, 2026");
        assert_eq!(
            english("MM/DD/YYYY @ h:mm A", &sample()),
            "03/05/2026 @ 2:07 PM"
        );
        assert_eq!(
            english("MMMM D, YYYY h:mm A", &sample()),
            "March 5, 2026 2:07 PM"
        );
    }

    #[test]
    fn moment_format_tokens() {
        assert_eq!(english("YY-M-D H:m:s", &sample()), "26-3-5 14:7:9");
        assert_eq!(
            english("dddd, MMM Do HH:mm:ss a", &sample()),
            "Thursday, Mar 5th 14:07:09 pm"
        );
        assert_eq!(english("hh A", &sample()), "02 PM");
        assert_eq!(english("[Today is] dddd", &sample()), "Today is Thursday");
        let midnight = DateTime {
            hour: 0,
            ..sample()
        };
        assert_eq!(english("h:mm a", &midnight), "12:07 am");
    }

    #[test]
    fn moment_format_locales() {
        assert_eq!(moment_format("D MMMM YYYY", &sample(), "fr"), "5 mars 2026");
        assert_eq!(
            moment_format("dddd D. MMMM", &sample(), "de"),
            "Donnerstag 5. März"
        );
        assert_eq!(moment_format("MMMM", &sample(), "en-gb"), "March");
        assert_eq!(moment_format("MMMM", &sample(), "xx"), "March"); // unknown: English
        assert_eq!(moment_format("Do", &sample(), "es"), "5.");
    }

    #[test]
    fn moment_format_copies_everything_else() {
        assert_eq!(english("YYYY年 [unclosed", &sample()), "2026年 [unclo9ed");
        assert_eq!(english("é[à]ü", &sample()), "éàü");
        assert_eq!(sample().format("Do", "en"), "5th");
    }

    #[test]
    fn ordinals() {
        let days = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 30, 31].map(ordinal);
        assert_eq!(
            days,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "23rd", "30th",
                "31st"
            ]
        );
    }

    #[test]
    fn locale_code_options() {
        assert_eq!(locale_code("French [fr]", Some("de-de")), "fr");
        assert_eq!(
            locale_code("English (United Kingdom) [en-gb]", None),
            "en-gb"
        );
        assert_eq!(locale_code("System [system]", Some("de_DE")), "de-de");
        assert_eq!(locale_code("System [system]", None), "en");
        assert_eq!(locale_code("System [system]", Some("")), "en");
        assert_eq!(locale_code("PT_br", None), "pt-br");
    }

    #[test]
    fn weekdays_of_dates() {
        assert_eq!(Weekday::of_date(2026, 3, 5), Weekday::Thursday);
        assert_eq!(Weekday::of_date(1970, 1, 1), Weekday::Thursday);
        assert_eq!(Weekday::of_date(2000, 2, 29), Weekday::Tuesday);
        assert_eq!(Weekday::of_date(2024, 12, 31), Weekday::Tuesday);
        assert_eq!(DateTime::new(2026, 3, 5, 14, 7, 9), sample());
        assert_eq!(Weekday::from_days_since_sunday(11), Weekday::Thursday);
    }
}
