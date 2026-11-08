#include "app/connections.hpp"
#include "platform/process.hpp"
#include <algorithm>
#include <chrono>
#include <thread>

// The process half of connections.hpp: the HTTPS / SSH checks and the list they fill.
namespace imgit::app
{
    namespace
    {
        bool ready(const std::future<Connection::Check>& job)
        {
            return job.valid() && job.wait_for(std::chrono::seconds(0)) == std::future_status::ready;
        }

        // The session credential for `host` (its url is scheme://host[:port]), or nullptr.
        const git::cli::Credential* session_credential(const std::vector<git::cli::Credential>& credentials,
                                                       const std::string& host)
        {
            for (const git::cli::Credential& credential : credentials)
            {
                const auto parsed = parse_remote_url(credential.url + "/");
                if (parsed && parsed->host == host)
                {
                    return &credential;
                }
            }
            return nullptr;
        }
    } // namespace

    Connection::Check check_https(const std::string& host)
    {
        // `git credential fill` asks the configured helpers (never interactively: git::cli::run turns their
        // prompts off) and, when none has the host, ImGit's askpass, which has nothing for it and fails.
        const git::cli::Output output =
                git::cli::run("", {"credential", "fill"}, "protocol=https\nhost=" + host + "\n\n");
        Connection::Check check;
        const std::string user = output.exit_code == 0 ? credential_username(output.text) : std::string();
        if (!user.empty())
        {
            check.state = Connection::State::CONNECTED;
            check.user = user;
            check.detail = "Stored by your Git credential helper";
        }
        else
        {
            check.state = Connection::State::NOT_CONNECTED;
            check.detail = "No stored username or token";
        }
        return check;
    }

    Connection::Check check_ssh(const std::string& target, const std::string& port)
    {
        // BatchMode: no passphrase or host-key prompt, so an unknown host or a locked key reports instead of waiting.
        std::vector<std::string> args = {"ssh", "-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=6"};
        if (!port.empty())
        {
            args.insert(args.end(), {"-p", port});
        }
        args.push_back(target);
        const platform::ProcessOutput output = platform::run_process("", args);
        Connection::Check check;
        if (!output.started)
        {
            check.state = Connection::State::NOT_CONNECTED;
            check.detail = "Could not start ssh (is OpenSSH on PATH?)";
            return check;
        }
        const SshAnswer answer = parse_ssh_answer(output.text, output.exit_code);
        check.state = answer.authenticated ? Connection::State::CONNECTED : Connection::State::NOT_CONNECTED;
        check.user = answer.user;
        check.detail = answer.authenticated ? "Your SSH key is accepted" : answer.detail;
        return check;
    }

    void forget_stored_credential(const std::string& host, const std::string& username)
    {
        (void)git::cli::run("", {"credential", "reject"},
                            "protocol=https\nhost=" + host + "\nusername=" + username + "\n\n");
    }

    Connections::~Connections()
    {
        if (checking())
        {
            std::thread([https = std::move(m_https_jobs), ssh = std::move(m_ssh_jobs)] {}).detach();
        }
    }

    void Connections::update_hosts(const std::vector<std::pair<std::string, std::string>>& remotes)
    {
        std::vector<Connection> list;
        for (const KnownService& service : known_services())
        {
            Connection connection;
            connection.name = service.name;
            connection.host = service.host;
            connection.ssh_target = std::string("git@") + service.ssh_host;
            list.push_back(std::move(connection));
        }
        for (const auto& [repo, url] : remotes)
        {
            const auto endpoint = parse_remote_url(url);
            if (!endpoint)
            {
                continue;
            }
            const KnownService* service = service_of(endpoint->host);
            const std::string host = service != nullptr ? service->host : endpoint->host;
            auto it = std::find_if(list.begin(), list.end(),
                                   [&](const Connection& c)
                                   {
                                       return c.host == host;
                                   });
            if (it == list.end())
            {
                Connection connection;
                connection.name = host;
                connection.host = host;
                connection.https = false;
                connection.ssh = false;
                list.push_back(std::move(connection));
                it = list.end() - 1;
            }
            if (service == nullptr)
            {
                if (endpoint->transport == RemoteEndpoint::Transport::HTTPS)
                {
                    it->https = true;
                }
                else
                {
                    it->ssh = true;
                    it->ssh_target = (endpoint->user.empty() ? std::string("git") : endpoint->user) + "@" + host;
                    it->ssh_port = endpoint->port;
                }
            }
            if (std::find(it->repos.begin(), it->repos.end(), repo) == it->repos.end())
            {
                it->repos.push_back(repo);
            }
        }
        // The checks so far carry over (an SSH check only while it is for the same target).
        for (Connection& connection : list)
        {
            if (const Connection* old = find(connection.host); old != nullptr)
            {
                connection.https_check = old->https_check;
                connection.session_credential = old->session_credential;
                if (old->ssh_target == connection.ssh_target && old->ssh_port == connection.ssh_port)
                {
                    connection.ssh_check = old->ssh_check;
                }
            }
        }
        m_list = std::move(list);
    }

    void Connections::check(const std::vector<git::cli::Credential>& session_credentials, bool force)
    {
        for (Connection& connection : m_list)
        {
            if (connection.https)
            {
                if (const git::cli::Credential* credential = session_credential(session_credentials, connection.host))
                {
                    connection.session_credential = true;
                    connection.https_check = {Connection::State::CONNECTED, credential->username,
                                              credential->remember ? "Entered this session and remembered"
                                                                   : "Entered this session"};
                }
                else
                {
                    if (connection.session_credential)
                    {
                        // Forgotten: look again for a stored one.
                        connection.session_credential = false;
                        connection.https_check = {};
                    }
                    if ((force || connection.https_check.state == Connection::State::UNCHECKED)
                        && !m_https_jobs.contains(connection.host))
                    {
                        connection.https_check = {Connection::State::CHECKING, {}, {}};
                        m_https_jobs[connection.host] = std::async(std::launch::async, check_https, connection.host);
                    }
                }
            }
            if (connection.ssh && !connection.ssh_target.empty()
                && (force || connection.ssh_check.state == Connection::State::UNCHECKED)
                && !m_ssh_jobs.contains(connection.host))
            {
                connection.ssh_check = {Connection::State::CHECKING, {}, {}};
                m_ssh_jobs[connection.host] =
                        std::async(std::launch::async, check_ssh, connection.ssh_target, connection.ssh_port);
            }
        }
    }

    void Connections::poll()
    {
        const auto take = [this](std::map<std::string, std::future<Connection::Check>>& jobs, bool https)
        {
            for (auto it = jobs.begin(); it != jobs.end();)
            {
                if (!ready(it->second))
                {
                    ++it;
                    continue;
                }
                Connection::Check result = it->second.get();
                if (Connection* connection = find(it->first); connection != nullptr)
                {
                    if (!https)
                    {
                        connection->ssh_check = std::move(result);
                    }
                    else if (!connection->session_credential)
                    {
                        connection->https_check = std::move(result);
                    }
                }
                it = jobs.erase(it);
            }
        };
        take(m_https_jobs, true);
        take(m_ssh_jobs, false);
    }

    bool Connections::checking() const
    {
        return !m_https_jobs.empty() || !m_ssh_jobs.empty();
    }

    Connection* Connections::find(const std::string& host)
    {
        const auto it = std::find_if(m_list.begin(), m_list.end(),
                                     [&](const Connection& c)
                                     {
                                         return c.host == host;
                                     });
        return it == m_list.end() ? nullptr : &*it;
    }
} // namespace imgit::app
