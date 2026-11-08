#include "app/connections.hpp"
#include <algorithm>
#include <cctype>

// The parsing half of connections.hpp (no processes): remote urls, ssh greetings, credential answers.
namespace imgit::app
{
    namespace
    {
        std::string lower(std::string_view s)
        {
            std::string out(s);
            std::transform(out.begin(), out.end(), out.begin(),
                           [](unsigned char c)
                           {
                               return static_cast<char>(std::tolower(c));
                           });
            return out;
        }

        bool ends_with_domain(std::string_view host, std::string_view domain)
        {
            return host == domain
                   || (host.size() > domain.size() && host.ends_with(domain)
                       && host[host.size() - domain.size() - 1] == '.');
        }

        // "[user@]host[:port]" into the endpoint.
        void split_authority(std::string_view authority, RemoteEndpoint& endpoint)
        {
            if (const auto at = authority.rfind('@'); at != std::string_view::npos)
            {
                endpoint.user = std::string(authority.substr(0, std::min(authority.find(':'), at)));
                authority = authority.substr(at + 1);
            }
            if (authority.starts_with('['))
            {
                // [::1]:22
                const auto close = authority.find(']');
                endpoint.host = lower(authority.substr(1, close == std::string_view::npos ? close : close - 1));
                if (close != std::string_view::npos && close + 1 < authority.size() && authority[close + 1] == ':')
                {
                    endpoint.port = std::string(authority.substr(close + 2));
                }
                return;
            }
            const auto colon = authority.find(':');
            endpoint.host = lower(authority.substr(0, colon));
            if (colon != std::string_view::npos)
            {
                endpoint.port = std::string(authority.substr(colon + 1));
            }
        }

        std::string trim(std::string_view s)
        {
            while (!s.empty() && std::isspace(static_cast<unsigned char>(s.front())))
            {
                s.remove_prefix(1);
            }
            while (!s.empty() && std::isspace(static_cast<unsigned char>(s.back())))
            {
                s.remove_suffix(1);
            }
            return std::string(s);
        }

        // The text between `open` and the first of `close` after it, in `text` (`lowered` finds `open`).
        std::string between(std::string_view text, std::string_view lowered, std::string_view open,
                            std::string_view close)
        {
            const auto at = lowered.find(open);
            if (at == std::string_view::npos)
            {
                return {};
            }
            const auto start = at + open.size();
            const auto end = text.find_first_of(close, start);
            return trim(text.substr(start, end == std::string_view::npos ? end : end - start));
        }
    } // namespace

    std::optional<RemoteEndpoint> parse_remote_url(std::string_view url)
    {
        RemoteEndpoint endpoint;
        if (const auto scheme_end = url.find("://"); scheme_end != std::string_view::npos)
        {
            const std::string scheme = lower(url.substr(0, scheme_end));
            if (scheme == "https" || scheme == "http")
            {
                endpoint.transport = RemoteEndpoint::Transport::HTTPS;
            }
            else if (scheme == "ssh" || scheme == "git+ssh" || scheme == "ssh+git")
            {
                endpoint.transport = RemoteEndpoint::Transport::SSH;
            }
            else
            {
                return std::nullopt;
            }
            std::string_view authority = url.substr(scheme_end + 3);
            authority = authority.substr(0, authority.find('/'));
            split_authority(authority, endpoint);
        }
        else
        {
            // scp-style: [user@]host:path, where the colon comes before any slash (and is not a drive letter).
            const auto colon = url.find(':');
            const auto slash = url.find_first_of("/\\");
            if (colon == std::string_view::npos || (slash != std::string_view::npos && slash < colon) || colon < 2)
            {
                return std::nullopt;
            }
            endpoint.transport = RemoteEndpoint::Transport::SSH;
            const std::string_view authority = url.substr(0, colon);
            if (const auto at = authority.rfind('@'); at != std::string_view::npos)
            {
                endpoint.user = std::string(authority.substr(0, at));
                endpoint.host = lower(authority.substr(at + 1));
            }
            else
            {
                endpoint.host = lower(authority);
            }
        }
        if (endpoint.host.empty())
        {
            return std::nullopt;
        }
        return endpoint;
    }

    const std::vector<KnownService>& known_services()
    {
        static const std::vector<KnownService> services = {
                {"GitHub", "github.com", "github.com"},
                {"GitLab", "gitlab.com", "gitlab.com"},
                {"Bitbucket", "bitbucket.org", "bitbucket.org"},
                {"Azure DevOps", "dev.azure.com", "ssh.dev.azure.com"},
        };
        return services;
    }

    const KnownService* service_of(std::string_view host)
    {
        const std::string h = lower(host);
        for (const KnownService& service : known_services())
        {
            if (ends_with_domain(h, service.host) || ends_with_domain(h, service.ssh_host))
            {
                return &service;
            }
        }
        // Azure DevOps' older hosts: <org>.visualstudio.com and vs-ssh.visualstudio.com.
        if (ends_with_domain(h, "visualstudio.com"))
        {
            return &known_services()[3];
        }
        return nullptr;
    }

    SshAnswer parse_ssh_answer(std::string_view output, int exit_code)
    {
        const std::string lowered = lower(output);
        SshAnswer answer;
        const auto has = [&](std::string_view needle)
        {
            return lowered.find(needle) != std::string::npos;
        };
        // The services' greetings: GitHub "Hi <user>! You've successfully authenticated...", Gitea / Forgejo
        // "Hi there, <user>! ...", GitLab "Welcome to GitLab, @<user>!", Bitbucket "authenticated via ssh key" (and
        // formerly "logged in as <user>."), Azure DevOps "Shell access is not supported."
        if (has("successfully authenticated"))
        {
            answer.authenticated = true;
            // Only the greeting's own line (ssh's warnings may come before it).
            const auto at = lowered.find("successfully authenticated");
            const auto line_start = lowered.rfind('\n', at);
            const std::size_t from = line_start == std::string::npos ? 0 : line_start + 1;
            const std::string_view line = output.substr(from, at - from);
            const std::string line_lowered = lowered.substr(from, at - from);
            if (line_lowered.starts_with("hi there, "))
            {
                answer.user = between(line, line_lowered, "hi there, ", "!");
            }
            else if (line_lowered.starts_with("hi "))
            {
                answer.user = between(line, line_lowered, "hi ", "!");
            }
            return answer;
        }
        if (has("welcome to gitlab"))
        {
            answer.authenticated = true;
            answer.user = between(output, lowered, ", @", "!\r\n");
            return answer;
        }
        if (has("logged in as "))
        {
            answer.authenticated = true;
            std::string user = between(output, lowered, "logged in as ", "\r\n");
            if (user.ends_with('.'))
            {
                user.pop_back();
            }
            answer.user = user;
            return answer;
        }
        if (has("authenticated via") || has("shell access is not supported") || has("shell access is disabled"))
        {
            answer.authenticated = true;
            return answer;
        }

        static const std::pair<std::string_view, std::string_view> failures[] = {
                {"permission denied", "The server refused your SSH key"},
                {"host key verification failed",
                 "The host is not in known_hosts yet: connect once with ssh to trust its key"},
                {"remote host identification has changed", "The host's key changed since it was trusted"},
                {"could not resolve hostname", "Could not resolve the host"},
                {"timed out", "The connection timed out"},
                {"connection refused", "The connection was refused"},
                {"no route to host", "No route to the host"},
                {"connection closed", "The server closed the connection"},
        };
        for (const auto& [needle, detail] : failures)
        {
            if (has(needle))
            {
                answer.detail = std::string(detail);
                return answer;
            }
        }
        if (exit_code == 0)
        {
            answer.authenticated = true;
            return answer;
        }
        // The last line ssh printed, as is.
        std::string_view rest = output;
        std::string last;
        while (!rest.empty())
        {
            const auto newline = rest.find('\n');
            std::string line = trim(rest.substr(0, newline));
            if (!line.empty())
            {
                last = std::move(line);
            }
            if (newline == std::string_view::npos)
            {
                break;
            }
            rest.remove_prefix(newline + 1);
        }
        answer.detail = last.empty() ? "ssh exited with code " + std::to_string(exit_code) : last;
        return answer;
    }

    std::string credential_username(std::string_view fill_output)
    {
        std::string_view rest = fill_output;
        while (!rest.empty())
        {
            const auto newline = rest.find('\n');
            std::string_view line = rest.substr(0, newline);
            if (line.ends_with('\r'))
            {
                line.remove_suffix(1);
            }
            if (line.starts_with("username="))
            {
                return std::string(line.substr(9));
            }
            if (newline == std::string_view::npos)
            {
                break;
            }
            rest.remove_prefix(newline + 1);
        }
        return {};
    }
} // namespace imgit::app
