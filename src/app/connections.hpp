#pragma once

#include "git/git_cli.hpp"
#include <future>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

// Preferences → Connections: which hosting services this machine can reach and as whom. ImGit has no accounts of
// its own, so a connection is what git itself would use: an HTTPS username / token (entered this session, or kept
// by the configured credential helper) and an SSH key the service accepts.
namespace imgit::app
{
    // A remote url split into what a connection check needs.
    struct RemoteEndpoint
    {
        enum class Transport
        {
            HTTPS, // http:// or https://
            SSH, // ssh://user@host[:port]/path or scp-style user@host:path
        };
        Transport transport = Transport::HTTPS;
        std::string host; // lowercase, without the port
        std::string user; // the ssh user (or the https url's user), "" when the url has none
        std::string port; // "" for the default
    };
    // nullopt for local paths, file:// and git:// urls.
    [[nodiscard]] std::optional<RemoteEndpoint> parse_remote_url(std::string_view url);

    // A hosting service ImGit always lists, whether or not a repository uses it.
    struct KnownService
    {
        const char* name;
        const char* host; // its HTTPS host (the credential helper's key)
        const char* ssh_host; // where its SSH endpoint answers
    };
    [[nodiscard]] const std::vector<KnownService>& known_services();
    // The known service `host` belongs to (its HTTPS or SSH host, or a subdomain such as *.visualstudio.com).
    [[nodiscard]] const KnownService* service_of(std::string_view host);

    // What `ssh -T` printed and returned: authenticated (and as whom, when the service says), refused, or failed.
    struct SshAnswer
    {
        bool authenticated = false;
        std::string user;
        std::string detail; // why not, in a line
    };
    [[nodiscard]] SshAnswer parse_ssh_answer(std::string_view output, int exit_code);

    // The `username=` of `git credential fill`'s answer ("" when it has none). The password in it is ignored.
    [[nodiscard]] std::string credential_username(std::string_view fill_output);

    struct Connection
    {
        enum class State
        {
            UNCHECKED,
            CHECKING,
            CONNECTED,
            NOT_CONNECTED,
        };
        struct Check
        {
            State state = State::UNCHECKED;
            std::string user;
            std::string detail;
        };

        std::string name; // "GitHub", or the host for other servers
        std::string host; // the HTTPS host
        bool https = true; // listed for HTTPS (known services, hosts an https remote uses)
        bool ssh = true; // listed for SSH (known services, hosts an ssh remote uses)
        std::string ssh_target; // user@host
        std::string ssh_port;
        bool session_credential = false; // the HTTPS user came from the credentials entered this session
        Check https_check;
        Check ssh_check;
        std::vector<std::string> repos; // the open repositories with a remote here
    };

    // The connections the page lists and their checks, which run in the background.
    class Connections
    {
    public:
        Connections() = default;
        // Checks still running finish on a detached thread, so closing ImGit never waits on ssh.
        ~Connections();
        Connections(const Connections&) = delete;
        Connections& operator=(const Connections&) = delete;

        // The known services, then the other hosts the open repositories' remotes point at.
        // `remotes`: (repository name, remote url) pairs.
        void update_hosts(const std::vector<std::pair<std::string, std::string>>& remotes);
        // Starts the checks that have not run (`force`: every check again).
        void check(const std::vector<git::cli::Credential>& session_credentials, bool force);
        // Takes the finished checks' results.
        void poll();
        [[nodiscard]] bool checking() const;

        [[nodiscard]] const std::vector<Connection>& list() const noexcept
        {
            return m_list;
        }
        Connection* find(const std::string& host);

    private:
        std::vector<Connection> m_list;
        std::map<std::string, std::future<Connection::Check>> m_https_jobs; // by host
        std::map<std::string, std::future<Connection::Check>> m_ssh_jobs;
    };

    // The checks themselves (they run git / ssh and block).
    [[nodiscard]] Connection::Check check_https(const std::string& host);
    [[nodiscard]] Connection::Check check_ssh(const std::string& target, const std::string& port);
    // Removes `host`'s HTTPS credential from the credential helper (`git credential reject`).
    void forget_stored_credential(const std::string& host, const std::string& username);
} // namespace imgit::app
