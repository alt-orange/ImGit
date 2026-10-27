#pragma once

#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
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

    // Called from the running thread with each chunk of git's output as it arrives.
    using OutputSink = std::function<void(std::string_view)>;

    // Runs `git <args>` in `workdir` without a console window and waits for it to finish; `input` is written to
    // its stdin (closed after it). Credential helpers never show their own UI; git's username / password prompts
    // are answered from `credentials`, and fail (see auth_request) for a host that has none.
    [[nodiscard]] Output run(const std::string& workdir, const std::vector<std::string>& args,
                             const std::string& input = {}, const std::vector<Credential>& credentials = {},
                             const OutputSink& on_output = {});

    // Where a `git clone --progress` is: its latest phase line ("Receiving objects:  45% (123/273), 1.2 MiB | ...")
    // and how far along the whole clone is, 0..1 across the phases (counting, compressing, receiving objects,
    // resolving deltas, updating files).
    struct CloneProgress
    {
        std::string line;
        float fraction = 0.f;
        std::string pending; // the output after the last line break, waiting for the rest of its line
    };
    // Feeds a chunk of the clone's output (split anywhere) into `progress`.
    void feed_clone_progress(CloneProgress& progress, std::string_view chunk);

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

    // The credentials run() hands its child through the environment, for the browser's in-process git
    // (web_git.cpp), which answers the prompts itself.
    [[nodiscard]] std::vector<Credential>
    credentials_from_environment(const std::vector<std::pair<std::string, std::string>>& environment);

    // ImGit started by git as its askpass program: run() marks the environment, and `prompt` is git's
    // "Username for '...': " / "Password for '...': ".
    [[nodiscard]] bool is_askpass_invocation();
    // Prints the answer to `prompt` for git; returns the process exit code (1: no answer).
    int askpass_main(const std::string& prompt);
} // namespace imgit::git::cli
