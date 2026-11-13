//! Network operations go through the user's `git` executable so the configured credential helpers and SSH
//! setup are honoured; libgit2 covers everything local.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use imgit_platform::process;

/// Marks the environment of a git started by [`run`]: ImGit launched with it set is git's askpass program.
const ASKPASS_MARKER: &str = "IMGIT_ASKPASS";
/// Carries [`run`]'s credentials to the askpass child.
const CREDENTIALS_VARIABLE: &str = "IMGIT_ASKPASS_CREDENTIALS";

/// What a finished `git` printed and returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// -1 when git could not be started.
    pub exit_code: i32,
    /// stdout + stderr, in order.
    pub text: String,
}

impl Output {
    #[must_use]
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// A username / password for one host (`scheme://host[:port]`), answered to git's prompts by ImGit itself
/// running as `GIT_ASKPASS`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credential {
    pub url: String,
    pub username: String,
    pub password: String,
    /// `false`: the configured credential helpers are skipped for the host (nothing stored).
    pub remember: bool,
}

/// What [`run`] takes besides the command itself.
#[derive(Default)]
pub struct RunOptions<'a> {
    /// Written to git's stdin (closed after it).
    pub input: &'a str,
    /// Answers to git's username / password prompts.
    pub credentials: &'a [Credential],
    /// Called from the running thread with each chunk of git's output as it arrives.
    pub on_output: Option<&'a mut dyn FnMut(&str)>,
}

/// Variables every [`run`] adds to git's environment (`GIT_SSH_COMMAND`...), asked for on the running thread
/// at each run; returns name / value pairs, an empty value meaning "leave it as inherited".
pub type EnvironmentProvider = Box<dyn Fn() -> Vec<(String, String)> + Send + Sync>;

type SharedProvider = Arc<dyn Fn() -> Vec<(String, String)> + Send + Sync>;

static ENVIRONMENT_PROVIDER: RwLock<Option<SharedProvider>> = RwLock::new(None);

/// Installs (or, with `None`, removes) the provider of the extra variables.
pub fn set_environment_provider(provider: Option<EnvironmentProvider>) {
    *ENVIRONMENT_PROVIDER
        .write()
        .unwrap_or_else(PoisonError::into_inner) = provider.map(Arc::from);
}

/// The provider's variables, asked for without holding the lock (it may well install another provider).
fn provided_environment() -> Vec<(String, String)> {
    let provider = ENVIRONMENT_PROVIDER
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    provider.map(|provider| provider()).unwrap_or_default()
}

/// The whole command line: a credential the user did not ask to remember keeps the helpers away from its
/// host (they would store it).
fn command_line<I, S>(args: I, credentials: &[Credential]) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut command = vec!["git".to_owned()];
    for credential in credentials.iter().filter(|credential| !credential.remember) {
        command.push("-c".to_owned());
        command.push(format!("credential.{}.helper=", credential.url));
    }
    command.extend(args.into_iter().map(|arg| arg.as_ref().to_owned()));
    command
}

/// The defaults with `provided` applied: its variables replace them (`GCM_INTERACTIVE`) or are added, an
/// empty value dropping the variable.
fn environment(
    askpass: String,
    credentials: &[Credential],
    provided: Vec<(String, String)>,
) -> Vec<(String, String)> {
    // Never block on a terminal prompt or a credential helper's window: git asks ImGit (its credentials
    // prompt), and the helpers still answer with what they have stored. GIT_ASKPASS is a bare path: git
    // executes it directly, not through a shell, so a quoted path (even one with a space) is "not found"
    // and every prompt fails with "could not read Username".
    let mut environment: Vec<(String, String)> = [
        ("GIT_TERMINAL_PROMPT", "0".to_owned()),
        ("GCM_INTERACTIVE", "never".to_owned()),
        ("GIT_ASKPASS", askpass),
        (ASKPASS_MARKER, "1".to_owned()),
        (CREDENTIALS_VARIABLE, encode(credentials)),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    for (name, value) in provided {
        environment.retain(|(existing, _)| *existing != name);
        if !value.is_empty() {
            environment.push((name, value));
        }
    }
    environment
}

/// Runs `git <args>` in `workdir` without a console window and waits for it to finish. Credential helpers
/// never show their own UI; git's username / password prompts are answered from `options.credentials`, and
/// fail (see [`auth_request`]) for a host that has none.
pub fn run<I, S>(workdir: &Path, args: I, options: RunOptions<'_>) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let command = command_line(args, options.credentials);
    let environment = environment(
        process::executable_path().to_string_lossy().into_owned(),
        options.credentials,
        provided_environment(),
    );
    // Reborrowed so the sink's lifetime can shrink to the local environment's.
    let on_output: Option<&mut dyn FnMut(&str)> = match options.on_output {
        Some(sink) => Some(sink),
        None => None,
    };
    let started = process::run_process(
        workdir,
        &command,
        process::RunOptions {
            input: options.input.as_bytes(),
            environment: &environment,
            on_output,
        },
    );
    match started {
        Ok(finished) => Output {
            exit_code: finished.exit_code,
            text: finished.text,
        },
        Err(_) => Output {
            exit_code: -1,
            text: if workdir.is_dir() {
                "could not start git (is it on PATH?)".to_owned()
            } else {
                format!("folder not found: {}", workdir.display())
            },
        },
    }
}

/// Whether `git` can be run at all; asked once.
#[must_use]
pub fn available() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| run(Path::new("."), ["--version"], RunOptions::default()).success())
}

/// Where a `git clone --progress` is: its latest phase line ("Receiving objects:  45% (123/273), 1.2 MiB | ...")
/// and how far along the whole clone is, 0..1 across the phases (counting, compressing, receiving objects,
/// resolving deltas, updating files).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CloneProgress {
    pub line: String,
    pub fraction: f32,
    /// The output after the last line break, waiting for the rest of its line.
    pending: String,
}

/// One of git's phases with the share of the bar it fills.
struct Phase {
    label: &'static str,
    from: f32,
    to: f32,
}

/// In order ("remote: " prefixes the server's).
const PHASES: [Phase; 7] = [
    Phase {
        label: "Enumerating objects",
        from: 0.0,
        to: 0.02,
    },
    Phase {
        label: "Counting objects",
        from: 0.02,
        to: 0.05,
    },
    Phase {
        label: "Compressing objects",
        from: 0.05,
        to: 0.1,
    },
    Phase {
        label: "Receiving objects",
        from: 0.1,
        to: 0.85,
    },
    Phase {
        label: "Resolving deltas",
        from: 0.85,
        to: 0.97,
    },
    Phase {
        label: "Updating files",
        from: 0.97,
        to: 1.0,
    },
    Phase {
        label: "Checking out files",
        from: 0.97,
        to: 1.0,
    },
];

impl CloneProgress {
    /// Feeds a chunk of the clone's output (split anywhere) in.
    pub fn feed(&mut self, chunk: &str) {
        self.pending.push_str(chunk);
        let pending = std::mem::take(&mut self.pending);
        // Progress lines overwrite each other with a bare '\r'.
        let mut lines = pending.split(['\r', '\n']);
        let unfinished = lines.next_back().unwrap_or_default();
        for line in lines {
            self.line(line);
        }
        self.pending = unfinished.to_owned();
    }

    fn line(&mut self, text: &str) {
        let text = text
            .strip_prefix("remote: ")
            .unwrap_or(text)
            .trim_end_matches(' ');
        if text.is_empty() {
            return;
        }
        if let Some(phase) = PHASES.iter().find(|phase| text.starts_with(phase.label)) {
            let within = phase_fraction(text);
            self.fraction = self
                .fraction
                .max(phase.from + (phase.to - phase.from) * within);
            self.line = text.to_owned();
        } else if text.starts_with("Cloning into") {
            self.line = text.to_owned();
        }
    }
}

/// How far a phase line says its phase is: "<label>: 45% (123/273)..." or "<label>: 273, done." (no
/// percentage: the phase is over).
fn phase_fraction(line: &str) -> f32 {
    let Some((before, _)) = line.split_once('%') else {
        return 1.0;
    };
    let digits = before.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    // No digits is 0%; more than fit is certainly past 100%.
    let percent: u8 = match &before[digits..] {
        "" => 0,
        number => number.parse().map_or(100, |value: u8| value.min(100)),
    };
    f32::from(percent) / 100.0
}

/// Feeds a chunk of the clone's output (split anywhere) into `progress`.
pub fn feed_clone_progress(progress: &mut CloneProgress, chunk: &str) {
    progress.feed(chunk);
}

/// The host part of a url, as credentials are keyed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialHost {
    /// `scheme://host[:port]`
    pub url: String,
    /// The url's `user@`, when it has one.
    pub username: Option<String>,
}

/// `scheme://host[:port]` of a url (and its `user@`, when it has one); `None` for scp-style ssh urls.
#[must_use]
pub fn credential_host(url: &str) -> Option<CredentialHost> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest
        .split_once('/')
        .map_or(rest, |(authority, _)| authority);
    let (username, host) = match authority.rsplit_once('@') {
        // "user:password@host": the username stops at the colon.
        Some((user_info, host)) => {
            let user = user_info
                .split_once(':')
                .map_or(user_info, |(user, _)| user);
            (Some(user.to_owned()), host)
        }
        None => (None, authority),
    };
    Some(CredentialHost {
        url: format!("{scheme}://{host}"),
        username,
    })
}

/// What a failed command's output says about credentials: the host that wanted them, the username in its
/// url, and whether the ones it was given were rejected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthRequest {
    pub url: String,
    pub username: Option<String>,
    pub rejected: bool,
}

/// The url quoted after `marker` in `text` ("... for '<https://host>': ...").
fn quoted_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let (_, after) = text.split_once(marker)?;
    let (quoted, _) = after.split_once('\'')?;
    Some(quoted)
}

/// Reads git's messages: "could not read Username for '<url>': terminal prompts disabled" when nobody
/// answered, "Authentication failed for '<url>'" when the answer was refused.
#[must_use]
pub fn auth_request(output: &str) -> Option<AuthRequest> {
    let refused = quoted_after(output, "Authentication failed for '");
    let url = refused
        .or_else(|| quoted_after(output, "could not read Username for '"))
        .or_else(|| quoted_after(output, "could not read Password for '"))?;
    let host = credential_host(url)?;
    Some(AuthRequest {
        url: host.url,
        username: host.username,
        rejected: refused.is_some(),
    })
}

/// The credentials travel to the askpass child as hex fields (url:username:password;...), so any byte is safe.
fn hex(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// `None` for anything [`hex`] does not produce.
fn unhex(text: &str) -> Option<String> {
    let nibble = |digit: u8| char::from(digit).to_digit(16);
    let bytes = text
        .as_bytes()
        .chunks(2)
        .map(|pair| match *pair {
            [high, low] => u8::try_from(nibble(high)? << 4 | nibble(low)?).ok(),
            _ => None,
        })
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

fn encode(credentials: &[Credential]) -> String {
    credentials
        .iter()
        .map(|credential| {
            format!(
                "{}:{}:{};",
                hex(&credential.url),
                hex(&credential.username),
                hex(&credential.password)
            )
        })
        .collect()
}

/// The credentials [`encode`] wrote; entries that are not its work are skipped.
fn decode(text: &str) -> Vec<Credential> {
    text.split(';')
        .filter_map(|entry| {
            let mut fields = entry.splitn(3, ':').map(unhex);
            Some(Credential {
                url: fields.next()??,
                username: fields.next()??,
                password: fields.next()??,
                remember: false,
            })
        })
        .collect()
}

/// ImGit started by git as its askpass program: [`run`] marks the environment.
#[must_use]
pub fn is_askpass_invocation() -> bool {
    std::env::var_os(ASKPASS_MARKER).is_some()
}

/// The answer to git's `prompt` ("Username for '...': " / "Password for '...': ") among `credentials`.
fn askpass_answer(prompt: &str, credentials: &[Credential]) -> Option<String> {
    let wants_username = prompt.starts_with("Username");
    if !wants_username && !prompt.starts_with("Password") {
        return None; // ssh's host key / passphrase questions are not ours
    }
    // The password prompt names the username git settled on ("Password for 'user@host'"), from the remote
    // url or our own answer; the host's credential answers it either way, since a remote url that carries
    // a user (https://me@github.com/...) never asks for one, whatever the sign-in bar was given.
    let host = credential_host(quoted_after(prompt, "for '")?)?.url;
    let credential = credentials
        .iter()
        .find(|credential| credential.url == host)?;
    Some(if wants_username {
        credential.username.clone()
    } else {
        credential.password.clone()
    })
}

/// Prints the answer to `prompt` for git; the process exit code (failure: no answer).
#[must_use]
pub fn askpass_main(prompt: &str) -> ExitCode {
    let encoded = std::env::var(CREDENTIALS_VARIABLE).unwrap_or_default();
    let Some(answer) = askpass_answer(prompt, &decode(&encoded)) else {
        return ExitCode::FAILURE;
    };
    let mut stdout = std::io::stdout().lock();
    match writeln!(stdout, "{answer}").and_then(|()| stdout.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(url: &str, username: &str, password: &str) -> Credential {
        Credential {
            url: url.to_owned(),
            username: username.to_owned(),
            password: password.to_owned(),
            remember: false,
        }
    }

    fn close(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() < 1e-5
    }

    #[test]
    fn clone_progress_survives_chunks_split_mid_line() {
        let mut progress = CloneProgress::default();
        feed_clone_progress(&mut progress, "Cloning into 'imgit'...\nremote: Enumer");
        assert_eq!(progress.line, "Cloning into 'imgit'...");
        assert!(close(progress.fraction, 0.0));

        feed_clone_progress(
            &mut progress,
            "ating objects: 273, done.\nremote: Counting obj",
        );
        assert_eq!(progress.line, "Enumerating objects: 273, done.");
        assert!(close(progress.fraction, 0.02));

        // The percentage itself is cut in two.
        feed_clone_progress(&mut progress, "ects:  5");
        assert_eq!(progress.line, "Enumerating objects: 273, done.");
        feed_clone_progress(&mut progress, "0% (136/273)        \r");
        assert_eq!(progress.line, "Counting objects:  50% (136/273)");
        assert!(close(progress.fraction, 0.035));
    }

    #[test]
    fn clone_progress_follows_carriage_return_updates() {
        let mut progress = CloneProgress::default();
        progress.feed(
            "Receiving objects:  10% (28/273)\rReceiving objects:  45% (123/273), 1.2 MiB | 2.0 MiB/s\rReceiving",
        );
        assert_eq!(
            progress.line,
            "Receiving objects:  45% (123/273), 1.2 MiB | 2.0 MiB/s"
        );
        assert!(close(progress.fraction, 0.1 + 0.75 * 0.45));

        progress.feed(" objects: 100% (273/273), 3.1 MiB | 2.0 MiB/s, done.\r\n");
        assert!(close(progress.fraction, 0.85));
        progress.feed("Resolving deltas:  50% (60/120)\r");
        assert!(close(progress.fraction, 0.91));
        progress
            .feed("Resolving deltas: 100% (120/120), done.\nUpdating files: 100% (40/40), done.\n");
        assert_eq!(progress.line, "Updating files: 100% (40/40), done.");
        assert!(close(progress.fraction, 1.0));
    }

    #[test]
    fn clone_progress_never_goes_back_and_ignores_other_lines() {
        let mut progress = CloneProgress::default();
        progress.feed("Receiving objects:  60% (6/10)\r");
        progress.feed("remote: Compressing objects: 100% (5/5), done.\n");
        // A late line of an earlier phase is shown but does not move the bar back.
        assert_eq!(progress.line, "Compressing objects: 100% (5/5), done.");
        assert!(close(progress.fraction, 0.1 + 0.75 * 0.6));

        progress.feed("warning: something else\n\n   \nremote: \n");
        assert_eq!(progress.line, "Compressing objects: 100% (5/5), done.");
        // A percentage past 100 is clamped.
        progress.feed("Resolving deltas: 250% (5/2)\n");
        assert!(close(progress.fraction, 0.97));
    }

    #[test]
    fn credential_host_cases() {
        let host = |url: &str, username: Option<&str>| {
            Some(CredentialHost {
                url: url.to_owned(),
                username: username.map(str::to_owned),
            })
        };
        assert_eq!(
            credential_host("https://github.com/owner/repo.git"),
            host("https://github.com", None)
        );
        assert_eq!(
            credential_host("https://me@github.com/owner/repo.git"),
            host("https://github.com", Some("me"))
        );
        assert_eq!(
            credential_host("https://me:secret@example.com:8443/repo"),
            host("https://example.com:8443", Some("me"))
        );
        assert_eq!(
            credential_host("http://localhost:3000"),
            host("http://localhost:3000", None)
        );
        assert_eq!(
            credential_host("ssh://git@example.com/repo.git"),
            host("ssh://example.com", Some("git"))
        );
        // scp-style ssh urls have no credentials of ours.
        assert_eq!(credential_host("git@github.com:owner/repo.git"), None);
        assert_eq!(credential_host(""), None);
    }

    #[test]
    fn auth_request_reads_gits_messages() {
        assert_eq!(
            auth_request(
                "fatal: could not read Username for 'https://github.com': terminal prompts disabled\n"
            ),
            Some(AuthRequest {
                url: "https://github.com".to_owned(),
                username: None,
                rejected: false,
            })
        );
        assert_eq!(
            auth_request(
                "fatal: could not read Password for 'https://me@example.com:8443': terminal prompts disabled\n"
            ),
            Some(AuthRequest {
                url: "https://example.com:8443".to_owned(),
                username: Some("me".to_owned()),
                rejected: false,
            })
        );
        assert_eq!(
            auth_request(
                "remote: Invalid username or password.\nfatal: Authentication failed for 'https://github.com/owner/repo.git/'\n"
            ),
            Some(AuthRequest {
                url: "https://github.com".to_owned(),
                username: None,
                rejected: true,
            })
        );
        // A rejection wins over a prompt nobody answered.
        let both = "fatal: could not read Username for 'https://a.example': terminal prompts disabled\n\
                    fatal: Authentication failed for 'https://b.example/repo'\n";
        assert_eq!(
            auth_request(both).map(|request| (request.url, request.rejected)),
            Some(("https://b.example".to_owned(), true))
        );
    }

    #[test]
    fn auth_request_ignores_everything_else() {
        assert_eq!(auth_request(""), None);
        assert_eq!(
            auth_request("fatal: repository 'https://github.com/owner/missing/' not found\n"),
            None
        );
        // No closing quote, and a url without a scheme.
        assert_eq!(
            auth_request("fatal: Authentication failed for 'https://github.com"),
            None
        );
        assert_eq!(
            auth_request("fatal: Authentication failed for 'github.com:owner/repo'"),
            None
        );
    }

    #[test]
    fn credentials_round_trip_through_the_environment_encoding() {
        let credentials = [
            credential("https://github.com", "me", "p:a;s's wörd"),
            credential("https://example.com:8443", "", ""),
        ];
        let encoded = encode(&credentials);
        assert!(
            encoded
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == ';')
        );
        assert_eq!(decode(&encoded), credentials);
        assert_eq!(decode(""), []);
        assert_eq!(decode("zz:00:00;6d65;"), []);
    }

    #[test]
    fn askpass_answers_from_the_hosts_credential() {
        let credentials = [
            credential("https://example.com", "other", "nope"),
            credential("https://github.com", "me", "secret"),
        ];
        let answer = |prompt: &str| askpass_answer(prompt, &credentials);
        assert_eq!(
            answer("Username for 'https://github.com': ").as_deref(),
            Some("me")
        );
        assert_eq!(
            answer("Password for 'https://me@github.com': ").as_deref(),
            Some("secret")
        );
        // The url's user does not pick the credential: the host does.
        assert_eq!(
            answer("Password for 'https://someone@github.com': ").as_deref(),
            Some("secret")
        );
        assert_eq!(answer("Username for 'https://gitlab.com': "), None);
        assert_eq!(
            answer("Enter passphrase for key '/home/me/.ssh/id_ed25519': "),
            None
        );
        assert_eq!(answer("Password: "), None);
    }

    #[test]
    fn unremembered_credentials_disable_the_helpers_for_their_host() {
        let credentials = [
            credential("https://github.com", "me", "secret"),
            Credential {
                remember: true,
                ..credential("https://example.com", "me", "secret")
            },
        ];
        assert_eq!(
            command_line(["fetch", "--all"], &credentials),
            [
                "git",
                "-c",
                "credential.https://github.com.helper=",
                "fetch",
                "--all"
            ]
        );
    }

    #[test]
    fn provided_variables_replace_or_drop_the_defaults() {
        let provided = vec![
            ("GCM_INTERACTIVE".to_owned(), "auto".to_owned()),
            ("GIT_TERMINAL_PROMPT".to_owned(), String::new()),
            ("GIT_SSH_COMMAND".to_owned(), "ssh -i key".to_owned()),
            ("GIT_TRACE".to_owned(), String::new()),
        ];
        let environment = environment("C:\\Program Files\\imgit.exe".to_owned(), &[], provided);
        let value = |name: &str| {
            let mut values = environment
                .iter()
                .filter(|(existing, _)| existing == name)
                .map(|(_, value)| value.as_str());
            let first = values.next();
            assert_eq!(values.next(), None, "{name} is set twice");
            first
        };
        assert_eq!(value("GCM_INTERACTIVE"), Some("auto"));
        assert_eq!(value("GIT_TERMINAL_PROMPT"), None);
        assert_eq!(value("GIT_SSH_COMMAND"), Some("ssh -i key"));
        assert_eq!(value("GIT_TRACE"), None);
        // A bare path, never quoted.
        assert_eq!(value("GIT_ASKPASS"), Some("C:\\Program Files\\imgit.exe"));
        assert_eq!(value(ASKPASS_MARKER), Some("1"));
        assert_eq!(value(CREDENTIALS_VARIABLE), Some(""));
    }

    #[test]
    fn run_git_version() {
        let mut streamed = String::new();
        let mut sink = |chunk: &str| streamed.push_str(chunk);
        let output = run(
            Path::new("."),
            ["--version"],
            RunOptions {
                on_output: Some(&mut sink),
                ..RunOptions::default()
            },
        );
        assert!(output.success(), "{output:?}");
        assert!(output.text.starts_with("git version "), "{output:?}");
        assert_eq!(streamed, output.text);
        assert!(available());
    }

    #[test]
    fn run_reports_a_failing_command() {
        let output = run(
            Path::new("."),
            ["definitely-not-a-git-command"],
            RunOptions::default(),
        );
        assert!(!output.success());
        assert_ne!(output.exit_code, -1);
        assert!(output.text.contains("definitely-not-a-git-command"));
    }

    #[test]
    fn run_in_a_missing_folder_cannot_start() {
        let missing = Path::new("this/folder/does/not/exist");
        let output = run(missing, ["--version"], RunOptions::default());
        assert_eq!(output.exit_code, -1);
        assert!(!output.success());
        assert_eq!(
            output.text,
            format!("folder not found: {}", missing.display())
        );
    }
}
