#include "git/git_cli.hpp"
#include "platform/process.hpp"
#include <algorithm>
#include <array>
#include <cstdio>
#include <cstdlib>
#include <filesystem>

namespace imgit::git::cli
{
    namespace
    {
        constexpr const char* ASKPASS_MARKER = "IMGIT_ASKPASS";
        constexpr const char* CREDENTIALS_VARIABLE = "IMGIT_ASKPASS_CREDENTIALS";

        // The credentials travel to the askpass child as hex fields (url:username:password;...), so any byte is safe.
        std::string hex(const std::string& text)
        {
            static constexpr char digits[] = "0123456789abcdef";
            std::string out;
            out.reserve(text.size() * 2);
            for (const char c : text)
            {
                const auto byte = static_cast<unsigned char>(c);
                out += digits[byte >> 4];
                out += digits[byte & 0xF];
            }
            return out;
        }

        std::string unhex(const std::string& text)
        {
            const auto nibble = [](char c)
            {
                return c >= 'a' ? c - 'a' + 10 : c - '0';
            };
            std::string out;
            for (std::size_t i = 0; i + 1 < text.size(); i += 2)
            {
                out += static_cast<char>(nibble(text[i]) << 4 | nibble(text[i + 1]));
            }
            return out;
        }

        std::string encode(const std::vector<Credential>& credentials)
        {
            std::string out;
            for (const Credential& credential : credentials)
            {
                out += hex(credential.url) + ":" + hex(credential.username) + ":" + hex(credential.password) + ";";
            }
            return out;
        }

        std::vector<Credential> decode(const std::string& text)
        {
            std::vector<Credential> credentials;
            std::size_t start = 0;
            while (start < text.size())
            {
                std::size_t end = text.find(';', start);
                if (end == std::string::npos)
                {
                    end = text.size();
                }
                const std::string entry = text.substr(start, end - start);
                const auto first = entry.find(':');
                const auto second = entry.find(':', first == std::string::npos ? 0 : first + 1);
                if (first != std::string::npos && second != std::string::npos)
                {
                    credentials.push_back(Credential{unhex(entry.substr(0, first)),
                                                     unhex(entry.substr(first + 1, second - first - 1)),
                                                     unhex(entry.substr(second + 1))});
                }
                start = end + 1;
            }
            return credentials;
        }

        // The url quoted after `marker` in `text` ("... for 'https://host': ...").
        std::optional<std::string> quoted_after(const std::string& text, const std::string& marker)
        {
            const auto at = text.find(marker);
            if (at == std::string::npos)
            {
                return std::nullopt;
            }
            const auto open = at + marker.size();
            const auto close = text.find('\'', open);
            if (close == std::string::npos)
            {
                return std::nullopt;
            }
            return text.substr(open, close - open);
        }
    } // namespace

    Output run(const std::string& workdir, const std::vector<std::string>& args, const std::string& input,
               const std::vector<Credential>& credentials, const OutputSink& on_output)
    {
        std::vector<std::string> command = {"git"};
        // A credential the user did not ask to remember keeps the helpers away from its host: they would store it.
        for (const Credential& credential : credentials)
        {
            if (!credential.remember)
            {
                command.insert(command.end(), {"-c", "credential." + credential.url + ".helper="});
            }
        }
        command.insert(command.end(), args.begin(), args.end());
        // Never block on a terminal prompt or a credential helper's window: git asks ImGit (its credentials
        // prompt), and the helpers still answer with what they have stored. GIT_ASKPASS is a bare path: git
        // executes it directly, not through a shell, so a quoted path (even one with a space) is "not found"
        // and every prompt fails with "could not read Username".
        const platform::ProcessOutput process =
                platform::run_process(workdir, command, input,
                                      {{"GIT_TERMINAL_PROMPT", "0"},
                                       {"GCM_INTERACTIVE", "never"},
                                       {"GIT_ASKPASS", platform::executable_path()},
                                       {ASKPASS_MARKER, "1"},
                                       {CREDENTIALS_VARIABLE, encode(credentials)}},
                                      on_output);
        Output output{process.exit_code, process.text};
        if (!process.started)
        {
            std::error_code error;
            output.text = std::filesystem::is_directory(workdir, error) ? "could not start git (is it on PATH?)"
                                                                        : "folder not found: " + workdir;
        }
        return output;
    }

    void feed_clone_progress(CloneProgress& progress, std::string_view chunk)
    {
        // git's phases in order, each with the share of the bar it fills ("remote: " prefixes the server's).
        struct Phase
        {
            std::string_view label;
            float from;
            float to;
        };
        static constexpr std::array<Phase, 7> PHASES = {{
                {"Enumerating objects", 0.f, 0.02f},
                {"Counting objects", 0.02f, 0.05f},
                {"Compressing objects", 0.05f, 0.1f},
                {"Receiving objects", 0.1f, 0.85f},
                {"Resolving deltas", 0.85f, 0.97f},
                {"Updating files", 0.97f, 1.f},
                {"Checking out files", 0.97f, 1.f},
        }};
        const auto line = [&](std::string_view text)
        {
            if (text.starts_with("remote: "))
            {
                text.remove_prefix(8);
            }
            while (!text.empty() && text.back() == ' ')
            {
                text.remove_suffix(1);
            }
            if (text.empty())
            {
                return;
            }
            for (const Phase& phase : PHASES)
            {
                if (!text.starts_with(phase.label))
                {
                    continue;
                }
                // "<label>: 45% (123/273)..." or "<label>: 273, done." (no percentage: the phase is over).
                float within = 1.f;
                if (const auto percent = text.find('%'); percent != std::string_view::npos)
                {
                    int value = 0;
                    std::size_t digits = percent;
                    while (digits > 0 && text[digits - 1] >= '0' && text[digits - 1] <= '9')
                    {
                        --digits;
                    }
                    for (std::size_t i = digits; i < percent; ++i)
                    {
                        value = value * 10 + (text[i] - '0');
                    }
                    within = static_cast<float>(std::min(value, 100)) / 100.f;
                }
                progress.fraction = std::max(progress.fraction, phase.from + (phase.to - phase.from) * within);
                progress.line = std::string(text);
                return;
            }
            if (text.starts_with("Cloning into"))
            {
                progress.line = std::string(text);
            }
        };
        progress.pending.append(chunk);
        std::size_t start = 0;
        while (true)
        {
            const std::size_t end = progress.pending.find_first_of("\r\n", start);
            if (end == std::string::npos)
            {
                break;
            }
            line(std::string_view(progress.pending).substr(start, end - start));
            start = end + 1;
        }
        progress.pending.erase(0, start);
    }

    bool available()
    {
        static const bool found = run(".", {"--version"}).exit_code == 0;
        return found;
    }

    std::string credential_host(const std::string& url, std::string* username)
    {
        const auto scheme = url.find("://");
        if (scheme == std::string::npos)
        {
            return {};
        }
        std::string authority = url.substr(scheme + 3);
        authority = authority.substr(0, authority.find('/'));
        if (const auto at = authority.rfind('@'); at != std::string::npos)
        {
            if (username != nullptr)
            {
                *username = authority.substr(0, std::min(authority.find(':'), at));
            }
            authority = authority.substr(at + 1);
        }
        return url.substr(0, scheme + 3) + authority;
    }

    std::optional<AuthRequest> auth_request(const std::string& output)
    {
        // git's messages: "could not read Username for '<url>': terminal prompts disabled" when nobody answered,
        // "Authentication failed for '<url>'" when the answer was refused.
        std::optional<std::string> url = quoted_after(output, "Authentication failed for '");
        const bool rejected = url.has_value();
        for (const char* marker : {"could not read Username for '", "could not read Password for '"})
        {
            if (!url)
            {
                url = quoted_after(output, marker);
            }
        }
        if (!url)
        {
            return std::nullopt;
        }
        AuthRequest request;
        request.url = credential_host(*url, &request.username);
        request.rejected = rejected;
        if (request.url.empty())
        {
            return std::nullopt;
        }
        return request;
    }

    bool is_askpass_invocation()
    {
        return std::getenv(ASKPASS_MARKER) != nullptr;
    }

    int askpass_main(const std::string& prompt)
    {
        const bool wants_username = prompt.starts_with("Username");
        if (!wants_username && !prompt.starts_with("Password"))
        {
            return 1; // ssh's host key / passphrase questions are not ours
        }
        const std::optional<std::string> url = quoted_after(prompt, "for '");
        if (!url)
        {
            return 1;
        }
        // The password prompt names the username git settled on ("Password for 'user@host'"), from the remote
        // url or our own answer; the host's credential answers it either way, since a remote url that carries
        // a user (https://me@github.com/...) never asks for one, whatever the sign-in bar was given.
        const std::string host = credential_host(*url);
        const char* encoded = std::getenv(CREDENTIALS_VARIABLE);
        for (const Credential& credential : decode(encoded != nullptr ? encoded : ""))
        {
            if (credential.url != host)
            {
                continue;
            }
            const std::string answer = (wants_username ? credential.username : credential.password) + "\n";
            std::fwrite(answer.data(), 1, answer.size(), stdout);
            std::fflush(stdout);
            return 0;
        }
        return 1;
    }
} // namespace imgit::git::cli
