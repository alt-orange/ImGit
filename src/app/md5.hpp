#pragma once

#include <string>
#include <string_view>

namespace imgit::app
{
    // MD5 (RFC 1321) of `text` as 32 lowercase hex digits: Gravatar's avatar key.
    [[nodiscard]] std::string md5_hex(std::string_view text);
} // namespace imgit::app
