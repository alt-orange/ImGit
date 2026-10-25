#pragma once

// Helpers shared by the *_win32.cpp files only.
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <string>
#include <vector>
#include <windows.h>

namespace imgit::platform::win32
{
    inline std::wstring widen(const std::string& text)
    {
        const int length = MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), nullptr, 0);
        std::wstring wide(static_cast<std::size_t>(length), L'\0');
        MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), wide.data(), length);
        return wide;
    }

    inline std::string narrow(const wchar_t* text)
    {
        const int length = WideCharToMultiByte(CP_UTF8, 0, text, -1, nullptr, 0, nullptr, nullptr);
        std::string result(static_cast<std::size_t>(length > 0 ? length - 1 : 0), '\0');
        WideCharToMultiByte(CP_UTF8, 0, text, -1, result.data(), length, nullptr, nullptr);
        return result;
    }

    // One argument quoted the way CommandLineToArgvW (and the MSVC CRT) splits it back: backslashes are literal
    // unless they precede a quote.
    inline std::wstring quote_argument(const std::wstring& arg)
    {
        if (!arg.empty() && arg.find_first_of(L" \t\n\v\"") == std::wstring::npos)
        {
            return arg;
        }
        std::wstring out = L"\"";
        std::size_t backslashes = 0;
        for (const wchar_t c : arg)
        {
            if (c == L'\\')
            {
                ++backslashes;
                continue;
            }
            out.append(c == L'"' ? backslashes * 2 + 1 : backslashes, L'\\');
            backslashes = 0;
            out += c;
        }
        out.append(backslashes * 2, L'\\');
        return out + L'"';
    }

    inline std::wstring command_line(const std::vector<std::string>& args)
    {
        std::wstring line;
        for (const std::string& arg : args)
        {
            if (!line.empty())
            {
                line += L' ';
            }
            line += quote_argument(widen(arg));
        }
        return line;
    }
} // namespace imgit::platform::win32
