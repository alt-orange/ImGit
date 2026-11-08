#include "app/date_format.hpp"
#include <algorithm>
#include <array>
#include <cctype>

namespace imgit::app
{
    namespace
    {
        struct Names
        {
            std::string_view language;
            std::array<std::string_view, 12> months;
            std::array<std::string_view, 12> months_short;
            std::array<std::string_view, 7> weekdays; // Sunday first
            std::array<std::string_view, 7> weekdays_short;
        };

        // The languages of the Locale list (moment's names for them).
        constexpr Names NAMES[] = {
                {"en",
                 {"January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
                  "November", "December"},
                 {"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"},
                 {"Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"},
                 {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"}},
                {"fr",
                 {"janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre",
                  "novembre", "décembre"},
                 {"janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."},
                 {"dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"},
                 {"dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."}},
                {"de",
                 {"Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober",
                  "November", "Dezember"},
                 {"Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sep.", "Okt.", "Nov.", "Dez."},
                 {"Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"},
                 {"So.", "Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa."}},
                {"es",
                 {"enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre",
                  "noviembre", "diciembre"},
                 {"ene.", "feb.", "mar.", "abr.", "may.", "jun.", "jul.", "ago.", "sept.", "oct.", "nov.", "dic."},
                 {"domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"},
                 {"dom.", "lun.", "mar.", "mié.", "jue.", "vie.", "sáb."}},
                {"it",
                 {"gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre",
                  "ottobre", "novembre", "dicembre"},
                 {"gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"},
                 {"domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"},
                 {"dom", "lun", "mar", "mer", "gio", "ven", "sab"}},
                {"ja",
                 {"1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"},
                 {"1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"},
                 {"日曜日", "月曜日", "火曜日", "水曜日", "木曜日", "金曜日", "土曜日"},
                 {"日", "月", "火", "水", "木", "金", "土"}},
                {"ru",
                 {"января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября",
                  "ноября", "декабря"},
                 {"янв.", "февр.", "мар.", "апр.", "мая", "июня", "июля", "авг.", "сент.", "окт.", "нояб.", "дек."},
                 {"воскресенье", "понедельник", "вторник", "среда", "четверг", "пятница", "суббота"},
                 {"вс", "пн", "вт", "ср", "чт", "пт", "сб"}},
                {"zh",
                 {"一月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "十一月", "十二月"},
                 {"1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"},
                 {"星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"},
                 {"周日", "周一", "周二", "周三", "周四", "周五", "周六"}},
        };

        const Names& names_for(std::string_view locale)
        {
            const std::string_view language = locale.substr(0, locale.find('-'));
            for (const Names& names : NAMES)
            {
                if (names.language == language)
                {
                    return names;
                }
            }
            return NAMES[0];
        }

        std::string ordinal(int day)
        {
            const char* suffix = (day % 10 == 1 && day != 11)   ? "st"
                                 : (day % 10 == 2 && day != 12) ? "nd"
                                 : (day % 10 == 3 && day != 13) ? "rd"
                                                                : "th";
            return std::to_string(day) + suffix;
        }
    } // namespace

    std::string locale_code(std::string_view option, std::string_view system_locale)
    {
        const auto open = option.rfind('[');
        const auto close = option.rfind(']');
        std::string code(open != std::string_view::npos && close != std::string_view::npos && close > open
                                 ? option.substr(open + 1, close - open - 1)
                                 : option);
        if (code == "system")
        {
            code = system_locale.empty() ? std::string("en") : std::string(system_locale);
        }
        std::transform(code.begin(), code.end(), code.begin(),
                       [](unsigned char c)
                       {
                           return c == '_' ? '-' : static_cast<char>(std::tolower(c));
                       });
        return code;
    }

    std::string moment_format(std::string_view format, const std::tm& t, std::string_view locale)
    {
        const Names& names = names_for(locale);
        const auto two = [](int v)
        {
            return (v < 10 ? "0" : "") + std::to_string(v);
        };
        const int month = std::clamp(t.tm_mon, 0, 11);
        const int weekday = std::clamp(t.tm_wday, 0, 6);
        const int hour12 = t.tm_hour % 12 == 0 ? 12 : t.tm_hour % 12;
        std::string out;
        std::size_t i = 0;
        while (i < format.size())
        {
            const auto take = [&](std::string_view token)
            {
                if (format.compare(i, token.size(), token) == 0)
                {
                    i += token.size();
                    return true;
                }
                return false;
            };
            if (format[i] == '[')
            {
                const auto close = format.find(']', i + 1);
                if (close != std::string_view::npos)
                {
                    out += format.substr(i + 1, close - i - 1);
                    i = close + 1;
                    continue;
                }
            }
            if (take("YYYY"))
            {
                out += std::to_string(t.tm_year + 1900);
            }
            else if (take("YY"))
            {
                out += two((t.tm_year + 1900) % 100);
            }
            else if (take("MMMM"))
            {
                out += names.months[static_cast<std::size_t>(month)];
            }
            else if (take("MMM"))
            {
                out += names.months_short[static_cast<std::size_t>(month)];
            }
            else if (take("MM"))
            {
                out += two(month + 1);
            }
            else if (take("M"))
            {
                out += std::to_string(month + 1);
            }
            else if (take("DD"))
            {
                out += two(t.tm_mday);
            }
            else if (take("Do"))
            {
                out += names.language == "en" ? ordinal(t.tm_mday) : std::to_string(t.tm_mday) + ".";
            }
            else if (take("D"))
            {
                out += std::to_string(t.tm_mday);
            }
            else if (take("dddd"))
            {
                out += names.weekdays[static_cast<std::size_t>(weekday)];
            }
            else if (take("ddd"))
            {
                out += names.weekdays_short[static_cast<std::size_t>(weekday)];
            }
            else if (take("HH"))
            {
                out += two(t.tm_hour);
            }
            else if (take("H"))
            {
                out += std::to_string(t.tm_hour);
            }
            else if (take("hh"))
            {
                out += two(hour12);
            }
            else if (take("h"))
            {
                out += std::to_string(hour12);
            }
            else if (take("mm"))
            {
                out += two(t.tm_min);
            }
            else if (take("m"))
            {
                out += std::to_string(t.tm_min);
            }
            else if (take("ss"))
            {
                out += two(t.tm_sec);
            }
            else if (take("s"))
            {
                out += std::to_string(t.tm_sec);
            }
            else if (take("A"))
            {
                out += t.tm_hour < 12 ? "AM" : "PM";
            }
            else if (take("a"))
            {
                out += t.tm_hour < 12 ? "am" : "pm";
            }
            else
            {
                out += format[i++];
            }
        }
        return out;
    }
} // namespace imgit::app
