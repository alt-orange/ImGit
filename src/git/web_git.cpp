// The browser's `git`: a page cannot start programs, so the commands ImGit (and its terminal panel) hand to the git
// executable run here instead, on libgit2. Remotes are reached over HTTP(S) with the browser's fetch, through the
// CORS proxy the page names (res/web/shell.html, tools/serve_web.py), since git hosts do not allow other origins.
// Registered as the program `git` for platform::run_process (platform/programs.hpp); browser build only.
//
// The output imitates git's where ImGit reads it: the clone progress lines (cli::feed_clone_progress), the
// credential failures (cli::auth_request) and the rejected push (Application::offer_force_push).
#include "git/git_cli.hpp"
#include "platform/programs.hpp"
#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <cstring>
#include <emscripten/emscripten.h>
#include <emscripten/fetch.h>
#include <filesystem>
#include <fstream>
#include <git2.h>
#include <git2/sys/credential.h>
#include <git2/sys/email.h>
#include <git2/sys/errors.h>
#include <git2/sys/transport.h>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <sstream>

// The CORS proxy the page asks for (read on the main thread, where the page's Module is): `?git-proxy=<prefix>` in
// the page's address, else Module.imgitGitProxy, else this server's /git-proxy/ (tools/serve_web.py). Empty: no
// proxy, for hosts that allow the page's origin themselves.
EM_JS_DEPS(imgit_web_git, "$stringToNewUTF8");
EM_JS(char*, imgit_git_proxy, (), {
    var proxy = Module.imgitGitProxy;
    try {
        var query = new URLSearchParams(location.search).get('git-proxy');
        if (query !== null) {
            proxy = query;
        }
    } catch (error) {
    }
    if (proxy === undefined) {
        proxy = location.origin + '/git-proxy/';
    }
    return stringToNewUTF8(proxy);
});

namespace imgit::git::web
{
    namespace
    {
        using platform::OutputSink;

        std::string g_proxy;

        // ------------------------------------------------------------------------------------------------------
        // libgit2 handles

        template<class T, void (*Free)(T*)>
        struct Handle
        {
            T* ptr = nullptr;

            Handle() = default;
            Handle(const Handle&) = delete;
            Handle& operator=(const Handle&) = delete;
            ~Handle()
            {
                if (ptr != nullptr)
                {
                    Free(ptr);
                }
            }
            T** out()
            {
                return &ptr;
            }
            T* get() const
            {
                return ptr;
            }
        };

        using RepoHandle = Handle<git_repository, git_repository_free>;
        using RemoteHandle = Handle<git_remote, git_remote_free>;
        using RefHandle = Handle<git_reference, git_reference_free>;
        using CommitHandle = Handle<git_commit, git_commit_free>;
        using ObjectHandle = Handle<git_object, git_object_free>;
        using TreeHandle = Handle<git_tree, git_tree_free>;
        using IndexHandle = Handle<git_index, git_index_free>;
        using DiffHandle = Handle<git_diff, git_diff_free>;
        using AnnotatedHandle = Handle<git_annotated_commit, git_annotated_commit_free>;
        using SignatureHandle = Handle<git_signature, git_signature_free>;
        using WalkHandle = Handle<git_revwalk, git_revwalk_free>;
        using StatusHandle = Handle<git_status_list, git_status_list_free>;
        using RebaseHandle = Handle<git_rebase, git_rebase_free>;
        using ConfigHandle = Handle<git_config, git_config_free>;

        struct Buf
        {
            git_buf buf = GIT_BUF_INIT;
            Buf() = default;
            Buf(const Buf&) = delete;
            Buf& operator=(const Buf&) = delete;
            ~Buf()
            {
                git_buf_dispose(&buf);
            }
        };

        struct StrArray
        {
            git_strarray array{};
            StrArray() = default;
            StrArray(const StrArray&) = delete;
            StrArray& operator=(const StrArray&) = delete;
            ~StrArray()
            {
                git_strarray_dispose(&array);
            }
        };

        // A git_strarray over `strings` (which must outlive it).
        struct Paths
        {
            std::vector<char*> pointers;
            git_strarray array{};

            explicit Paths(const std::vector<std::string>& strings)
            {
                for (const std::string& text : strings)
                {
                    pointers.push_back(const_cast<char*>(text.c_str()));
                }
                array.strings = pointers.data();
                array.count = pointers.size();
            }
        };

        std::string last_error(const char* fallback)
        {
            const git_error* error = git_error_last();
            return error != nullptr && error->message != nullptr && *error->message != '\0' ? error->message
                                                                                              : fallback;
        }

        std::string oid_text(const git_oid* oid)
        {
            char text[GIT_OID_MAX_HEXSIZE + 1] = {};
            git_oid_tostr(text, sizeof(text), oid);
            return text;
        }

        std::string short_oid(const git_oid* oid)
        {
            return oid_text(oid).substr(0, 7);
        }

        // ------------------------------------------------------------------------------------------------------
        // The smart HTTP transport over the browser's fetch (synchronous, so on a worker thread only).

        std::string base64(const std::string& text)
        {
            static constexpr char ALPHABET[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            std::string out;
            std::size_t i = 0;
            for (; i + 2 < text.size(); i += 3)
            {
                const unsigned value = static_cast<unsigned char>(text[i]) << 16
                                       | static_cast<unsigned char>(text[i + 1]) << 8
                                       | static_cast<unsigned char>(text[i + 2]);
                out += ALPHABET[value >> 18 & 63];
                out += ALPHABET[value >> 12 & 63];
                out += ALPHABET[value >> 6 & 63];
                out += ALPHABET[value & 63];
            }
            if (i < text.size())
            {
                unsigned value = static_cast<unsigned char>(text[i]) << 16;
                if (i + 1 < text.size())
                {
                    value |= static_cast<unsigned char>(text[i + 1]) << 8;
                }
                out += ALPHABET[value >> 18 & 63];
                out += ALPHABET[value >> 12 & 63];
                out += i + 1 < text.size() ? ALPHABET[value >> 6 & 63] : '=';
                out += '=';
            }
            return out;
        }

        struct HttpAnswer
        {
            int status = 0; // 0: no answer (offline, refused by the browser, no proxy)
            std::string body;
        };

        HttpAnswer http_request(const std::string& url, bool post, const std::string& body,
                                const std::string& content_type, const std::string& accept,
                                const std::string& authorization)
        {
            emscripten_fetch_attr_t attributes;
            emscripten_fetch_attr_init(&attributes);
            std::strcpy(attributes.requestMethod, post ? "POST" : "GET");
            attributes.attributes = EMSCRIPTEN_FETCH_LOAD_TO_MEMORY | EMSCRIPTEN_FETCH_SYNCHRONOUS;
            std::vector<const char*> headers;
            if (!content_type.empty())
            {
                headers.insert(headers.end(), {"Content-Type", content_type.c_str()});
            }
            if (!accept.empty())
            {
                headers.insert(headers.end(), {"Accept", accept.c_str()});
            }
            if (!authorization.empty())
            {
                headers.insert(headers.end(), {"Authorization", authorization.c_str()});
            }
            headers.push_back(nullptr);
            attributes.requestHeaders = headers.data();
            if (post)
            {
                attributes.requestData = body.data();
                attributes.requestDataSize = body.size();
            }
            const std::string target = g_proxy + url;
            emscripten_fetch_t* fetch = emscripten_fetch(&attributes, target.c_str());
            if (fetch == nullptr)
            {
                return {};
            }
            HttpAnswer answer;
            answer.status = fetch->status;
            if (fetch->data != nullptr && fetch->numBytes > 0)
            {
                answer.body.assign(fetch->data, static_cast<std::size_t>(fetch->numBytes));
            }
            emscripten_fetch_close(fetch);
            return answer;
        }

        struct Subtransport
        {
            git_smart_subtransport parent; // first: libgit2 hands this pointer back
            git_transport* owner = nullptr;
            std::string authorization; // "Basic ..." once a credential was accepted, for the requests after it
        };

        struct Stream
        {
            git_smart_subtransport_stream parent; // first, likewise
            Subtransport* subtransport = nullptr;
            std::string url;
            bool post = false;
            std::string content_type;
            std::string accept;
            std::string request;
            std::string response;
            std::size_t offset = 0;
            bool sent = false;
        };

        // Sends the request (the first time the answer is read), asking the remote's credential callback when the
        // server wants one.
        int send(Stream& stream)
        {
            Subtransport& transport = *stream.subtransport;
            for (int attempt = 0;; ++attempt)
            {
                const HttpAnswer answer = http_request(stream.url, stream.post, stream.request, stream.content_type,
                                                       stream.accept, transport.authorization);
                if (answer.status == 401 && attempt < 3)
                {
                    git_credential* credential = nullptr;
                    if (git_transport_smart_credentials(&credential, transport.owner, nullptr,
                                                        GIT_CREDENTIAL_USERPASS_PLAINTEXT)
                                < 0
                        || credential == nullptr)
                    {
                        git_error_set_str(GIT_ERROR_NET, "authentication required");
                        return GIT_EAUTH;
                    }
                    const auto* plain = reinterpret_cast<const git_credential_userpass_plaintext*>(credential);
                    transport.authorization =
                            "Basic " + base64(std::string(plain->username) + ":" + std::string(plain->password));
                    git_credential_free(credential);
                    continue;
                }
                if (answer.status == 0)
                {
                    git_error_set_str(GIT_ERROR_NET, ("could not reach " + stream.url
                                                      + " (offline, or no CORS proxy answered: see docs/WEB.md)")
                                                             .c_str());
                    return -1;
                }
                if (answer.status == 401)
                {
                    git_error_set_str(GIT_ERROR_NET, "authentication failed");
                    return GIT_EAUTH;
                }
                if (answer.status != 200)
                {
                    git_error_set_str(GIT_ERROR_NET, ("unexpected HTTP status code: " + std::to_string(answer.status)
                                                      + " from " + stream.url)
                                                             .c_str());
                    return -1;
                }
                stream.response = answer.body;
                stream.sent = true;
                return 0;
            }
        }

        int stream_read(git_smart_subtransport_stream* base, char* buffer, size_t size, size_t* bytes_read)
        {
            Stream& stream = *reinterpret_cast<Stream*>(base);
            *bytes_read = 0;
            if (!stream.sent)
            {
                if (const int error = send(stream); error < 0)
                {
                    return error;
                }
            }
            const std::size_t count = std::min(size, stream.response.size() - stream.offset);
            std::memcpy(buffer, stream.response.data() + stream.offset, count);
            stream.offset += count;
            *bytes_read = count;
            return 0;
        }

        int stream_write(git_smart_subtransport_stream* base, const char* buffer, size_t length)
        {
            Stream& stream = *reinterpret_cast<Stream*>(base);
            stream.request.append(buffer, length);
            return 0;
        }

        void stream_free(git_smart_subtransport_stream* base)
        {
            delete reinterpret_cast<Stream*>(base);
        }

        int subtransport_action(git_smart_subtransport_stream** out, git_smart_subtransport* base, const char* url,
                                git_smart_service_t service)
        {
            auto stream = std::make_unique<Stream>();
            stream->parent.subtransport = base;
            stream->parent.read = stream_read;
            stream->parent.write = stream_write;
            stream->parent.free = stream_free;
            stream->subtransport = reinterpret_cast<Subtransport*>(base);
            std::string root = url;
            // A remote url with "user:password@" signs in with it (the request itself goes without).
            if (const auto scheme = root.find("://"); scheme != std::string::npos)
            {
                const auto host_end = root.find('/', scheme + 3);
                const auto at = root.rfind('@', host_end);
                if (at != std::string::npos && at > scheme)
                {
                    const std::string userinfo = root.substr(scheme + 3, at - scheme - 3);
                    if (userinfo.find(':') != std::string::npos && stream->subtransport->authorization.empty())
                    {
                        stream->subtransport->authorization = "Basic " + base64(userinfo);
                    }
                    root.erase(scheme + 3, at - scheme - 2);
                }
            }
            while (!root.empty() && root.back() == '/')
            {
                root.pop_back();
            }
            switch (service)
            {
            case GIT_SERVICE_UPLOADPACK_LS:
                stream->url = root + "/info/refs?service=git-upload-pack";
                break;
            case GIT_SERVICE_RECEIVEPACK_LS:
                stream->url = root + "/info/refs?service=git-receive-pack";
                break;
            case GIT_SERVICE_UPLOADPACK:
                stream->url = root + "/git-upload-pack";
                stream->post = true;
                stream->content_type = "application/x-git-upload-pack-request";
                stream->accept = "application/x-git-upload-pack-result";
                break;
            case GIT_SERVICE_RECEIVEPACK:
                stream->url = root + "/git-receive-pack";
                stream->post = true;
                stream->content_type = "application/x-git-receive-pack-request";
                stream->accept = "application/x-git-receive-pack-result";
                break;
            default:
                git_error_set_str(GIT_ERROR_NET, "unknown smart service");
                return -1;
            }
            *out = &stream.release()->parent;
            return 0;
        }

        int subtransport_close(git_smart_subtransport*)
        {
            return 0;
        }

        void subtransport_free(git_smart_subtransport* base)
        {
            delete reinterpret_cast<Subtransport*>(base);
        }

        int create_subtransport(git_smart_subtransport** out, git_transport* owner, void*)
        {
            auto* transport = new Subtransport();
            transport->parent.action = subtransport_action;
            transport->parent.close = subtransport_close;
            transport->parent.free = subtransport_free;
            transport->owner = owner;
            *out = &transport->parent;
            return 0;
        }

        int create_transport(git_transport** out, git_remote* owner, void*)
        {
            static git_smart_subtransport_definition definition = {create_subtransport, 1, nullptr};
            return git_transport_smart(out, owner, &definition);
        }

        // ------------------------------------------------------------------------------------------------------
        // One command's state: its output and the remote callbacks' bookkeeping.

        struct Command
        {
            std::string workdir;
            std::vector<std::string> args;
            std::string input;
            std::vector<cli::Credential> credentials;
            const OutputSink& out;

            // credentials: the host last asked about, and how the answer went
            std::string auth_host{};
            bool auth_missing = false;
            bool auth_offered = false;
            bool auth_rejected = false;
            // progress lines already printed
            int receiving = -1;
            int resolving = -1;
            int updating = -1;
            // push results
            std::vector<std::string> rejected{};

            void print(const std::string& text) const
            {
                out(text);
            }

            int fail(const std::string& message) const
            {
                out("fatal: " + message + "\n");
                return 128;
            }

            int fail_git(const char* fallback) const
            {
                return fail(last_error(fallback));
            }
        };

        int on_credentials(git_credential** out, const char* url, const char* username_from_url, unsigned int,
                           void* payload)
        {
            Command& command = *static_cast<Command*>(payload);
            const std::string host = cli::credential_host(url != nullptr ? url : "");
            command.auth_host = host;
            if (command.auth_offered)
            {
                command.auth_rejected = true; // the server turned the one we gave down
                return GIT_EUSER;
            }
            for (const cli::Credential& credential : command.credentials)
            {
                if (credential.url == host)
                {
                    command.auth_offered = true;
                    const std::string username = !credential.username.empty() ? credential.username
                                                 : username_from_url != nullptr ? username_from_url
                                                                                : "";
                    return git_credential_userpass_plaintext_new(out, username.c_str(), credential.password.c_str());
                }
            }
            command.auth_missing = true;
            return GIT_EUSER;
        }

        int on_sideband(const char* text, int length, void* payload)
        {
            const Command& command = *static_cast<Command*>(payload);
            command.print("remote: " + std::string(text, static_cast<std::size_t>(length)));
            return 0;
        }

        int percent(std::size_t done, std::size_t total)
        {
            return total == 0 ? 100 : static_cast<int>(done * 100 / total);
        }

        int on_transfer(const git_indexer_progress* progress, void* payload)
        {
            Command& command = *static_cast<Command*>(payload);
            if (progress->total_objects > 0)
            {
                const int now = percent(progress->received_objects, progress->total_objects);
                if (now != command.receiving)
                {
                    command.receiving = now;
                    const bool done = progress->received_objects == progress->total_objects;
                    command.print("Receiving objects: " + std::to_string(now) + "% ("
                                  + std::to_string(progress->received_objects) + "/"
                                  + std::to_string(progress->total_objects) + ")" + (done ? ", done.\n" : "\r"));
                }
            }
            if (progress->total_deltas > 0)
            {
                const int now = percent(progress->indexed_deltas, progress->total_deltas);
                if (now != command.resolving)
                {
                    command.resolving = now;
                    const bool done = progress->indexed_deltas == progress->total_deltas;
                    command.print("Resolving deltas: " + std::to_string(now) + "% ("
                                  + std::to_string(progress->indexed_deltas) + "/"
                                  + std::to_string(progress->total_deltas) + ")" + (done ? ", done.\n" : "\r"));
                }
            }
            return 0;
        }

        void on_checkout_progress(const char*, size_t completed, size_t total, void* payload)
        {
            Command& command = *static_cast<Command*>(payload);
            const int now = percent(completed, total);
            if (total > 0 && now != command.updating)
            {
                command.updating = now;
                command.print("Updating files: " + std::to_string(now) + "% (" + std::to_string(completed) + "/"
                              + std::to_string(total) + ")" + (completed == total ? ", done.\n" : "\r"));
            }
        }

        int on_update_tips(const char* refname, const git_oid* from, const git_oid* to, void* payload)
        {
            const Command& command = *static_cast<Command*>(payload);
            std::string name = refname;
            for (const char* prefix : {"refs/remotes/", "refs/heads/", "refs/tags/"})
            {
                if (name.starts_with(prefix))
                {
                    name.erase(0, std::strlen(prefix));
                }
            }
            if (git_oid_is_zero(from))
            {
                command.print(" * [new ref]         " + name + "\n");
            }
            else if (git_oid_is_zero(to))
            {
                command.print(" - [deleted]         " + name + "\n");
            }
            else
            {
                command.print("   " + short_oid(from) + ".." + short_oid(to) + "  " + name + "\n");
            }
            return 0;
        }

        int on_push_update(const char* refname, const char* status, void* payload)
        {
            Command& command = *static_cast<Command*>(payload);
            if (status != nullptr)
            {
                command.rejected.push_back(refname);
                command.print(std::string(" ! [rejected]        ") + refname + " (" + status + ")\n");
            }
            else
            {
                command.print(std::string("   ") + refname + "\n");
            }
            return 0;
        }

        git_remote_callbacks callbacks(Command& command)
        {
            git_remote_callbacks result;
            git_remote_init_callbacks(&result, GIT_REMOTE_CALLBACKS_VERSION);
            result.credentials = on_credentials;
            result.sideband_progress = on_sideband;
            result.transfer_progress = on_transfer;
            result.update_tips = on_update_tips;
            result.push_update_reference = on_push_update;
            result.payload = &command;
            return result;
        }

        // A failed network command's message, in git's words where ImGit reads them.
        int network_failure(Command& command, const std::string& url, const char* fallback)
        {
            if (command.auth_missing)
            {
                return command.fail("could not read Username for '" + command.auth_host
                                    + "': terminal prompts disabled");
            }
            if (command.auth_rejected)
            {
                command.print("remote: Invalid username or password.\n");
                return command.fail("Authentication failed for '" + url + "'");
            }
            return command.fail_git(fallback);
        }

        // ------------------------------------------------------------------------------------------------------
        // Helpers on an open repository

        std::string absolute(const std::string& workdir, const std::string& path)
        {
            const std::filesystem::path given(path);
            return given.is_absolute() ? path : (std::filesystem::path(workdir) / given).lexically_normal().string();
        }

        bool open_repository(Command& command, RepoHandle& repo)
        {
            if (git_repository_open_ext(repo.out(), command.workdir.c_str(), 0, nullptr) < 0)
            {
                command.fail("not a git repository (or any of the parent directories): .git");
                return false;
            }
            return true;
        }

        std::optional<std::string> current_branch(git_repository* repo)
        {
            RefHandle head;
            if (git_repository_head(head.out(), repo) < 0 || !git_reference_is_branch(head.get()))
            {
                return std::nullopt;
            }
            return std::string(git_reference_shorthand(head.get()));
        }

        // The branch's upstream as remote and remote-side branch name ("origin", "main").
        std::optional<std::pair<std::string, std::string>> upstream_of(git_repository* repo, const std::string& branch)
        {
            ConfigHandle config;
            if (git_repository_config_snapshot(config.out(), repo) < 0)
            {
                return std::nullopt;
            }
            const char* remote = nullptr;
            const char* merge = nullptr;
            if (git_config_get_string(&remote, config.get(), ("branch." + branch + ".remote").c_str()) < 0
                || git_config_get_string(&merge, config.get(), ("branch." + branch + ".merge").c_str()) < 0)
            {
                return std::nullopt;
            }
            std::string target = merge;
            if (target.starts_with("refs/heads/"))
            {
                target.erase(0, 11);
            }
            return std::pair<std::string, std::string>(remote, target);
        }

        std::vector<std::string> remote_names(git_repository* repo)
        {
            StrArray names;
            std::vector<std::string> result;
            if (git_remote_list(&names.array, repo) == 0)
            {
                for (std::size_t i = 0; i < names.array.count; ++i)
                {
                    result.emplace_back(names.array.strings[i]);
                }
            }
            return result;
        }

        std::string default_remote(git_repository* repo)
        {
            if (const auto branch = current_branch(repo))
            {
                if (const auto upstream = upstream_of(repo, *branch))
                {
                    return upstream->first;
                }
            }
            const std::vector<std::string> names = remote_names(repo);
            if (std::find(names.begin(), names.end(), "origin") != names.end() || names.empty())
            {
                return "origin";
            }
            return names.front();
        }

        bool resolve_commit(git_repository* repo, const std::string& spec, CommitHandle& commit)
        {
            ObjectHandle object;
            if (git_revparse_single(object.out(), repo, spec.c_str()) < 0)
            {
                return false;
            }
            return git_object_peel(reinterpret_cast<git_object**>(commit.out()), object.get(), GIT_OBJECT_COMMIT)
                   == 0;
        }

        bool signature(Command& command, git_repository* repo, SignatureHandle& out)
        {
            if (git_signature_default(out.out(), repo) < 0)
            {
                command.print("Author identity unknown\n\n*** Please tell me who you are.\n\n"
                              "  git config --global user.name \"Your Name\"\n"
                              "  git config --global user.email \"you@example.com\"\n\n");
                command.fail("unable to auto-detect email address");
                return false;
            }
            return true;
        }

        bool index_has_conflicts(git_repository* repo)
        {
            IndexHandle index;
            return git_repository_index(index.out(), repo) == 0 && git_index_has_conflicts(index.get());
        }

        void print_conflicts(Command& command, git_repository* repo)
        {
            IndexHandle index;
            if (git_repository_index(index.out(), repo) < 0)
            {
                return;
            }
            git_index_conflict_iterator* iterator = nullptr;
            if (git_index_conflict_iterator_new(&iterator, index.get()) < 0)
            {
                return;
            }
            const git_index_entry* ancestor = nullptr;
            const git_index_entry* ours = nullptr;
            const git_index_entry* theirs = nullptr;
            while (git_index_conflict_next(&ancestor, &ours, &theirs, iterator) == 0)
            {
                const git_index_entry* entry = ours != nullptr ? ours : theirs != nullptr ? theirs : ancestor;
                command.print(std::string("CONFLICT (content): Merge conflict in ") + entry->path + "\n");
            }
            git_index_conflict_iterator_free(iterator);
        }

        // Moves the checked-out branch (or a detached HEAD) to `target`, updating the files.
        int fast_forward(Command& command, git_repository* repo, const git_oid* target, const std::string& message)
        {
            ObjectHandle commit;
            if (git_object_lookup(commit.out(), repo, target, GIT_OBJECT_COMMIT) < 0)
            {
                return command.fail_git("commit not found");
            }
            git_checkout_options checkout;
            git_checkout_options_init(&checkout, GIT_CHECKOUT_OPTIONS_VERSION);
            checkout.checkout_strategy = GIT_CHECKOUT_SAFE;
            checkout.progress_cb = on_checkout_progress;
            checkout.progress_payload = &command;
            if (git_checkout_tree(repo, commit.get(), &checkout) < 0)
            {
                return command.fail_git("checkout failed");
            }
            RefHandle head;
            RefHandle moved;
            if (git_repository_head(head.out(), repo) == 0 && git_reference_is_branch(head.get()))
            {
                if (git_reference_set_target(moved.out(), head.get(), target, message.c_str()) < 0)
                {
                    return command.fail_git("could not update the branch");
                }
            }
            else if (git_repository_set_head_detached(repo, target) < 0)
            {
                return command.fail_git("could not update HEAD");
            }
            return 0;
        }

        // ------------------------------------------------------------------------------------------------------
        // Network commands

        int fetch_remote(Command& command, git_repository* repo, const std::string& name,
                         const std::vector<std::string>& refspecs, bool prune)
        {
            RemoteHandle remote;
            if (git_remote_lookup(remote.out(), repo, name.c_str()) < 0)
            {
                return command.fail("'" + name + "' does not appear to be a git repository");
            }
            const std::string url = git_remote_url(remote.get()) != nullptr ? git_remote_url(remote.get()) : "";
            git_fetch_options options;
            git_fetch_options_init(&options, GIT_FETCH_OPTIONS_VERSION);
            options.callbacks = callbacks(command);
            options.prune = prune ? GIT_FETCH_PRUNE : GIT_FETCH_PRUNE_UNSPECIFIED;
            command.print("From " + url + "\n");
            const Paths specs(refspecs);
            if (git_remote_fetch(remote.get(), refspecs.empty() ? nullptr : &specs.array, &options, nullptr) < 0)
            {
                return network_failure(command, url, "fetch failed");
            }
            return 0;
        }

        int run_fetch(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            bool all = false;
            bool prune = false;
            std::vector<std::string> positional;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                const std::string& arg = command.args[i];
                if (arg == "--all")
                {
                    all = true;
                }
                else if (arg == "--prune" || arg == "-p")
                {
                    prune = true;
                }
                else if (!arg.starts_with("-"))
                {
                    positional.push_back(arg);
                }
            }
            if (all)
            {
                for (const std::string& name : remote_names(repo.get()))
                {
                    if (const int error = fetch_remote(command, repo.get(), name, {}, prune); error != 0)
                    {
                        return error;
                    }
                }
                return 0;
            }
            const std::string name = positional.empty() ? default_remote(repo.get()) : positional.front();
            std::vector<std::string> refspecs;
            for (std::size_t i = 1; i < positional.size(); ++i)
            {
                // "branch:local" fetches into a local branch, as `git fetch origin main:main`.
                std::string spec = positional[i];
                if (const auto colon = spec.find(':'); colon != std::string::npos)
                {
                    spec = "refs/heads/" + spec.substr(0, colon) + ":refs/heads/" + spec.substr(colon + 1);
                }
                refspecs.push_back(spec);
            }
            return fetch_remote(command, repo.get(), name, refspecs, prune);
        }

        int run_clone(Command& command)
        {
            std::vector<std::string> positional;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                if (!command.args[i].starts_with("-"))
                {
                    positional.push_back(command.args[i]);
                }
            }
            if (positional.empty())
            {
                return command.fail("You must specify a repository to clone.");
            }
            const std::string url = positional[0];
            std::string name = positional.size() > 1 ? positional[1] : std::string();
            if (name.empty())
            {
                name = url;
                while (!name.empty() && name.back() == '/')
                {
                    name.pop_back();
                }
                name = name.substr(name.find_last_of("/:") + 1);
                if (name.ends_with(".git"))
                {
                    name.resize(name.size() - 4);
                }
            }
            const std::string destination = absolute(command.workdir, name);
            if (!url.starts_with("https://") && !url.starts_with("http://"))
            {
                return command.fail("the browser reaches remotes over HTTP(S) only, not '" + url + "'");
            }
            command.print("Cloning into '" + name + "'...\n");
            git_clone_options options;
            git_clone_options_init(&options, GIT_CLONE_OPTIONS_VERSION);
            options.fetch_opts.callbacks = callbacks(command);
            options.checkout_opts.progress_cb = on_checkout_progress;
            options.checkout_opts.progress_payload = &command;
            RepoHandle repo;
            if (git_clone(repo.out(), url.c_str(), destination.c_str(), &options) < 0)
            {
                return network_failure(command, url, "clone failed");
            }
            return 0;
        }

        // Rebases the checked-out branch onto `upstream` (the commits it has that upstream lacks, one by one).
        int rebase_onto(Command& command, git_repository* repo, git_annotated_commit* upstream)
        {
            SignatureHandle committer;
            if (!signature(command, repo, committer))
            {
                return 128;
            }
            git_rebase_options options;
            git_rebase_options_init(&options, GIT_REBASE_OPTIONS_VERSION);
            RebaseHandle rebase;
            if (git_rebase_init(rebase.out(), repo, nullptr, upstream, nullptr, &options) < 0)
            {
                return command.fail_git("could not start the rebase");
            }
            git_rebase_operation* operation = nullptr;
            int error = 0;
            while ((error = git_rebase_next(&operation, rebase.get())) == 0)
            {
                if (index_has_conflicts(repo))
                {
                    print_conflicts(command, repo);
                    command.print("error: could not apply " + short_oid(&operation->id)
                                  + "\nResolve all conflicts, stage them, then commit; or abort the rebase.\n");
                    return 1;
                }
                git_oid committed;
                const int commit = git_rebase_commit(&committed, rebase.get(), nullptr, committer.get(), nullptr,
                                                     nullptr);
                if (commit < 0 && commit != GIT_EAPPLIED)
                {
                    return command.fail_git("could not commit the rebased change");
                }
            }
            if (error != GIT_ITEROVER)
            {
                return command.fail_git("rebase failed");
            }
            if (git_rebase_finish(rebase.get(), committer.get()) < 0)
            {
                return command.fail_git("could not finish the rebase");
            }
            command.print("Successfully rebased and updated HEAD.\n");
            return 0;
        }

        // Merges `their` into the checked-out branch: a fast-forward where it can, else a merge commit (or a rebase).
        int integrate(Command& command, git_repository* repo, git_annotated_commit* their,
                      const std::string& their_name, bool ff_only, bool rebase)
        {
            git_merge_analysis_t analysis = GIT_MERGE_ANALYSIS_NONE;
            git_merge_preference_t preference = GIT_MERGE_PREFERENCE_NONE;
            const git_annotated_commit* heads[] = {their};
            if (git_merge_analysis(&analysis, &preference, repo, heads, 1) < 0)
            {
                return command.fail_git("could not analyse the merge");
            }
            if (analysis & GIT_MERGE_ANALYSIS_UP_TO_DATE)
            {
                command.print("Already up to date.\n");
                return 0;
            }
            if (analysis & (GIT_MERGE_ANALYSIS_FASTFORWARD | GIT_MERGE_ANALYSIS_UNBORN))
            {
                command.print("Fast-forward\n");
                return fast_forward(command, repo, git_annotated_commit_id(their), "pull: Fast-forward");
            }
            if (ff_only)
            {
                return command.fail("Not possible to fast-forward, aborting.");
            }
            if (rebase)
            {
                return rebase_onto(command, repo, their);
            }
            SignatureHandle committer;
            if (!signature(command, repo, committer))
            {
                return 128;
            }
            git_merge_options merge;
            git_merge_options_init(&merge, GIT_MERGE_OPTIONS_VERSION);
            git_checkout_options checkout;
            git_checkout_options_init(&checkout, GIT_CHECKOUT_OPTIONS_VERSION);
            checkout.checkout_strategy = GIT_CHECKOUT_SAFE | GIT_CHECKOUT_ALLOW_CONFLICTS;
            if (git_merge(repo, heads, 1, &merge, &checkout) < 0)
            {
                return command.fail_git("merge failed");
            }
            if (index_has_conflicts(repo))
            {
                print_conflicts(command, repo);
                command.print("Automatic merge failed; fix conflicts and then commit the result.\n");
                return 1;
            }
            IndexHandle index;
            git_oid tree_id;
            TreeHandle tree;
            CommitHandle ours;
            CommitHandle theirs;
            if (git_repository_index(index.out(), repo) < 0 || git_index_write_tree(&tree_id, index.get()) < 0
                || git_tree_lookup(tree.out(), repo, &tree_id) < 0 || !resolve_commit(repo, "HEAD", ours)
                || git_commit_lookup(theirs.out(), repo, git_annotated_commit_id(their)) < 0)
            {
                return command.fail_git("could not write the merge");
            }
            const std::string message = "Merge " + their_name + "\n";
            const git_commit* parents[] = {ours.get(), theirs.get()};
            git_oid merged;
            if (git_commit_create(&merged, repo, "HEAD", committer.get(), committer.get(), nullptr, message.c_str(),
                                  tree.get(), 2, parents)
                < 0)
            {
                return command.fail_git("could not commit the merge");
            }
            git_repository_state_cleanup(repo);
            command.print("Merge made by the 'ort' strategy.\n");
            return 0;
        }

        int run_pull(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            bool ff_only = false;
            bool rebase = false;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                const std::string& arg = command.args[i];
                ff_only = ff_only || arg == "--ff-only";
                rebase = (rebase || arg == "--rebase" || arg == "-r") && arg != "--no-rebase";
            }
            const auto branch = current_branch(repo.get());
            if (!branch)
            {
                return command.fail("You are not currently on a branch.");
            }
            const auto upstream = upstream_of(repo.get(), *branch);
            if (!upstream)
            {
                return command.fail("There is no tracking information for the current branch.");
            }
            if (const int error = fetch_remote(command, repo.get(), upstream->first, {}, false); error != 0)
            {
                return error;
            }
            const std::string tracking = "refs/remotes/" + upstream->first + "/" + upstream->second;
            RefHandle ref;
            AnnotatedHandle their;
            if (git_reference_lookup(ref.out(), repo.get(), tracking.c_str()) < 0
                || git_annotated_commit_from_ref(their.out(), repo.get(), ref.get()) < 0)
            {
                return command.fail("couldn't find remote ref " + upstream->second);
            }
            return integrate(command, repo.get(), their.get(),
                             "branch '" + upstream->second + "' of " + upstream->first, ff_only, rebase);
        }

        // The remote's current value of `ref`, for --force-with-lease.
        std::optional<std::string> remote_value(Command& command, git_remote* remote, const std::string& ref)
        {
            git_remote_callbacks remote_callbacks = callbacks(command);
            if (git_remote_connect(remote, GIT_DIRECTION_PUSH, &remote_callbacks, nullptr, nullptr) < 0)
            {
                return std::nullopt;
            }
            const git_remote_head** heads = nullptr;
            size_t count = 0;
            std::optional<std::string> value = std::string();
            if (git_remote_ls(&heads, &count, remote) == 0)
            {
                for (size_t i = 0; i < count; ++i)
                {
                    if (ref == heads[i]->name)
                    {
                        value = oid_text(&heads[i]->oid);
                    }
                }
            }
            git_remote_disconnect(remote);
            return value;
        }

        int run_push(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            bool force = false;
            bool lease = false;
            bool remove = false;
            bool set_upstream = false;
            std::vector<std::string> positional;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                const std::string& arg = command.args[i];
                if (arg == "--force" || arg == "-f")
                {
                    force = true;
                }
                else if (arg.starts_with("--force-with-lease"))
                {
                    lease = true;
                }
                else if (arg == "--delete" || arg == "-d")
                {
                    remove = true;
                }
                else if (arg == "--set-upstream" || arg == "-u")
                {
                    set_upstream = true;
                }
                else if (!arg.starts_with("-"))
                {
                    positional.push_back(arg);
                }
            }
            const auto branch = current_branch(repo.get());
            const std::string name = positional.empty() ? default_remote(repo.get()) : positional.front();
            RemoteHandle remote;
            if (git_remote_lookup(remote.out(), repo.get(), name.c_str()) < 0)
            {
                return command.fail("'" + name + "' does not appear to be a git repository");
            }
            const std::string url = git_remote_url(remote.get()) != nullptr ? git_remote_url(remote.get()) : "";
            // refspecs: "src:dst", a branch or ref name, or the current branch to its upstream (or its own name)
            std::vector<std::pair<std::string, std::string>> updates;
            const auto qualify = [&](const std::string& ref)
            {
                if (ref.starts_with("refs/"))
                {
                    return ref;
                }
                RefHandle tag;
                if (git_reference_lookup(tag.out(), repo.get(), ("refs/tags/" + ref).c_str()) == 0
                    && !remove)
                {
                    return "refs/tags/" + ref;
                }
                return "refs/heads/" + ref;
            };
            for (std::size_t i = 1; i < positional.size(); ++i)
            {
                const std::string& spec = positional[i];
                const auto colon = spec.find(':');
                if (remove)
                {
                    updates.emplace_back("", qualify(spec));
                }
                else if (colon != std::string::npos)
                {
                    updates.emplace_back(qualify(spec.substr(0, colon)), qualify(spec.substr(colon + 1)));
                }
                else
                {
                    updates.emplace_back(qualify(spec), qualify(spec));
                }
            }
            if (updates.empty())
            {
                if (!branch)
                {
                    return command.fail("You are not currently on a branch.");
                }
                const auto upstream = upstream_of(repo.get(), *branch);
                const std::string target = upstream && upstream->first == name ? upstream->second : *branch;
                if (!upstream && !set_upstream)
                {
                    return command.fail("The current branch " + *branch + " has no upstream branch.\n"
                                        + "To push the current branch and set the remote as upstream, use\n\n"
                                        + "    git push --set-upstream " + name + " " + *branch + "\n");
                }
                updates.emplace_back("refs/heads/" + *branch, "refs/heads/" + target);
            }
            if (lease)
            {
                // Forced only while the remote still holds what this repository last saw of it.
                for (const auto& [source, destination] : updates)
                {
                    if (!destination.starts_with("refs/heads/"))
                    {
                        continue;
                    }
                    const std::string tracking = "refs/remotes/" + name + "/" + destination.substr(11);
                    git_oid seen;
                    const std::string expected =
                            git_reference_name_to_id(&seen, repo.get(), tracking.c_str()) == 0 ? oid_text(&seen) : "";
                    const auto actual = remote_value(command, remote.get(), destination);
                    if (!actual)
                    {
                        return network_failure(command, url, "could not reach the remote");
                    }
                    if (*actual != expected)
                    {
                        command.print("To " + url + "\n ! [rejected]        " + source + " -> " + destination
                                      + " (stale info)\n");
                        return command.fail("failed to push some refs to '" + url + "'");
                    }
                }
                force = true;
            }
            std::vector<std::string> refspecs;
            for (const auto& [source, destination] : updates)
            {
                refspecs.push_back((force && !source.empty() ? "+" : "") + source + ":" + destination);
            }
            git_push_options options;
            git_push_options_init(&options, GIT_PUSH_OPTIONS_VERSION);
            options.callbacks = callbacks(command);
            command.print("To " + url + "\n");
            const Paths specs(refspecs);
            if (const int error = git_remote_push(remote.get(), &specs.array, &options); error < 0)
            {
                if (error == GIT_ENONFASTFORWARD)
                {
                    const bool missing = last_error("").find("not present locally") != std::string::npos;
                    for (const auto& [source, destination] : updates)
                    {
                        command.print(" ! [rejected]        " + source + " -> " + destination
                                      + (missing ? " (fetch first)\n" : " (non-fast-forward)\n"));
                    }
                    command.print("error: failed to push some refs to '" + url + "'\n");
                    return 1;
                }
                return network_failure(command, url, "push failed");
            }
            if (!command.rejected.empty())
            {
                command.print("error: failed to push some refs to '" + url + "'\n");
                return 1;
            }
            if (set_upstream)
            {
                for (const auto& [source, destination] : updates)
                {
                    if (!source.starts_with("refs/heads/") || !destination.starts_with("refs/heads/"))
                    {
                        continue;
                    }
                    RefHandle local;
                    const std::string upstream = name + "/" + destination.substr(11);
                    if (git_branch_lookup(local.out(), repo.get(), source.substr(11).c_str(), GIT_BRANCH_LOCAL) == 0
                        && git_branch_set_upstream(local.get(), upstream.c_str()) == 0)
                    {
                        command.print("branch '" + source.substr(11) + "' set up to track '" + upstream + "'.\n");
                    }
                }
            }
            return 0;
        }

        // ------------------------------------------------------------------------------------------------------
        // Local commands ImGit runs

        // `rev-list --date-order [--ignore-missing] [--max-count=N] --stdin`: the history's row order.
        int run_rev_list(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            std::size_t max_count = SIZE_MAX;
            for (const std::string& arg : command.args)
            {
                if (arg.starts_with("--max-count="))
                {
                    max_count = static_cast<std::size_t>(std::strtoull(arg.c_str() + 12, nullptr, 10));
                }
            }
            WalkHandle walk;
            if (git_revwalk_new(walk.out(), repo.get()) < 0)
            {
                return command.fail_git("revwalk failed");
            }
            git_revwalk_sorting(walk.get(), GIT_SORT_TIME);
            std::istringstream lines(command.input);
            std::string line;
            while (std::getline(lines, line))
            {
                git_oid oid;
                if (git_oid_fromstrp(&oid, line.c_str()) == 0)
                {
                    git_revwalk_push(walk.get(), &oid); // --ignore-missing: a failure is skipped
                }
            }
            std::string listed;
            git_oid oid;
            for (std::size_t count = 0; count < max_count && git_revwalk_next(&oid, walk.get()) == 0; ++count)
            {
                listed += oid_text(&oid) + "\n";
            }
            command.print(listed);
            return 0;
        }

        // `merge / cherry-pick / revert / rebase --abort`
        int run_abort(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            if (command.args.front() == "rebase")
            {
                RebaseHandle rebase;
                git_rebase_options options;
                git_rebase_options_init(&options, GIT_REBASE_OPTIONS_VERSION);
                if (git_rebase_open(rebase.out(), repo.get(), &options) < 0 || git_rebase_abort(rebase.get()) < 0)
                {
                    return command.fail_git("No rebase in progress?");
                }
                return 0;
            }
            CommitHandle head;
            if (!resolve_commit(repo.get(), "HEAD", head))
            {
                return command.fail_git("HEAD not found");
            }
            git_checkout_options checkout;
            git_checkout_options_init(&checkout, GIT_CHECKOUT_OPTIONS_VERSION);
            checkout.checkout_strategy = GIT_CHECKOUT_FORCE;
            if (git_reset(repo.get(), reinterpret_cast<git_object*>(head.get()), GIT_RESET_HARD, &checkout) < 0)
            {
                return command.fail_git("reset failed");
            }
            git_repository_state_cleanup(repo.get());
            return 0;
        }

        // `rebase --continue` / `rebase <upstream>`
        int run_rebase(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            if (command.args.size() < 2)
            {
                return command.fail("no upstream given");
            }
            if (command.args[1] == "--continue")
            {
                SignatureHandle committer;
                if (!signature(command, repo.get(), committer))
                {
                    return 128;
                }
                RebaseHandle rebase;
                git_rebase_options options;
                git_rebase_options_init(&options, GIT_REBASE_OPTIONS_VERSION);
                if (git_rebase_open(rebase.out(), repo.get(), &options) < 0)
                {
                    return command.fail("No rebase in progress?");
                }
                if (index_has_conflicts(repo.get()))
                {
                    return command.fail("resolve the conflicts and stage the files first");
                }
                git_oid committed;
                const int commit = git_rebase_commit(&committed, rebase.get(), nullptr, committer.get(), nullptr,
                                                     nullptr);
                if (commit < 0 && commit != GIT_EAPPLIED)
                {
                    return command.fail_git("could not commit");
                }
                git_rebase_operation* operation = nullptr;
                int error = 0;
                while ((error = git_rebase_next(&operation, rebase.get())) == 0)
                {
                    if (index_has_conflicts(repo.get()))
                    {
                        print_conflicts(command, repo.get());
                        return 1;
                    }
                    if (const int next = git_rebase_commit(&committed, rebase.get(), nullptr, committer.get(),
                                                           nullptr, nullptr);
                        next < 0 && next != GIT_EAPPLIED)
                    {
                        return command.fail_git("could not commit");
                    }
                }
                if (error != GIT_ITEROVER || git_rebase_finish(rebase.get(), committer.get()) < 0)
                {
                    return command.fail_git("rebase failed");
                }
                command.print("Successfully rebased and updated HEAD.\n");
                return 0;
            }
            CommitHandle upstream;
            AnnotatedHandle annotated;
            if (!resolve_commit(repo.get(), command.args[1], upstream)
                || git_annotated_commit_lookup(annotated.out(), repo.get(), git_commit_id(upstream.get())) < 0)
            {
                return command.fail("invalid upstream '" + command.args[1] + "'");
            }
            return rebase_onto(command, repo.get(), annotated.get());
        }

        int stash_index(const std::string& spec)
        {
            // "stash@{n}" or "n"
            const auto open = spec.find('{');
            const std::string number = open == std::string::npos ? spec : spec.substr(open + 1);
            return std::atoi(number.c_str());
        }

        int run_stash(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            const std::string verb = command.args.size() > 1 ? command.args[1] : "push";
            if (verb == "push" || verb == "save" || verb.starts_with("-"))
            {
                SignatureHandle stasher;
                if (!signature(command, repo.get(), stasher))
                {
                    return 128;
                }
                std::vector<std::string> paths;
                std::string message;
                bool after_dashes = false;
                for (std::size_t i = 1; i < command.args.size(); ++i)
                {
                    const std::string& arg = command.args[i];
                    if (after_dashes)
                    {
                        paths.push_back(arg);
                    }
                    else if (arg == "--")
                    {
                        after_dashes = true;
                    }
                    else if ((arg == "-m" || arg == "--message") && i + 1 < command.args.size())
                    {
                        message = command.args[++i];
                    }
                }
                const Paths pathspec(paths);
                git_stash_save_options options;
                git_stash_save_options_init(&options, GIT_STASH_SAVE_OPTIONS_VERSION);
                options.stasher = stasher.get();
                options.message = message.empty() ? nullptr : message.c_str();
                options.paths = pathspec.array;
                git_oid stashed;
                if (const int error = git_stash_save_with_opts(&stashed, repo.get(), &options); error < 0)
                {
                    if (error == GIT_ENOTFOUND)
                    {
                        command.print("No local changes to save\n");
                        return 0;
                    }
                    return command.fail_git("stash failed");
                }
                command.print("Saved working directory and index state\n");
                return 0;
            }
            if (verb == "list")
            {
                git_stash_foreach(
                        repo.get(),
                        [](size_t index, const char* message, const git_oid*, void* payload)
                        {
                            static_cast<Command*>(payload)->print("stash@{" + std::to_string(index) + "}: " + message
                                                                  + "\n");
                            return 0;
                        },
                        &command);
                return 0;
            }
            bool index = false;
            std::string spec = "stash@{0}";
            for (std::size_t i = 2; i < command.args.size(); ++i)
            {
                if (command.args[i] == "--index")
                {
                    index = true;
                }
                else if (!command.args[i].starts_with("-"))
                {
                    spec = command.args[i];
                }
            }
            git_stash_apply_options options;
            git_stash_apply_options_init(&options, GIT_STASH_APPLY_OPTIONS_VERSION);
            options.flags = index ? GIT_STASH_APPLY_REINSTATE_INDEX : GIT_STASH_APPLY_DEFAULT;
            const int position = stash_index(spec);
            int error = 0;
            if (verb == "apply")
            {
                error = git_stash_apply(repo.get(), static_cast<size_t>(position), &options);
            }
            else if (verb == "pop")
            {
                error = git_stash_pop(repo.get(), static_cast<size_t>(position), &options);
            }
            else if (verb == "drop")
            {
                error = git_stash_drop(repo.get(), static_cast<size_t>(position));
            }
            else
            {
                return command.fail("unknown stash command '" + verb + "'");
            }
            if (error < 0)
            {
                if (error == GIT_EMERGECONFLICT || error == GIT_ECONFLICT)
                {
                    command.print("error: Your local changes would be overwritten.\n");
                    return 1;
                }
                return command.fail_git("stash failed");
            }
            return 0;
        }

        // `apply [--cached] <patch>`
        int run_apply(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            bool cached = false;
            std::string file;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                if (command.args[i] == "--cached" || command.args[i] == "--index")
                {
                    cached = true;
                }
                else if (!command.args[i].starts_with("-"))
                {
                    file = command.args[i];
                }
            }
            std::string patch = command.input;
            if (!file.empty())
            {
                std::ifstream in(absolute(command.workdir, file), std::ios::binary);
                if (!in)
                {
                    return command.fail("can't open patch '" + file + "'");
                }
                patch.assign(std::istreambuf_iterator<char>(in), {});
            }
            DiffHandle diff;
            if (git_diff_from_buffer(diff.out(), patch.data(), patch.size()) < 0)
            {
                return command.fail_git("corrupt patch");
            }
            if (git_apply(repo.get(), diff.get(), cached ? GIT_APPLY_LOCATION_INDEX : GIT_APPLY_LOCATION_WORKDIR,
                          nullptr)
                < 0)
            {
                command.print("error: " + last_error("patch does not apply") + "\n");
                return 1;
            }
            return 0;
        }

        std::string diff_text(git_diff* diff)
        {
            Buf buf;
            if (git_diff_to_buf(&buf.buf, diff, GIT_DIFF_FORMAT_PATCH) < 0)
            {
                return {};
            }
            return std::string(buf.buf.ptr, buf.buf.size);
        }

        // `diff [--cached] [<commit>] [-- <path>...]`
        int run_diff(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            bool cached = false;
            std::string base;
            std::vector<std::string> paths;
            bool after_dashes = false;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                const std::string& arg = command.args[i];
                if (after_dashes)
                {
                    paths.push_back(arg);
                }
                else if (arg == "--")
                {
                    after_dashes = true;
                }
                else if (arg == "--cached" || arg == "--staged")
                {
                    cached = true;
                }
                else if (!arg.starts_with("-"))
                {
                    base = arg;
                }
            }
            const Paths pathspec(paths);
            git_diff_options options;
            git_diff_options_init(&options, GIT_DIFF_OPTIONS_VERSION);
            options.pathspec = pathspec.array;
            options.flags |= GIT_DIFF_SHOW_BINARY;
            TreeHandle tree;
            if (cached || !base.empty())
            {
                CommitHandle commit;
                if (resolve_commit(repo.get(), base.empty() ? "HEAD" : base, commit))
                {
                    git_commit_tree(tree.out(), commit.get());
                }
            }
            DiffHandle diff;
            int error = 0;
            if (cached)
            {
                error = git_diff_tree_to_index(diff.out(), repo.get(), tree.get(), nullptr, &options);
            }
            else if (!base.empty())
            {
                error = git_diff_tree_to_workdir_with_index(diff.out(), repo.get(), tree.get(), &options);
            }
            else
            {
                error = git_diff_index_to_workdir(diff.out(), repo.get(), nullptr, &options);
            }
            if (error < 0)
            {
                return command.fail_git("diff failed");
            }
            command.print(diff_text(diff.get()));
            return 0;
        }

        // `format-patch --stdout (-1 <commit> | <from>..<to>)`
        int run_format_patch(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            bool single = false;
            std::string spec;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                if (command.args[i] == "-1")
                {
                    single = true;
                }
                else if (!command.args[i].starts_with("-"))
                {
                    spec = command.args[i];
                }
            }
            std::vector<git_oid> commits;
            if (single)
            {
                CommitHandle commit;
                if (!resolve_commit(repo.get(), spec, commit))
                {
                    return command.fail("bad revision '" + spec + "'");
                }
                commits.push_back(*git_commit_id(commit.get()));
            }
            else
            {
                WalkHandle walk;
                if (git_revwalk_new(walk.out(), repo.get()) < 0 || git_revwalk_push_range(walk.get(), spec.c_str()) < 0)
                {
                    return command.fail("bad revision range '" + spec + "'");
                }
                git_revwalk_sorting(walk.get(), GIT_SORT_TOPOLOGICAL | GIT_SORT_REVERSE);
                git_oid oid;
                while (git_revwalk_next(&oid, walk.get()) == 0)
                {
                    commits.push_back(oid);
                }
            }
            std::string patches;
            for (std::size_t i = 0; i < commits.size(); ++i)
            {
                CommitHandle commit;
                CommitHandle parent;
                TreeHandle tree;
                TreeHandle parent_tree;
                if (git_commit_lookup(commit.out(), repo.get(), &commits[i]) < 0
                    || git_commit_tree(tree.out(), commit.get()) < 0)
                {
                    return command.fail_git("commit not found");
                }
                if (git_commit_parentcount(commit.get()) > 0 && git_commit_parent(parent.out(), commit.get(), 0) == 0)
                {
                    git_commit_tree(parent_tree.out(), parent.get());
                }
                DiffHandle diff;
                if (git_diff_tree_to_tree(diff.out(), repo.get(), parent_tree.get(), tree.get(), nullptr) < 0)
                {
                    return command.fail_git("diff failed");
                }
                git_email_create_options options{};
                options.version = GIT_EMAIL_CREATE_OPTIONS_VERSION;
                options.flags = GIT_EMAIL_CREATE_DEFAULT;
                git_diff_options_init(&options.diff_opts, GIT_DIFF_OPTIONS_VERSION);
                options.diff_opts.flags = GIT_DIFF_SHOW_BINARY;
                git_diff_find_options_init(&options.diff_find_opts, GIT_DIFF_FIND_OPTIONS_VERSION);
                Buf email;
                if (git_email_create_from_diff(&email.buf, diff.get(), i + 1, commits.size(), &commits[i],
                                               git_commit_summary(commit.get()), git_commit_body(commit.get()),
                                               git_commit_author(commit.get()), &options)
                    < 0)
                {
                    return command.fail_git("could not format the patch");
                }
                patches.append(email.buf.ptr, email.buf.size);
            }
            command.print(patches);
            return 0;
        }

        // `branch` (list), `branch <name> [<start>]`, `branch -f <name> <target>`, `branch -d|-D <name>`
        int run_branch(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            bool force = false;
            bool remove = false;
            std::vector<std::string> positional;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                const std::string& arg = command.args[i];
                if (arg == "-f" || arg == "--force")
                {
                    force = true;
                }
                else if (arg == "-d" || arg == "-D" || arg == "--delete")
                {
                    remove = true;
                }
                else if (!arg.starts_with("-"))
                {
                    positional.push_back(arg);
                }
            }
            if (positional.empty())
            {
                git_branch_iterator* iterator = nullptr;
                if (git_branch_iterator_new(&iterator, repo.get(), GIT_BRANCH_LOCAL) < 0)
                {
                    return command.fail_git("could not list branches");
                }
                git_reference* ref = nullptr;
                git_branch_t type;
                std::string listed;
                while (git_branch_next(&ref, &type, iterator) == 0)
                {
                    listed += std::string(git_branch_is_head(ref) == 1 ? "* " : "  ") + git_reference_shorthand(ref)
                              + "\n";
                    git_reference_free(ref);
                }
                git_branch_iterator_free(iterator);
                command.print(listed);
                return 0;
            }
            if (remove)
            {
                RefHandle ref;
                if (git_branch_lookup(ref.out(), repo.get(), positional.front().c_str(), GIT_BRANCH_LOCAL) < 0
                    || git_branch_delete(ref.get()) < 0)
                {
                    return command.fail_git("could not delete the branch");
                }
                command.print("Deleted branch " + positional.front() + "\n");
                return 0;
            }
            CommitHandle target;
            if (!resolve_commit(repo.get(), positional.size() > 1 ? positional[1] : "HEAD", target))
            {
                return command.fail("not a valid object name");
            }
            RefHandle created;
            if (git_branch_create(created.out(), repo.get(), positional.front().c_str(), target.get(), force ? 1 : 0)
                < 0)
            {
                return command.fail_git("could not create the branch");
            }
            return 0;
        }

        // `checkout <commit> -- <path>...`, `checkout|switch [-b] <branch>`
        int run_checkout(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            bool create = false;
            std::vector<std::string> positional;
            std::vector<std::string> paths;
            bool after_dashes = false;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                const std::string& arg = command.args[i];
                if (after_dashes)
                {
                    paths.push_back(arg);
                }
                else if (arg == "--")
                {
                    after_dashes = true;
                }
                else if (arg == "-b" || arg == "-c")
                {
                    create = true;
                }
                else if (!arg.starts_with("-"))
                {
                    positional.push_back(arg);
                }
            }
            git_checkout_options checkout;
            git_checkout_options_init(&checkout, GIT_CHECKOUT_OPTIONS_VERSION);
            if (!paths.empty())
            {
                // Files from a commit (or the index), over the working copy.
                const Paths pathspec(paths);
                checkout.checkout_strategy = GIT_CHECKOUT_FORCE | GIT_CHECKOUT_DISABLE_PATHSPEC_MATCH;
                checkout.paths = pathspec.array;
                int error = 0;
                if (positional.empty())
                {
                    error = git_checkout_index(repo.get(), nullptr, &checkout);
                }
                else
                {
                    CommitHandle commit;
                    if (!resolve_commit(repo.get(), positional.front(), commit))
                    {
                        return command.fail("invalid reference: " + positional.front());
                    }
                    error = git_checkout_tree(repo.get(), reinterpret_cast<git_object*>(commit.get()), &checkout);
                }
                return error < 0 ? command.fail_git("checkout failed") : 0;
            }
            if (positional.empty())
            {
                return command.fail("no branch given");
            }
            const std::string name = positional.front();
            RefHandle branch;
            if (create)
            {
                CommitHandle start;
                if (!resolve_commit(repo.get(), positional.size() > 1 ? positional[1] : "HEAD", start)
                    || git_branch_create(branch.out(), repo.get(), name.c_str(), start.get(), 0) < 0)
                {
                    return command.fail_git("could not create the branch");
                }
            }
            else if (git_branch_lookup(branch.out(), repo.get(), name.c_str(), GIT_BRANCH_LOCAL) < 0)
            {
                // A remote branch of that name: a local one that tracks it, as git's checkout does.
                RefHandle remote_branch;
                CommitHandle start;
                const std::string tracking = default_remote(repo.get()) + "/" + name;
                if (git_branch_lookup(remote_branch.out(), repo.get(), tracking.c_str(), GIT_BRANCH_REMOTE) < 0
                    || !resolve_commit(repo.get(), tracking, start)
                    || git_branch_create(branch.out(), repo.get(), name.c_str(), start.get(), 0) < 0)
                {
                    // Not a branch: a detached checkout of a commit.
                    CommitHandle commit;
                    if (!resolve_commit(repo.get(), name, commit))
                    {
                        return command.fail("pathspec '" + name + "' did not match any file(s) known to git");
                    }
                    checkout.checkout_strategy = GIT_CHECKOUT_SAFE;
                    if (git_checkout_tree(repo.get(), reinterpret_cast<git_object*>(commit.get()), &checkout) < 0
                        || git_repository_set_head_detached(repo.get(), git_commit_id(commit.get())) < 0)
                    {
                        return command.fail_git("checkout failed");
                    }
                    command.print("HEAD is now at " + short_oid(git_commit_id(commit.get())) + "\n");
                    return 0;
                }
                git_branch_set_upstream(branch.get(), tracking.c_str());
            }
            ObjectHandle target;
            if (git_reference_peel(target.out(), branch.get(), GIT_OBJECT_COMMIT) < 0)
            {
                return command.fail_git("branch has no commit");
            }
            checkout.checkout_strategy = GIT_CHECKOUT_SAFE;
            if (git_checkout_tree(repo.get(), target.get(), &checkout) < 0
                || git_repository_set_head(repo.get(), git_reference_name(branch.get())) < 0)
            {
                return command.fail_git("checkout failed");
            }
            command.print("Switched to branch '" + name + "'\n");
            return 0;
        }

        // ------------------------------------------------------------------------------------------------------
        // Commands for the terminal panel

        int run_init(Command& command)
        {
            const std::string path = absolute(command.workdir, command.args.size() > 1 ? command.args.back() : ".");
            RepoHandle repo;
            if (git_repository_init(repo.out(), path.c_str(), 0) < 0)
            {
                return command.fail_git("init failed");
            }
            command.print("Initialized empty Git repository in " + path + "/.git/\n");
            return 0;
        }

        int run_status(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            if (const auto branch = current_branch(repo.get()))
            {
                command.print("On branch " + *branch + "\n");
            }
            git_status_options options;
            git_status_options_init(&options, GIT_STATUS_OPTIONS_VERSION);
            options.flags = GIT_STATUS_OPT_INCLUDE_UNTRACKED | GIT_STATUS_OPT_RECURSE_UNTRACKED_DIRS;
            StatusHandle list;
            if (git_status_list_new(list.out(), repo.get(), &options) < 0)
            {
                return command.fail_git("status failed");
            }
            const std::size_t count = git_status_list_entrycount(list.get());
            if (count == 0)
            {
                command.print("nothing to commit, working tree clean\n");
                return 0;
            }
            std::string listed;
            for (std::size_t i = 0; i < count; ++i)
            {
                const git_status_entry* entry = git_status_byindex(list.get(), i);
                const unsigned flags = entry->status;
                const char staged = flags & GIT_STATUS_INDEX_NEW          ? 'A'
                                    : flags & GIT_STATUS_INDEX_MODIFIED   ? 'M'
                                    : flags & GIT_STATUS_INDEX_DELETED    ? 'D'
                                    : flags & GIT_STATUS_INDEX_RENAMED    ? 'R'
                                    : flags & GIT_STATUS_WT_NEW           ? '?'
                                    : flags & GIT_STATUS_CONFLICTED       ? 'U'
                                                                          : ' ';
                const char unstaged = flags & GIT_STATUS_WT_MODIFIED ? 'M'
                                      : flags & GIT_STATUS_WT_DELETED ? 'D'
                                      : flags & GIT_STATUS_WT_NEW     ? '?'
                                      : flags & GIT_STATUS_CONFLICTED ? 'U'
                                                                      : ' ';
                const git_diff_delta* delta = entry->index_to_workdir != nullptr ? entry->index_to_workdir
                                                                                 : entry->head_to_index;
                listed += std::string{staged, unstaged, ' '} + (delta != nullptr ? delta->new_file.path : "") + "\n";
            }
            command.print(listed);
            return 0;
        }

        int run_log(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            std::size_t limit = 20;
            bool oneline = false;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                const std::string& arg = command.args[i];
                if (arg == "--oneline")
                {
                    oneline = true;
                }
                else if (arg == "-n" && i + 1 < command.args.size())
                {
                    limit = static_cast<std::size_t>(std::atoi(command.args[++i].c_str()));
                }
                else if (arg.size() > 1 && arg[0] == '-' && std::isdigit(static_cast<unsigned char>(arg[1])))
                {
                    limit = static_cast<std::size_t>(std::atoi(arg.c_str() + 1));
                }
            }
            WalkHandle walk;
            if (git_revwalk_new(walk.out(), repo.get()) < 0 || git_revwalk_push_head(walk.get()) < 0)
            {
                return command.fail("your current branch does not have any commits yet");
            }
            git_revwalk_sorting(walk.get(), GIT_SORT_TOPOLOGICAL | GIT_SORT_TIME);
            std::string listed;
            git_oid oid;
            for (std::size_t count = 0; count < limit && git_revwalk_next(&oid, walk.get()) == 0; ++count)
            {
                CommitHandle commit;
                if (git_commit_lookup(commit.out(), repo.get(), &oid) < 0)
                {
                    continue;
                }
                if (oneline)
                {
                    listed += "\x1b[33m" + short_oid(&oid) + "\x1b[0m " + git_commit_summary(commit.get()) + "\n";
                    continue;
                }
                const git_signature* author = git_commit_author(commit.get());
                listed += "\x1b[33mcommit " + oid_text(&oid) + "\x1b[0m\nAuthor: " + author->name + " <"
                          + author->email + ">\n\n    " + git_commit_summary(commit.get()) + "\n\n";
            }
            command.print(listed);
            return 0;
        }

        int run_add(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            std::vector<std::string> paths;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                const std::string& arg = command.args[i];
                if (arg == "-A" || arg == "--all")
                {
                    paths.push_back(".");
                }
                else if (!arg.starts_with("-"))
                {
                    paths.push_back(arg);
                }
            }
            if (paths.empty())
            {
                return command.fail("Nothing specified, nothing added.");
            }
            const Paths pathspec(paths);
            IndexHandle index;
            if (git_repository_index(index.out(), repo.get()) < 0
                || git_index_add_all(index.get(), &pathspec.array, GIT_INDEX_ADD_DEFAULT, nullptr, nullptr) < 0
                || git_index_update_all(index.get(), &pathspec.array, nullptr, nullptr) < 0
                || git_index_write(index.get()) < 0)
            {
                return command.fail_git("add failed");
            }
            return 0;
        }

        int run_commit(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            std::string message;
            bool all = false;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                const std::string& arg = command.args[i];
                if ((arg == "-m" || arg == "--message") && i + 1 < command.args.size())
                {
                    message += (message.empty() ? "" : "\n\n") + command.args[++i];
                }
                else if (arg == "-a" || arg == "--all")
                {
                    all = true;
                }
                else if (arg == "-am" && i + 1 < command.args.size())
                {
                    all = true;
                    message += (message.empty() ? "" : "\n\n") + command.args[++i];
                }
            }
            if (message.empty())
            {
                return command.fail("no commit message: use -m \"message\" (there is no editor here)");
            }
            SignatureHandle author;
            if (!signature(command, repo.get(), author))
            {
                return 128;
            }
            IndexHandle index;
            git_oid tree_id;
            TreeHandle tree;
            if (git_repository_index(index.out(), repo.get()) < 0
                || (all && (git_index_update_all(index.get(), nullptr, nullptr, nullptr) < 0
                            || git_index_write(index.get()) < 0))
                || git_index_write_tree(&tree_id, index.get()) < 0
                || git_tree_lookup(tree.out(), repo.get(), &tree_id) < 0)
            {
                return command.fail_git("could not write the tree");
            }
            CommitHandle parent;
            const bool has_parent = resolve_commit(repo.get(), "HEAD", parent);
            const git_commit* parents[] = {parent.get()};
            git_oid committed;
            if (git_commit_create(&committed, repo.get(), "HEAD", author.get(), author.get(), nullptr,
                                  (message + "\n").c_str(), tree.get(), has_parent ? 1 : 0, parents)
                < 0)
            {
                return command.fail_git("commit failed");
            }
            command.print("[" + current_branch(repo.get()).value_or("detached HEAD") + " " + short_oid(&committed)
                          + "] " + message.substr(0, message.find('\n')) + "\n");
            return 0;
        }

        int run_remote(Command& command)
        {
            RepoHandle repo;
            if (!open_repository(command, repo))
            {
                return 128;
            }
            const std::string verb = command.args.size() > 1 ? command.args[1] : "";
            if (verb == "add" && command.args.size() > 3)
            {
                RemoteHandle remote;
                if (git_remote_create(remote.out(), repo.get(), command.args[2].c_str(), command.args[3].c_str())
                    < 0)
                {
                    return command.fail_git("could not add the remote");
                }
                return 0;
            }
            if ((verb == "remove" || verb == "rm") && command.args.size() > 2)
            {
                return git_remote_delete(repo.get(), command.args[2].c_str()) < 0
                               ? command.fail_git("could not remove the remote")
                               : 0;
            }
            if ((verb == "set-url") && command.args.size() > 3)
            {
                return git_remote_set_url(repo.get(), command.args[2].c_str(), command.args[3].c_str()) < 0
                               ? command.fail_git("could not set the url")
                               : 0;
            }
            const bool verbose = verb == "-v" || verb == "--verbose";
            std::string listed;
            for (const std::string& name : remote_names(repo.get()))
            {
                RemoteHandle remote;
                if (!verbose || git_remote_lookup(remote.out(), repo.get(), name.c_str()) < 0)
                {
                    listed += name + "\n";
                    continue;
                }
                const std::string url = git_remote_url(remote.get()) != nullptr ? git_remote_url(remote.get()) : "";
                listed += name + "\t" + url + " (fetch)\n" + name + "\t" + url + " (push)\n";
            }
            command.print(listed);
            return 0;
        }

        int run_config(Command& command)
        {
            bool global = false;
            std::vector<std::string> positional;
            for (std::size_t i = 1; i < command.args.size(); ++i)
            {
                if (command.args[i] == "--global")
                {
                    global = true;
                }
                else if (!command.args[i].starts_with("-"))
                {
                    positional.push_back(command.args[i]);
                }
            }
            if (positional.empty())
            {
                return command.fail("usage: git config [--global] <name> [<value>]");
            }
            ConfigHandle config;
            RepoHandle repo;
            if (global)
            {
                ConfigHandle all;
                if (git_config_open_default(all.out()) < 0
                    || git_config_open_level(config.out(), all.get(), GIT_CONFIG_LEVEL_GLOBAL) < 0)
                {
                    // No global file yet: create it where git looks for it.
                    const std::string path = std::string(std::getenv("HOME") != nullptr ? std::getenv("HOME") : "/")
                                             + "/.gitconfig";
                    if (git_config_open_ondisk(config.out(), path.c_str()) < 0)
                    {
                        return command.fail_git("could not open the global config");
                    }
                }
            }
            else if (!open_repository(command, repo) || git_repository_config(config.out(), repo.get()) < 0)
            {
                return 128;
            }
            if (positional.size() > 1)
            {
                return git_config_set_string(config.get(), positional[0].c_str(), positional[1].c_str()) < 0
                               ? command.fail_git("could not set the value")
                               : 0;
            }
            Buf value;
            if (git_config_get_string_buf(&value.buf, config.get(), positional[0].c_str()) < 0)
            {
                return 1;
            }
            command.print(std::string(value.buf.ptr, value.buf.size) + "\n");
            return 0;
        }

        int run_help(Command& command)
        {
            command.print("The browser's git (libgit2) knows:\n"
                          "  clone fetch pull push            remotes over HTTP(S), through the page's CORS proxy\n"
                          "  init status log add commit       (commit -m: there is no editor)\n"
                          "  branch checkout switch remote config\n"
                          "  stash diff apply format-patch rebase rev-list\n"
                          "  merge|rebase|cherry-pick|revert --abort\n"
                          "Not here: ssh remotes, submodule, lfs, sparse-checkout, hooks, credential helpers.\n");
            return 0;
        }

        int dispatch(Command& command)
        {
            if (command.args.empty() || command.args.front() == "help" || command.args.front() == "--help")
            {
                return run_help(command);
            }
            const std::string& verb = command.args.front();
            if (verb == "--version" || verb == "version")
            {
                command.print(std::string("git version 2.47.0 (ImGit in the browser, libgit2 ") + LIBGIT2_VERSION
                              + ")\n");
                return 0;
            }
            if (command.args.size() > 1 && command.args[1] == "--abort"
                && (verb == "merge" || verb == "rebase" || verb == "cherry-pick" || verb == "revert"))
            {
                return run_abort(command);
            }
            static const std::map<std::string, int (*)(Command&)> COMMANDS = {
                    {"clone", run_clone},
                    {"fetch", run_fetch},
                    {"pull", run_pull},
                    {"push", run_push},
                    {"rev-list", run_rev_list},
                    {"rebase", run_rebase},
                    {"stash", run_stash},
                    {"apply", run_apply},
                    {"diff", run_diff},
                    {"format-patch", run_format_patch},
                    {"branch", run_branch},
                    {"checkout", run_checkout},
                    {"switch", run_checkout},
                    {"init", run_init},
                    {"status", run_status},
                    {"log", run_log},
                    {"add", run_add},
                    {"commit", run_commit},
                    {"remote", run_remote},
                    {"config", run_config},
            };
            const auto found = COMMANDS.find(verb);
            if (found == COMMANDS.end())
            {
                command.print("git: '" + verb + "' is not available in the browser. See 'git help'.\n");
                return 1;
            }
            return found->second(command);
        }

        int run_git(const std::string& workdir, std::span<const std::string> args, const std::string& input,
                    const platform::Environment& environment, const OutputSink& out)
        {
            static std::once_flag registered;
            std::call_once(registered,
                           []
                           {
                               git_transport_register("https", create_transport, nullptr);
                               git_transport_register("http", create_transport, nullptr);
                           });
            Command command{workdir, {}, input, cli::credentials_from_environment(environment), out};
            // git's own options before the command: -c name=value (the credential helper overrides) and -C dir
            std::size_t i = 0;
            for (; i < args.size(); ++i)
            {
                if (args[i] == "-c" && i + 1 < args.size())
                {
                    ++i;
                }
                else if (args[i] == "-C" && i + 1 < args.size())
                {
                    command.workdir = absolute(command.workdir, args[++i]);
                }
                else
                {
                    break;
                }
            }
            command.args.assign(args.begin() + static_cast<std::ptrdiff_t>(i), args.end());
            if (command.workdir.empty())
            {
                command.workdir = ".";
            }
            return dispatch(command);
        }

        // Registers `git` before main() runs, and reads the page's proxy setting while on the main thread.
        struct Registration
        {
            Registration()
            {
                char* proxy = imgit_git_proxy();
                g_proxy = proxy != nullptr ? proxy : "";
                std::free(proxy);
                platform::register_program("git", run_git);
            }
        };
        const Registration g_registration;
    } // namespace
} // namespace imgit::git::web
