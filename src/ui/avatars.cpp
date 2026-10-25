#include "ui/avatars.hpp"
#include "app/assets.hpp"
#include "app/md5.hpp"
#include "gfx/image.hpp"
#include "platform/system.hpp"
#include "ui/vector_icons.hpp"
#include <algorithm>
#include <atomic>
#include <cctype>
#include <condition_variable>
#include <cpr/cpr.h>
#include <deque>
#include <filesystem>
#include <fstream>
#include <map>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>

namespace imgit::ui
{
    namespace
    {
        struct Request
        {
            std::string key;
            std::string email;
            int size = 0;
        };

        enum class State
        {
            QUEUED,
            DECODED,
            READY,
            FAILED,
        };

        struct Entry
        {
            State state = State::QUEUED;
            int width = 0;
            int height = 0;
            std::vector<unsigned char> rgba;
            ImTextureID texture = ImTextureID_Invalid;
        };

        std::mutex g_mutex;
        std::condition_variable g_wake;
        std::deque<Request> g_queue;
        std::unordered_map<std::string, Entry> g_entries;
        std::atomic<int> g_in_flight{0};
        std::atomic<bool> g_enabled{true};
        std::atomic<bool> g_stop{false};
        std::thread g_worker;
        std::filesystem::path g_cache_dir;

        std::string lowercase(std::string text)
        {
            std::transform(text.begin(), text.end(), text.begin(),
                           [](unsigned char c)
                           {
                               return static_cast<char>(std::tolower(c));
                           });
            return text;
        }

        // Without the spaces an author's email sometimes carries around it.
        std::string trim(const std::string& text)
        {
            const std::size_t begin = text.find_first_not_of(" \t\r\n");
            if (begin == std::string::npos)
            {
                return {};
            }
            const std::size_t end = text.find_last_not_of(" \t\r\n");
            return text.substr(begin, end - begin + 1);
        }

        // GETs `url` into `file` (through a temporary name, so a cut-off download is never taken for an image).
        bool download(const std::string& url, const std::filesystem::path& file)
        {
            // One session for the worker thread: its connection to gravatar.com is kept alive between avatars.
            static thread_local cpr::Session session;
            static thread_local bool configured = false;
            if (!configured)
            {
                configured = true;
                session.SetTimeout(cpr::Timeout{30000});
                // The system proxy, as the Windows download API used before honoured it.
                if (const std::string proxy = platform::system_proxy(); !proxy.empty())
                {
                    session.SetProxies(cpr::Proxies{{"http", proxy}, {"https", proxy}});
                }
            }
            session.SetUrl(cpr::Url{url});
            const cpr::Response response = session.Get();
            // No Gravatar for the address: remember it, so the initials are drawn without asking again.
            if (!response.error && response.status_code == 404)
            {
                std::filesystem::path none = file;
                none += ".none";
                std::ofstream{none};
                return false;
            }
            if (response.error || response.status_code != 200 || response.text.empty())
            {
                return false;
            }
            std::filesystem::path partial = file;
            partial += ".part";
            {
                std::ofstream out(partial, std::ios::binary);
                out.write(response.text.data(), static_cast<std::streamsize>(response.text.size()));
                if (!out)
                {
                    return false;
                }
            }
            std::error_code error;
            std::filesystem::rename(partial, file, error);
            return !error;
        }

        void process(const Request& request)
        {
            Entry result;
            result.state = State::FAILED;
            const std::string hash = app::md5_hex(lowercase(trim(request.email)));
            if (!g_cache_dir.empty())
            {
                std::error_code error;
                std::filesystem::create_directories(g_cache_dir, error);
                const std::filesystem::path file = g_cache_dir / (hash + "-" + std::to_string(request.size) + ".png");
                std::filesystem::path none = file;
                none += ".none";
                // d=404: an address without a Gravatar gets the initials instead of a generated picture.
                const bool have = std::filesystem::exists(file, error)
                                  || (!std::filesystem::exists(none, error)
                                      && download("https://www.gravatar.com/avatar/" + hash
                                                          + "?d=404&s=" + std::to_string(request.size),
                                                  file));
                if (std::optional<gfx::Image> image = have ? gfx::load_image(file) : std::nullopt; image)
                {
                    result.width = image->width;
                    result.height = image->height;
                    result.rgba = std::move(image->rgba);
                    result.state = State::DECODED;
                }
                else if (have)
                {
                    // A file that does not decode (a bad download) goes: the next request downloads it again.
                    std::filesystem::remove(file, error);
                }
            }
            std::lock_guard lock(g_mutex);
            g_entries[request.key] = std::move(result);
        }

        void worker()
        {
            while (true)
            {
                Request request;
                {
                    std::unique_lock lock(g_mutex);
                    g_wake.wait(lock,
                                []
                                {
                                    return g_stop.load() || !g_queue.empty();
                                });
                    if (g_stop.load())
                    {
                        break;
                    }
                    request = std::move(g_queue.front());
                    g_queue.pop_front();
                }
                process(request);
                --g_in_flight;
            }
        }
    } // namespace

    void set_avatar_cache_directory(const std::string& directory)
    {
        g_cache_dir = directory;
    }

    void set_avatars_enabled(bool enabled) noexcept
    {
        g_enabled = enabled;
    }

    ImTextureID avatar_texture(const std::string& email, int size)
    {
        // An empty email asks Gravatar for md5("").
        if (!g_enabled.load() || size <= 0)
        {
            return ImTextureID_Invalid;
        }
        const std::string key = lowercase(email) + "#" + std::to_string(size);
        std::lock_guard lock(g_mutex);
        auto found = g_entries.find(key);
        if (found == g_entries.end())
        {
            g_entries.emplace(key, Entry{});
            g_queue.push_back({key, email, size});
            ++g_in_flight;
            if (!g_worker.joinable())
            {
                g_worker = std::thread(worker);
            }
            g_wake.notify_one();
            return ImTextureID_Invalid;
        }
        Entry& entry = found->second;
        if (entry.state == State::DECODED)
        {
            entry.texture = create_texture(entry.width, entry.height, entry.rgba.data());
            entry.rgba.clear();
            entry.rgba.shrink_to_fit();
            entry.state = entry.texture == ImTextureID_Invalid ? State::FAILED : State::READY;
        }
        return entry.state == State::READY ? entry.texture : ImTextureID_Invalid;
    }

    bool avatars_pending() noexcept
    {
        if (g_in_flight.load() > 0)
        {
            return true;
        }
        std::lock_guard lock(g_mutex);
        for (const auto& [key, entry] : g_entries)
        {
            if (entry.state == State::DECODED)
            {
                return true; // decoded, waiting for its texture upload on the next frame
            }
        }
        return false;
    }

    void shutdown_avatars()
    {
        {
            std::lock_guard lock(g_mutex);
            g_stop = true;
        }
        g_wake.notify_all();
        if (g_worker.joinable())
        {
            g_worker.join();
        }
        std::lock_guard lock(g_mutex);
        for (auto& [key, entry] : g_entries)
        {
            destroy_texture(entry.texture);
        }
        g_entries.clear();
    }
} // namespace imgit::ui
