//! The process half of the connections: the HTTPS / SSH checks themselves. They run git / ssh and block, so
//! [`Connections`](super::Connections) runs them on background threads.

use std::path::Path;

use imgit_git::cli;
use imgit_platform::process;

use super::{Check, SshAnswer, SshTarget, credential_username, parse_ssh_answer};

/// No working directory: the checks do not depend on one.
fn anywhere() -> &'static Path {
    Path::new("")
}

/// What `git credential fill` / `reject` reads for `host`.
fn credential_request(host: &str, username: Option<&str>) -> String {
    match username {
        Some(username) => format!("protocol=https\nhost={host}\nusername={username}\n\n"),
        None => format!("protocol=https\nhost={host}\n\n"),
    }
}

/// The `ssh` command line that asks `target` who this machine is.
fn ssh_command(target: &SshTarget) -> Vec<String> {
    // BatchMode: no passphrase or host-key prompt, so an unknown host or a locked key reports instead of waiting.
    let mut args = vec!["ssh", "-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=6"];
    if let Some(port) = &target.port {
        args.extend(["-p", port]);
    }
    args.push(&target.target);
    args.into_iter().map(str::to_owned).collect()
}

/// Whether the configured credential helper has a username / token for `host`.
#[must_use]
pub fn check_https(host: &str) -> Check {
    // `git credential fill` asks the configured helpers (never interactively: cli::run turns their prompts off)
    // and, when none has the host, ImGit's askpass, which has nothing for it and fails.
    let output = cli::run(
        anywhere(),
        ["credential", "fill"],
        cli::RunOptions {
            input: &credential_request(host, None),
            ..cli::RunOptions::default()
        },
    );
    let user = if output.exit_code == 0 {
        credential_username(&output.text)
    } else {
        None
    };
    match user {
        Some(user) => Check::connected(Some(user), "Stored by your Git credential helper"),
        None => Check::not_connected("No stored username or token"),
    }
}

/// Whether the service at `target` accepts one of this machine's SSH keys.
#[must_use]
pub fn check_ssh(target: &SshTarget) -> Check {
    let Ok(output) = process::run_process(
        anywhere(),
        &ssh_command(target),
        process::RunOptions::default(),
    ) else {
        return Check::not_connected("Could not start ssh (is OpenSSH on PATH?)");
    };
    match parse_ssh_answer(&output.text, output.exit_code) {
        SshAnswer::Authenticated { user } => Check::connected(user, "Your SSH key is accepted"),
        SshAnswer::Refused { detail } => Check::not_connected(detail),
    }
}

/// Removes `host`'s HTTPS credential from the credential helper (`git credential reject`).
pub fn forget_stored_credential(host: &str, username: &str) {
    // Nothing to report: a helper that had nothing stored says nothing either.
    let _ = cli::run(
        anywhere(),
        ["credential", "reject"],
        cli::RunOptions {
            input: &credential_request(host, Some(username)),
            ..cli::RunOptions::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_requests() {
        assert_eq!(
            credential_request("github.com", None),
            "protocol=https\nhost=github.com\n\n"
        );
        assert_eq!(
            credential_request("github.com", Some("alice")),
            "protocol=https\nhost=github.com\nusername=alice\n\n"
        );
    }

    #[test]
    fn ssh_command_lines() {
        let default_port = SshTarget {
            target: "git@github.com".to_owned(),
            port: None,
        };
        assert_eq!(
            ssh_command(&default_port),
            [
                "ssh",
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=6",
                "git@github.com"
            ]
        );
        let other_port = SshTarget {
            target: "deploy@git.example.com".to_owned(),
            port: Some("2222".to_owned()),
        };
        assert_eq!(
            ssh_command(&other_port)[6..],
            ["-p", "2222", "deploy@git.example.com"]
        );
    }
}
