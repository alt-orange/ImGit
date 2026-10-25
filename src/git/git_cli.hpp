#pragma once

#include <optional>
#include <string>
#include <vector>

// Network operations go through the user's `git` executable so the configured credential helpers and SSH
// setup are honoured; libgit2 covers everything local.
namespace imgit::git::cli
{
    struct Output
    {
        int exit_code = -1;
        std::string text; // stdout + stderr, in order
    };

    // A username / password for one host (`scheme://host[:port]`), answered to git's prompts by ImGit itself
    // running as GIT_ASKPASS.
    struct Credential
    {
        std::string url;
        std::string username;
        std::string password;
        bool remember = false; // false: the configured credential helpers are skipped for the host (nothing stored)
    };

    // Runs `git <args>` in `workdir` without a console window and waits for it to finish; `input` is written to
    // its stdin (closed after it). Credential helpers never show their own UI; git's username / password prompts
    // are answered from `credentials`, and fail (see auth_request) for a host that has none.
    [[nodiscard]] Output run(const std::string& workdir, const std::vector<std::string>& args,
                             const std::string& input = {}, const std::vector<Credential>& credentials = {});

    [[nodiscard]] bool available();

    // What a failed command's output says about credentials: the host that wanted them, the username in its
    // url, and whether the ones it was given were rejected.
    struct AuthRequest
    {
        std::string url;
        std::string username;
        bool rejected = false;
    };
    [[nodiscard]] std::optional<AuthRequest> auth_request(const std::string& output);

    // `scheme://host[:port]` of a url (and its `user@`, when it has one); empty for scp-style ssh urls.
    [[nodiscard]] std::string credential_host(const std::string& url, std::string* username = nullptr);

    // ImGit started by git as its askpass program: run() marks the environment, and `prompt` is git's
    // "Username for '...': " / "Password for '...': ".
    [[nodiscard]] bool is_askpass_invocation();
    // Prints the answer to `prompt` for git; returns the process exit code (1: no answer).
    int askpass_main(const std::string& prompt);
} // namespace imgit::git::cli
