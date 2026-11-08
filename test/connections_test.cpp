#include "app/connections.hpp"
#include <gtest/gtest.h>

namespace
{
    using imgit::app::credential_username;
    using imgit::app::parse_remote_url;
    using imgit::app::parse_ssh_answer;
    using imgit::app::RemoteEndpoint;
    using imgit::app::service_of;

    TEST(RemoteUrl, Https)
    {
        const auto e = parse_remote_url("https://me@GitHub.com:8443/owner/repo.git");
        ASSERT_TRUE(e);
        EXPECT_EQ(e->transport, RemoteEndpoint::Transport::HTTPS);
        EXPECT_EQ(e->host, "github.com");
        EXPECT_EQ(e->user, "me");
        EXPECT_EQ(e->port, "8443");
    }

    TEST(RemoteUrl, Ssh)
    {
        const auto scp = parse_remote_url("git@gitlab.com:group/repo.git");
        ASSERT_TRUE(scp);
        EXPECT_EQ(scp->transport, RemoteEndpoint::Transport::SSH);
        EXPECT_EQ(scp->host, "gitlab.com");
        EXPECT_EQ(scp->user, "git");

        const auto url = parse_remote_url("ssh://git@ssh.github.com:443/owner/repo.git");
        ASSERT_TRUE(url);
        EXPECT_EQ(url->transport, RemoteEndpoint::Transport::SSH);
        EXPECT_EQ(url->host, "ssh.github.com");
        EXPECT_EQ(url->port, "443");

        const auto bare = parse_remote_url("myserver:repos/x.git");
        ASSERT_TRUE(bare);
        EXPECT_EQ(bare->host, "myserver");
        EXPECT_EQ(bare->user, "");
    }

    TEST(RemoteUrl, Local)
    {
        EXPECT_FALSE(parse_remote_url("C:\\repos\\x"));
        EXPECT_FALSE(parse_remote_url("C:/repos/x"));
        EXPECT_FALSE(parse_remote_url("/srv/git/x.git"));
        EXPECT_FALSE(parse_remote_url("../x.git"));
        EXPECT_FALSE(parse_remote_url("file:///srv/git/x.git"));
    }

    TEST(Services, Hosts)
    {
        ASSERT_NE(service_of("github.com"), nullptr);
        EXPECT_STREQ(service_of("ssh.github.com")->name, "GitHub");
        EXPECT_STREQ(service_of("ssh.dev.azure.com")->name, "Azure DevOps");
        EXPECT_STREQ(service_of("myorg.visualstudio.com")->name, "Azure DevOps");
        EXPECT_EQ(service_of("notgithub.com"), nullptr);
        EXPECT_EQ(service_of("git.example.com"), nullptr);
    }

    TEST(SshAnswer, Greetings)
    {
        auto a = parse_ssh_answer("Warning: Permanently added 'github.com' to the list of known hosts.\r\n"
                                  "Hi octo-cat! You've successfully authenticated, but GitHub does not provide shell "
                                  "access.\r\n",
                                  1);
        EXPECT_TRUE(a.authenticated);
        EXPECT_EQ(a.user, "octo-cat");

        a = parse_ssh_answer("Welcome to GitLab, @someone!\n", 0);
        EXPECT_TRUE(a.authenticated);
        EXPECT_EQ(a.user, "someone");

        a = parse_ssh_answer("Hi there, gitea_user! You've successfully authenticated with the key named k\n", 1);
        EXPECT_TRUE(a.authenticated);
        EXPECT_EQ(a.user, "gitea_user");

        a = parse_ssh_answer("logged in as bb-user.\n\nYou can use git or hg to connect to Bitbucket.\n", 0);
        EXPECT_TRUE(a.authenticated);
        EXPECT_EQ(a.user, "bb-user");

        a = parse_ssh_answer("remote: Shell access is not supported.\n", 1);
        EXPECT_TRUE(a.authenticated);
        EXPECT_EQ(a.user, "");
    }

    TEST(SshAnswer, Failures)
    {
        auto a = parse_ssh_answer("git@github.com: Permission denied (publickey).\r\n", 255);
        EXPECT_FALSE(a.authenticated);
        EXPECT_EQ(a.detail, "The server refused your SSH key");

        a = parse_ssh_answer("Host key verification failed.\n", 255);
        EXPECT_FALSE(a.authenticated);

        a = parse_ssh_answer("something odd\nkex_exchange_identification: read: Connection reset\n", 255);
        EXPECT_FALSE(a.authenticated);
        EXPECT_EQ(a.detail, "kex_exchange_identification: read: Connection reset");
    }

    TEST(Credential, Username)
    {
        EXPECT_EQ(credential_username("protocol=https\r\nhost=github.com\r\nusername=alice\r\npassword=x\r\n"),
                  "alice");
        EXPECT_EQ(credential_username("protocol=https\nhost=github.com\n"), "");
    }
} // namespace
