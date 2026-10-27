#include "platform/http.hpp"
#include <cstring>
#include <emscripten/fetch.h>

namespace imgit::platform
{
    HttpResponse http_get(const std::string& url)
    {
        // A synchronous fetch, which the browser allows on worker threads only. The host has to allow the page's
        // origin (CORS); gravatar.com allows any.
        emscripten_fetch_attr_t attributes;
        emscripten_fetch_attr_init(&attributes);
        std::strcpy(attributes.requestMethod, "GET");
        attributes.attributes = EMSCRIPTEN_FETCH_LOAD_TO_MEMORY | EMSCRIPTEN_FETCH_SYNCHRONOUS;
        attributes.timeoutMSecs = 30000;
        emscripten_fetch_t* fetch = emscripten_fetch(&attributes, url.c_str());
        if (fetch == nullptr)
        {
            return {};
        }
        HttpResponse response;
        // Status 0: the request never got an answer (offline, timed out, blocked by CORS).
        response.received = fetch->status != 0;
        response.status = fetch->status;
        if (fetch->data != nullptr && fetch->numBytes > 0)
        {
            response.body.assign(fetch->data, static_cast<std::size_t>(fetch->numBytes));
        }
        emscripten_fetch_close(fetch);
        return response;
    }
} // namespace imgit::platform
