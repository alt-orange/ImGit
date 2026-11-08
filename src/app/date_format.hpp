#pragma once

#include <ctime>
#include <string>
#include <string_view>

// The Date/Time preferences' formatting: moment.js format tokens with month and weekday names in the chosen
// locale.
namespace imgit::app
{
    // The language a Locale option names: "French [fr]" -> "fr", "English (United Kingdom) [en-gb]" -> "en-gb";
    // "System [system]" -> `system_locale` ("en-us", "de-de"...). Lower case.
    [[nodiscard]] std::string locale_code(std::string_view option, std::string_view system_locale);

    // `t` in a moment.js `format`: YYYY YY, MMMM MMM MM M, DD D Do, dddd ddd, HH H, hh h, mm m, ss s, A a, and
    // [literal text]; anything else is copied. Month and weekday names follow `locale` (a locale_code(); English
    // for one ImGit has no names for).
    [[nodiscard]] std::string moment_format(std::string_view format, const std::tm& t, std::string_view locale = "en");
} // namespace imgit::app
