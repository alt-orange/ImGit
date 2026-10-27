#include "platform/http.hpp"
#include "platform/system.hpp"
#include <cpr/cpr.h>

namespace imgit::platform
{
    HttpResponse http_get(const std::string& url)
    {
        // One session per thread: its connection to the host is kept alive between requests.
        static thread_local cpr::Session session;
        static thread_local bool configured = false;
        if (!configured)
        {
            configured = true;
            session.SetTimeout(cpr::Timeout{30000});
            // The system proxy, as the Windows download API used before honoured it.
            if (const std::string proxy = system_proxy(); !proxy.empty())
            {
                session.SetProxies(cpr::Proxies{{"http", proxy}, {"https", proxy}});
            }
        }
        session.SetUrl(cpr::Url{url});
        cpr::Response response = session.Get();
        if (response.error)
        {
            return {};
        }
        return {true, static_cast<int>(response.status_code), std::move(response.text)};
    }
} // namespace imgit::platform
