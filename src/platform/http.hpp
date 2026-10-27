#pragma once

#include <string>

// HTTP downloads (the Gravatars): libcurl through cpr on the desktop, the browser's fetch in the browser.
namespace imgit::platform
{
    struct HttpResponse
    {
        bool received = false; // false: no answer at all (no connection, timed out, refused by the browser)
        int status = 0;
        std::string body;
    };

    // GETs `url` and waits for the whole answer, so it runs on a worker thread (the browser lets only those wait).
    // Each thread keeps its connection alive between calls; the system proxy is used where one is set.
    [[nodiscard]] HttpResponse http_get(const std::string& url);
} // namespace imgit::platform
