//! Preferences → Connections: which hosting services this machine can reach and as whom. ImGit has no accounts
//! of its own, so a connection is what git itself would use: an HTTPS username / token (entered this session, or
//! kept by the configured credential helper) and an SSH key the service accepts.
//!
//! This module is the parsing half (remote urls, ssh greetings, credential answers) and the list the page shows;
//! [`checks`] runs git and ssh.

pub mod checks;

use std::collections::HashMap;
use std::str::FromStr;
use std::thread::JoinHandle;

use imgit_git::cli::Credential;

/// How a remote is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    /// `http://` or `https://`
    Https,
    /// `ssh://user@host[:port]/path` or scp-style `user@host:path`
    Ssh,
}

/// A remote url split into what a connection check needs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemoteEndpoint {
    pub transport: Transport,
    /// Lower case, without the port.
    pub host: String,
    /// The ssh user (or the https url's user); `None` when the url has none.
    pub user: Option<String>,
    /// `None` for the default.
    pub port: Option<String>,
}

/// The url names no host to connect to: a local path, or a `file://` or `git://` url.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, thiserror::Error)]
#[error("not the url of a remote reached over HTTPS or SSH")]
pub struct NotARemoteUrl;

impl FromStr for RemoteEndpoint {
    type Err = NotARemoteUrl;

    fn from_str(url: &str) -> Result<Self, Self::Err> {
        parse_remote_url(url).ok_or(NotARemoteUrl)
    }
}

/// `None` for an empty text.
fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_owned())
}

/// The white space of the C locale, trimmed off both ends.
fn trim(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, ' ' | '\t'..='\r'))
}

/// `host` is `domain` or one of its subdomains.
fn is_in_domain(host: &str, domain: &str) -> bool {
    host.strip_suffix(domain)
        .is_some_and(|subdomain| subdomain.is_empty() || subdomain.ends_with('.'))
}

/// "[user[:password]@]host[:port]" as (user, host, port).
fn split_authority(authority: &str) -> (Option<String>, String, Option<String>) {
    let (user, authority) = match authority.rfind('@') {
        Some(at) => {
            let user = &authority[..at];
            let user = user.split_once(':').map_or(user, |(user, _password)| user);
            (non_empty(user), &authority[at + 1..])
        }
        None => (None, authority),
    };
    if let Some(bracketed) = authority.strip_prefix('[') {
        // [::1]:22
        let (host, rest) = bracketed.split_once(']').unwrap_or((bracketed, ""));
        let port = rest.strip_prefix(':').and_then(non_empty);
        return (user, host.to_ascii_lowercase(), port);
    }
    let (host, port) = authority.split_once(':').unwrap_or((authority, ""));
    (user, host.to_ascii_lowercase(), non_empty(port))
}

/// The endpoint a remote url connects to; `None` for local paths, `file://` and `git://` urls.
#[must_use]
pub fn parse_remote_url(url: &str) -> Option<RemoteEndpoint> {
    let (transport, user, host, port) = if let Some((scheme, rest)) = url.split_once("://") {
        let transport = match scheme.to_ascii_lowercase().as_str() {
            "https" | "http" => Transport::Https,
            "ssh" | "git+ssh" | "ssh+git" => Transport::Ssh,
            _ => return None,
        };
        let authority = rest.split('/').next().unwrap_or(rest);
        let (user, host, port) = split_authority(authority);
        (transport, user, host, port)
    } else {
        // scp-style: [user@]host:path, where the colon comes before any slash (and is not a drive letter).
        let colon = url.find(':')?;
        if colon < 2 || url.find(['/', '\\']).is_some_and(|slash| slash < colon) {
            return None;
        }
        let authority = &url[..colon];
        let (user, host) = match authority.rfind('@') {
            Some(at) => (non_empty(&authority[..at]), &authority[at + 1..]),
            None => (None, authority),
        };
        (Transport::Ssh, user, host.to_ascii_lowercase(), None)
    };
    let safe = !host.is_empty()
        && is_safe_component(&host)
        && user.as_deref().is_none_or(is_safe_component);
    safe.then_some(RemoteEndpoint {
        transport,
        host,
        user,
        port,
    })
}

/// A user or host that can be handed to `ssh` / `git credential` as it is: it does not start with `-` (which
/// ssh would read as an option) and holds no white space or control character (which would split the argument
/// or add lines to the credential protocol).
fn is_safe_component(text: &str) -> bool {
    !text.starts_with('-') && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// A hosting service ImGit always lists, whether or not a repository uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KnownService {
    pub name: &'static str,
    /// Its HTTPS host (the credential helper's key).
    pub host: &'static str,
    /// Where its SSH endpoint answers.
    pub ssh_host: &'static str,
}

const AZURE_DEVOPS: KnownService = KnownService {
    name: "Azure DevOps",
    host: "dev.azure.com",
    ssh_host: "ssh.dev.azure.com",
};

static KNOWN_SERVICES: [KnownService; 4] = [
    KnownService {
        name: "GitHub",
        host: "github.com",
        ssh_host: "github.com",
    },
    KnownService {
        name: "GitLab",
        host: "gitlab.com",
        ssh_host: "gitlab.com",
    },
    KnownService {
        name: "Bitbucket",
        host: "bitbucket.org",
        ssh_host: "bitbucket.org",
    },
    AZURE_DEVOPS,
];

/// The services, in the order the page lists them.
#[must_use]
pub fn known_services() -> &'static [KnownService] {
    &KNOWN_SERVICES
}

/// The known service `host` belongs to (its HTTPS or SSH host, or a subdomain such as `*.visualstudio.com`).
#[must_use]
pub fn service_of(host: &str) -> Option<&'static KnownService> {
    let host = host.to_ascii_lowercase();
    KNOWN_SERVICES
        .iter()
        .find(|service| is_in_domain(&host, service.host) || is_in_domain(&host, service.ssh_host))
        // Azure DevOps' older hosts: <org>.visualstudio.com and vs-ssh.visualstudio.com.
        .or_else(|| is_in_domain(&host, "visualstudio.com").then_some(&KNOWN_SERVICES[3]))
}

/// What `ssh -T` printed and returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshAnswer {
    /// The key is accepted; as whom, when the service says.
    Authenticated { user: Option<String> },
    /// Refused or failed; why, in a line.
    Refused { detail: String },
}

/// What ssh says when it fails, and what the page says then.
const SSH_FAILURES: [(&str, &str); 8] = [
    ("permission denied", "The server refused your SSH key"),
    (
        "host key verification failed",
        "The host is not in known_hosts yet: connect once with ssh to trust its key",
    ),
    (
        "remote host identification has changed",
        "The host's key changed since it was trusted",
    ),
    ("could not resolve hostname", "Could not resolve the host"),
    ("timed out", "The connection timed out"),
    ("connection refused", "The connection was refused"),
    ("no route to host", "No route to the host"),
    ("connection closed", "The server closed the connection"),
];

/// The trimmed text after `open` up to the first of the `close` characters (or the end), in `text`; `lowered` is
/// `text` in lower case, where `open` is looked for. `None` when there is none.
fn between(text: &str, lowered: &str, open: &str, close: &[char]) -> Option<String> {
    let start = lowered.find(open)? + open.len();
    let rest = &text[start..];
    non_empty(trim(&rest[..rest.find(close).unwrap_or(rest.len())]))
}

/// Reads the answer to `ssh -T <target>`: the services greet an accepted key each in their own words, and none
/// of them gives a shell (so the exit code alone says little).
#[must_use]
pub fn parse_ssh_answer(output: &str, exit_code: i32) -> SshAnswer {
    // Lower case byte for byte, so positions found in one are positions in the other.
    let lowered = output.to_ascii_lowercase();
    let has = |needle: &str| lowered.contains(needle);
    let authenticated = |user: Option<String>| SshAnswer::Authenticated { user };

    // The services' greetings: GitHub "Hi <user>! You've successfully authenticated...", Gitea / Forgejo
    // "Hi there, <user>! ...", GitLab "Welcome to GitLab, @<user>!", Bitbucket "authenticated via ssh key" (and
    // formerly "logged in as <user>."), Azure DevOps "Shell access is not supported."
    if let Some(at) = lowered.find("successfully authenticated") {
        // Only the greeting's own line (ssh's warnings may come before it).
        let from = lowered[..at].rfind('\n').map_or(0, |newline| newline + 1);
        let (line, line_lowered) = (&output[from..at], &lowered[from..at]);
        let user = ["hi there, ", "hi "]
            .into_iter()
            .find(|greeting| line_lowered.starts_with(greeting))
            .and_then(|greeting| between(line, line_lowered, greeting, &['!']));
        return authenticated(user);
    }
    if has("welcome to gitlab") {
        return authenticated(between(output, &lowered, ", @", &['!', '\r', '\n']));
    }
    if has("logged in as ") {
        let user = between(output, &lowered, "logged in as ", &['\r', '\n'])
            .map(|user| user.strip_suffix('.').unwrap_or(&user).to_owned())
            .filter(|user| !user.is_empty());
        return authenticated(user);
    }
    if has("authenticated via")
        || has("shell access is not supported")
        || has("shell access is disabled")
    {
        return authenticated(None);
    }

    if let Some((_, detail)) = SSH_FAILURES.iter().find(|(needle, _)| has(needle)) {
        return SshAnswer::Refused {
            detail: (*detail).to_owned(),
        };
    }
    if exit_code == 0 {
        return authenticated(None);
    }
    // The last line ssh printed, as is.
    let last_line = output.split('\n').map(trim).rfind(|line| !line.is_empty());
    SshAnswer::Refused {
        detail: last_line.map_or_else(
            || format!("ssh exited with code {exit_code}"),
            str::to_owned,
        ),
    }
}

/// The `username=` of `git credential fill`'s answer; `None` when it has none (or an empty one). The password
/// in it is ignored.
#[must_use]
pub fn credential_username(fill_output: &str) -> Option<String> {
    fill_output
        .split('\n')
        .find_map(|line| {
            line.strip_suffix('\r')
                .unwrap_or(line)
                .strip_prefix("username=")
        })
        .and_then(non_empty)
}

/// Where one check of a connection stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Check {
    /// Not looked at yet.
    #[default]
    Unchecked,
    /// Running in the background.
    Checking,
    Connected {
        /// As whom, when that is known.
        user: Option<String>,
        /// How, in a line.
        detail: String,
    },
    NotConnected {
        /// Why not, in a line.
        detail: String,
    },
}

impl Check {
    fn connected(user: Option<String>, detail: &str) -> Self {
        Self::Connected {
            user,
            detail: detail.to_owned(),
        }
    }

    fn not_connected(detail: impl Into<String>) -> Self {
        Self::NotConnected {
            detail: detail.into(),
        }
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    /// Whom the service knows this machine as.
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        match self {
            Self::Connected { user, .. } => user.as_deref(),
            _ => None,
        }
    }

    /// The line under the state; `None` while there is nothing to say.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Connected { detail, .. } | Self::NotConnected { detail } => Some(detail),
            Self::Unchecked | Self::Checking => None,
        }
    }
}

/// Where a host's SSH endpoint is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SshTarget {
    /// `user@host`
    pub target: String,
    /// `None` for the default.
    pub port: Option<String>,
}

/// One row of the page: a host and how this machine stands with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// "GitHub", or the host for other servers.
    pub name: String,
    /// The HTTPS host.
    pub host: String,
    /// Listed for HTTPS (known services, hosts an https remote uses).
    pub https: bool,
    /// Listed for SSH (known services, hosts an ssh remote uses): where its endpoint is.
    pub ssh: Option<SshTarget>,
    /// The HTTPS user came from the credentials entered this session.
    pub session_credential: bool,
    pub https_check: Check,
    pub ssh_check: Check,
    /// The open repositories with a remote here.
    pub repos: Vec<String>,
}

/// The checks a list runs: [`checks::check_https`] and [`checks::check_ssh`], or stand-ins.
#[derive(Debug, Clone, Copy)]
struct Checker {
    https: fn(&str) -> Check,
    ssh: fn(&SshTarget) -> Check,
}

/// The connections the page lists and their checks, which run in the background. Checks still running when the
/// list is dropped finish on their own threads, so closing ImGit never waits on ssh.
#[derive(Debug)]
pub struct Connections {
    list: Vec<Connection>,
    /// The running checks, by host.
    https_jobs: HashMap<String, JoinHandle<Check>>,
    ssh_jobs: HashMap<String, JoinHandle<Check>>,
    checker: Checker,
}

impl Default for Connections {
    fn default() -> Self {
        Self::new()
    }
}

impl Connections {
    /// An empty list: [`update_hosts`](Self::update_hosts) fills it.
    #[must_use]
    pub fn new() -> Self {
        Self::with_checker(Checker {
            https: checks::check_https,
            ssh: checks::check_ssh,
        })
    }

    fn with_checker(checker: Checker) -> Self {
        Self {
            list: Vec::new(),
            https_jobs: HashMap::new(),
            ssh_jobs: HashMap::new(),
            checker,
        }
    }

    /// Lists the known services, then the other hosts the open repositories' remotes point at.
    /// `remotes`: (repository name, remote url) pairs. The checks so far carry over.
    pub fn update_hosts<R: AsRef<str>, U: AsRef<str>>(&mut self, remotes: &[(R, U)]) {
        let mut list: Vec<Connection> = KNOWN_SERVICES
            .iter()
            .map(|service| Connection {
                name: service.name.to_owned(),
                host: service.host.to_owned(),
                https: true,
                ssh: Some(SshTarget {
                    target: format!("git@{}", service.ssh_host),
                    port: None,
                }),
                session_credential: false,
                https_check: Check::Unchecked,
                ssh_check: Check::Unchecked,
                repos: Vec::new(),
            })
            .collect();
        for (repo, url) in remotes {
            let Some(endpoint) = parse_remote_url(url.as_ref()) else {
                continue;
            };
            let service = service_of(&endpoint.host);
            let host = service.map_or(endpoint.host.as_str(), |service| service.host);
            let index = list.iter().position(|c| c.host == host).unwrap_or_else(|| {
                list.push(Connection {
                    name: host.to_owned(),
                    host: host.to_owned(),
                    https: false,
                    ssh: None,
                    session_credential: false,
                    https_check: Check::Unchecked,
                    ssh_check: Check::Unchecked,
                    repos: Vec::new(),
                });
                list.len() - 1
            });
            let connection = &mut list[index];
            if service.is_none() {
                match endpoint.transport {
                    Transport::Https => connection.https = true,
                    Transport::Ssh => {
                        connection.ssh = Some(SshTarget {
                            target: format!("{}@{host}", endpoint.user.as_deref().unwrap_or("git")),
                            port: endpoint.port.clone(),
                        });
                    }
                }
            }
            let repo = repo.as_ref();
            if !connection.repos.iter().any(|listed| listed == repo) {
                connection.repos.push(repo.to_owned());
            }
        }
        // The checks so far carry over (an SSH check only while it is for the same target).
        for connection in &mut list {
            if let Some(old) = self.find(&connection.host) {
                connection.https_check = old.https_check.clone();
                connection.session_credential = old.session_credential;
                if old.ssh == connection.ssh {
                    connection.ssh_check = old.ssh_check.clone();
                }
            }
        }
        self.list = list;
    }

    /// Starts the checks that have not run (`force`: every check again). A host with a credential entered this
    /// session is connected as that user without asking the credential helper.
    pub fn check(&mut self, session_credentials: &[Credential], force: bool) {
        let checker = self.checker;
        for connection in &mut self.list {
            if connection.https {
                if let Some(credential) = session_credential(session_credentials, &connection.host)
                {
                    connection.session_credential = true;
                    connection.https_check = Check::connected(
                        non_empty(&credential.username),
                        if credential.remember {
                            "Entered this session and remembered"
                        } else {
                            "Entered this session"
                        },
                    );
                } else {
                    if connection.session_credential {
                        // Forgotten: look again for a stored one.
                        connection.session_credential = false;
                        connection.https_check = Check::Unchecked;
                    }
                    if (force || connection.https_check == Check::Unchecked)
                        && !self.https_jobs.contains_key(&connection.host)
                    {
                        connection.https_check = Check::Checking;
                        let host = connection.host.clone();
                        self.https_jobs.insert(
                            connection.host.clone(),
                            std::thread::spawn(move || (checker.https)(&host)),
                        );
                    }
                }
            }
            if let Some(target) = &connection.ssh
                && (force || connection.ssh_check == Check::Unchecked)
                && !self.ssh_jobs.contains_key(&connection.host)
            {
                connection.ssh_check = Check::Checking;
                let target = target.clone();
                self.ssh_jobs.insert(
                    connection.host.clone(),
                    std::thread::spawn(move || (checker.ssh)(&target)),
                );
            }
        }
    }

    /// Takes the finished checks' results.
    pub fn poll(&mut self) {
        for (host, result) in take_finished(&mut self.https_jobs) {
            if let Some(connection) = self.find_mut(&host)
                && !connection.session_credential
            {
                connection.https_check = result;
            }
        }
        for (host, result) in take_finished(&mut self.ssh_jobs) {
            if let Some(connection) = self.find_mut(&host) {
                connection.ssh_check = result;
            }
        }
    }

    /// Whether a check is still running (or finished and waits for [`poll`](Self::poll)).
    #[must_use]
    pub fn checking(&self) -> bool {
        !self.https_jobs.is_empty() || !self.ssh_jobs.is_empty()
    }

    #[must_use]
    pub fn list(&self) -> &[Connection] {
        &self.list
    }

    /// The connection of an HTTPS host.
    #[must_use]
    pub fn find(&self, host: &str) -> Option<&Connection> {
        self.list.iter().find(|connection| connection.host == host)
    }

    pub fn find_mut(&mut self, host: &str) -> Option<&mut Connection> {
        self.list
            .iter_mut()
            .find(|connection| connection.host == host)
    }
}

/// The session credential for `host` (its url is `scheme://host[:port]`), if one was entered.
fn session_credential<'a>(credentials: &'a [Credential], host: &str) -> Option<&'a Credential> {
    credentials.iter().find(|credential| {
        parse_remote_url(&credential.url).is_some_and(|endpoint| endpoint.host == host)
    })
}

/// Removes the jobs that are done and returns what they found, by host.
fn take_finished(jobs: &mut HashMap<String, JoinHandle<Check>>) -> Vec<(String, Check)> {
    let finished: Vec<String> = jobs
        .iter()
        .filter(|(_, job)| job.is_finished())
        .map(|(host, _)| host.clone())
        .collect();
    finished
        .into_iter()
        .filter_map(|host| {
            let result = jobs
                .remove(&host)?
                .join()
                .unwrap_or_else(|_| Check::not_connected("The check failed"));
            Some((host, result))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_url_https() {
        let e = parse_remote_url("https://me@GitHub.com:8443/owner/repo.git").unwrap();
        assert_eq!(e.transport, Transport::Https);
        assert_eq!(e.host, "github.com");
        assert_eq!(e.user.as_deref(), Some("me"));
        assert_eq!(e.port.as_deref(), Some("8443"));

        // A password in the url is not part of the user.
        let e = parse_remote_url("HTTP://me:secret@host.example/x").unwrap();
        assert_eq!(e.transport, Transport::Https);
        assert_eq!(
            (e.user.as_deref(), e.host.as_str(), e.port),
            (Some("me"), "host.example", None)
        );
        // No path at all (a credential's url).
        assert_eq!(
            parse_remote_url("https://Example.com").unwrap().host,
            "example.com"
        );
    }

    #[test]
    fn remote_url_ssh() {
        let scp = parse_remote_url("git@gitlab.com:group/repo.git").unwrap();
        assert_eq!(scp.transport, Transport::Ssh);
        assert_eq!(scp.host, "gitlab.com");
        assert_eq!(scp.user.as_deref(), Some("git"));

        let url: RemoteEndpoint = "ssh://git@ssh.github.com:443/owner/repo.git"
            .parse()
            .unwrap();
        assert_eq!(url.transport, Transport::Ssh);
        assert_eq!(url.host, "ssh.github.com");
        assert_eq!(url.port.as_deref(), Some("443"));

        let bare = parse_remote_url("myserver:repos/x.git").unwrap();
        assert_eq!(bare.host, "myserver");
        assert_eq!(bare.user, None);

        let v6 = parse_remote_url("ssh://me@[::1]:2222/x.git").unwrap();
        assert_eq!(
            (v6.host.as_str(), v6.port.as_deref()),
            ("::1", Some("2222"))
        );
        assert_eq!(
            parse_remote_url("git+ssh://[FE80::1]/x.git").unwrap().port,
            None
        );
    }

    #[test]
    fn remote_url_local() {
        for url in [
            "C:\\repos\\x",
            "C:/repos/x",
            "/srv/git/x.git",
            "../x.git",
            "file:///srv/git/x.git",
            "git://example.com/x.git",
            "https:///x.git",
            "x",
            "",
            // Option injection into ssh, and extra lines for `git credential`.
            "ssh://-oProxyCommand=cmd@host/x",
            "-oProxyCommand=cmd@host:x",
            "ssh://git@-oProxyCommand=cmd/x",
            "-host:x",
            "https://host\nusername=evil/x",
            "ssh://a b@host/x",
            "https://host\r/x",
        ] {
            assert_eq!(parse_remote_url(url), None, "{url}");
            assert_eq!(url.parse::<RemoteEndpoint>(), Err(NotARemoteUrl));
        }
    }

    #[test]
    fn services_hosts() {
        assert!(service_of("github.com").is_some());
        assert_eq!(service_of("ssh.github.com").unwrap().name, "GitHub");
        assert_eq!(service_of("GitLab.com").unwrap().name, "GitLab");
        assert_eq!(
            service_of("ssh.dev.azure.com").unwrap().name,
            "Azure DevOps"
        );
        assert_eq!(
            service_of("myorg.visualstudio.com").unwrap().name,
            "Azure DevOps"
        );
        assert_eq!(service_of("notgithub.com"), None);
        assert_eq!(service_of("git.example.com"), None);
        assert_eq!(known_services()[3], AZURE_DEVOPS);
    }

    fn authenticated(user: &str) -> SshAnswer {
        SshAnswer::Authenticated {
            user: non_empty(user),
        }
    }

    fn refused(detail: &str) -> SshAnswer {
        SshAnswer::Refused {
            detail: detail.to_owned(),
        }
    }

    #[test]
    fn ssh_answer_greetings() {
        let a = parse_ssh_answer(
            "Warning: Permanently added 'github.com' to the list of known hosts.\r\n\
             Hi octo-cat! You've successfully authenticated, but GitHub does not provide shell access.\r\n",
            1,
        );
        assert_eq!(a, authenticated("octo-cat"));

        let a = parse_ssh_answer("Welcome to GitLab, @someone!\n", 0);
        assert_eq!(a, authenticated("someone"));

        let a = parse_ssh_answer(
            "Hi there, gitea_user! You've successfully authenticated with the key named k\n",
            1,
        );
        assert_eq!(a, authenticated("gitea_user"));

        let a = parse_ssh_answer(
            "logged in as bb-user.\n\nYou can use git or hg to connect to Bitbucket.\n",
            0,
        );
        assert_eq!(a, authenticated("bb-user"));

        let a = parse_ssh_answer("remote: Shell access is not supported.\n", 1);
        assert_eq!(a, authenticated(""));

        // A greeting that names nobody, and no greeting at all.
        assert_eq!(
            parse_ssh_answer("You've successfully authenticated.\n", 1),
            authenticated("")
        );
        assert_eq!(
            parse_ssh_answer("authenticated via ssh key.\n", 0),
            authenticated("")
        );
        assert_eq!(parse_ssh_answer("", 0), authenticated(""));
    }

    #[test]
    fn ssh_answer_failures() {
        let a = parse_ssh_answer("git@github.com: Permission denied (publickey).\r\n", 255);
        assert_eq!(a, refused("The server refused your SSH key"));

        let a = parse_ssh_answer("Host key verification failed.\n", 255);
        assert!(matches!(a, SshAnswer::Refused { .. }));

        let a = parse_ssh_answer(
            "something odd\nkex_exchange_identification: read: Connection reset\n",
            255,
        );
        assert_eq!(
            a,
            refused("kex_exchange_identification: read: Connection reset")
        );

        assert_eq!(
            parse_ssh_answer(" \r\n", 3),
            refused("ssh exited with code 3")
        );
        // A failure ssh names wins over a clean exit code.
        assert_eq!(
            parse_ssh_answer("ssh: connect to host x port 22: Connection refused", 0),
            refused("The connection was refused")
        );
    }

    #[test]
    fn credential_usernames() {
        assert_eq!(
            credential_username(
                "protocol=https\r\nhost=github.com\r\nusername=alice\r\npassword=x\r\n"
            )
            .as_deref(),
            Some("alice")
        );
        assert_eq!(
            credential_username("protocol=https\nhost=github.com\n"),
            None
        );
        assert_eq!(credential_username("username=\npassword=x\n"), None);
        assert_eq!(credential_username("username=a=b"), Some("a=b".to_owned()));
    }

    // The list, with stand-ins for git and ssh.

    fn stored_https(host: &str) -> Check {
        if host == "github.com" {
            Check::connected(
                Some("stored-user".to_owned()),
                "Stored by your Git credential helper",
            )
        } else {
            Check::not_connected("No stored username or token")
        }
    }

    fn accepting_ssh(target: &SshTarget) -> Check {
        Check::connected(
            Some(format!(
                "{}:{}",
                target.target,
                target.port.as_deref().unwrap_or("22")
            )),
            "Your SSH key is accepted",
        )
    }

    fn connections() -> Connections {
        Connections::with_checker(Checker {
            https: stored_https,
            ssh: accepting_ssh,
        })
    }

    fn finish(connections: &mut Connections) {
        while connections.checking() {
            std::thread::yield_now();
            connections.poll();
        }
    }

    fn credential(url: &str, username: &str, remember: bool) -> Credential {
        Credential {
            url: url.to_owned(),
            username: username.to_owned(),
            password: "secret".to_owned(),
            remember,
        }
    }

    #[test]
    fn hosts_listed() {
        let mut connections = connections();
        assert!(connections.list().is_empty());
        connections.update_hosts(&[
            ("imgit", "git@github.com:me/imgit.git"),
            ("imgit", "https://github.com/me/imgit.git"),
            ("legacy", "https://myorg.visualstudio.com/p/_git/legacy"),
            ("work", "ssh://deploy@Git.Example.com:2222/srv/work.git"),
            ("site", "https://git.example.com/site.git"),
            ("web", "https://www.example.org/web.git"),
            ("local", "/srv/git/local.git"),
        ]);
        let names: Vec<&str> = connections.list().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "GitHub",
                "GitLab",
                "Bitbucket",
                "Azure DevOps",
                "git.example.com",
                "www.example.org"
            ]
        );

        let github = connections.find("github.com").unwrap();
        assert!(github.https);
        assert_eq!(github.ssh.as_ref().unwrap().target, "git@github.com");
        assert_eq!(github.repos, ["imgit"]);
        let azure = connections.find("dev.azure.com").unwrap();
        assert_eq!(azure.ssh.as_ref().unwrap().target, "git@ssh.dev.azure.com");
        assert_eq!(azure.repos, ["legacy"]);

        let example = connections.find("git.example.com").unwrap();
        assert!(example.https);
        assert_eq!(
            example.ssh,
            Some(SshTarget {
                target: "deploy@git.example.com".to_owned(),
                port: Some("2222".to_owned()),
            })
        );
        assert_eq!(example.repos, ["work", "site"]);
        let web = connections.find("www.example.org").unwrap();
        assert!(web.https);
        assert_eq!(web.ssh, None);
        assert!(connections.find("gitlab.com").unwrap().repos.is_empty());
    }

    #[test]
    fn checks_run_once_and_carry_over() {
        let mut connections = connections();
        connections.update_hosts(&[("work", "git@git.example.com:work.git")]);
        assert!(!connections.checking());
        connections.check(&[], false);
        assert!(connections.checking());
        assert_eq!(
            connections.find("gitlab.com").unwrap().https_check,
            Check::Checking
        );
        assert_eq!(
            connections.find("gitlab.com").unwrap().ssh_check,
            Check::Checking
        );
        // Not listed for HTTPS: not checked for it.
        assert_eq!(
            connections.find("git.example.com").unwrap().https_check,
            Check::Unchecked
        );
        finish(&mut connections);

        let github = connections.find("github.com").unwrap();
        assert!(github.https_check.is_connected());
        assert_eq!(github.https_check.user(), Some("stored-user"));
        assert_eq!(
            github.https_check.detail(),
            Some("Stored by your Git credential helper")
        );
        assert_eq!(github.ssh_check.user(), Some("git@github.com:22"));
        let gitlab = connections.find("gitlab.com").unwrap();
        assert_eq!(
            gitlab.https_check,
            Check::not_connected("No stored username or token")
        );
        assert_eq!(gitlab.https_check.user(), None);
        let example = connections.find("git.example.com").unwrap();
        assert_eq!(example.ssh_check.user(), Some("git@git.example.com:22"));

        // Checked already: nothing to do, until forced.
        connections.check(&[], false);
        assert!(!connections.checking());
        connections.check(&[], true);
        assert!(connections.checking());
        finish(&mut connections);

        // The results carry over to the next list; an SSH check only while it is for the same target.
        connections.update_hosts(&[("work", "ssh://deploy@git.example.com:2222/work.git")]);
        assert!(
            connections
                .find("github.com")
                .unwrap()
                .https_check
                .is_connected()
        );
        assert!(
            connections
                .find("github.com")
                .unwrap()
                .ssh_check
                .is_connected()
        );
        assert_eq!(
            connections.find("git.example.com").unwrap().ssh_check,
            Check::Unchecked
        );
        connections.check(&[], false);
        finish(&mut connections);
        assert_eq!(
            connections
                .find("git.example.com")
                .unwrap()
                .ssh_check
                .user(),
            Some("deploy@git.example.com:2222")
        );
    }

    #[test]
    fn session_credentials_win() {
        let mut connections = connections();
        connections.update_hosts::<&str, &str>(&[]);
        let entered = [
            credential("https://github.com", "typed-user", false),
            credential("https://gitlab.com:443", "", true),
        ];
        connections.check(&entered, false);
        let github = connections.find("github.com").unwrap();
        assert!(github.session_credential);
        assert_eq!(
            github.https_check,
            Check::connected(Some("typed-user".to_owned()), "Entered this session")
        );
        assert_eq!(
            connections.find("gitlab.com").unwrap().https_check,
            Check::connected(None, "Entered this session and remembered")
        );
        finish(&mut connections);
        assert_eq!(
            connections.find("github.com").unwrap().https_check.user(),
            Some("typed-user")
        );

        // Forgotten: the stored one is looked for again.
        connections.check(&[], false);
        let github = connections.find("github.com").unwrap();
        assert!(!github.session_credential);
        assert_eq!(github.https_check, Check::Checking);
        finish(&mut connections);
        assert_eq!(
            connections.find("github.com").unwrap().https_check.user(),
            Some("stored-user")
        );

        // What the page does after "forget": the row is looked up and reset.
        connections.find_mut("github.com").unwrap().https_check = Check::Unchecked;
        assert_eq!(
            connections.find("github.com").unwrap().https_check.detail(),
            None
        );
    }
}
