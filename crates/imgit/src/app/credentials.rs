//! The credentials prompt: a git command refused for want of a username / password waits here for the user,
//! then `retry` runs it again with what they entered.

use imgit_git::cli::{self, Credential};

use crate::app::action::CredentialAction;
use crate::app::application::Application;

/// The prompt bar's state. The credentials dialog (`ui::panels::dialogs`) draws it.
pub struct CredentialPrompt {
    /// `scheme://host`.
    pub url: String,
    pub username: String,
    pub password: String,
    pub remember: bool,
    /// "Authentication failed. ..."
    pub rejected: bool,
    pub just_opened: bool,
    /// Runs the command again with the credential entered (taken out when the user confirms).
    pub retry: Option<CredentialAction>,
}

impl Application {
    /// Opens the prompt when a failed command's `output` asked for credentials. `false` otherwise (or when a
    /// prompt is already open). `attempt` is what that run was given; `retry` runs the command again.
    pub fn prompt_for_credentials(
        &mut self,
        output: &str,
        attempt: Option<&Credential>,
        retry: CredentialAction,
    ) -> bool {
        let Some(request) = cli::auth_request(output) else {
            return false;
        };
        if self.credential_prompt.is_some() {
            return false;
        }
        let retried = attempt.filter(|attempt| attempt.url == request.url);
        self.credential_prompt = Some(CredentialPrompt {
            url: request.url,
            username: retried.map_or_else(
                || request.username.clone().unwrap_or_default(),
                |attempt| attempt.username.clone(),
            ),
            password: String::new(),
            remember: retried.is_some_and(|attempt| attempt.remember),
            rejected: request.rejected || retried.is_some(),
            just_opened: true,
            retry: Some(retry),
        });
        true
    }

    /// Keeps a credential git accepted for the session (replacing its host's).
    pub fn keep_credential(&mut self, credential: &Credential) {
        self.credentials.retain(|other| other.url != credential.url);
        self.credentials.push(credential.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application() -> (tempfile::TempDir, Application) {
        let scratch = tempfile::tempdir().unwrap();
        let app = Application::with_config_directory(scratch.path().join("config"));
        (scratch, app)
    }

    const DENIED: &str =
        "fatal: could not read Username for 'https://github.com': terminal prompts disabled";

    #[test]
    fn a_refused_command_opens_the_prompt_once() {
        let (_scratch, mut app) = application();
        assert!(!app.prompt_for_credentials("nothing to see", None, Box::new(|_, _| {})));
        assert!(app.credential_prompt.is_none());
        assert!(app.prompt_for_credentials(DENIED, None, Box::new(|_, _| {})));
        let prompt = app.credential_prompt.as_ref().unwrap();
        assert_eq!(prompt.url, "https://github.com");
        assert!(!prompt.rejected && prompt.just_opened && prompt.retry.is_some());
        assert!(!app.prompt_for_credentials(DENIED, None, Box::new(|_, _| {})));
    }

    #[test]
    fn a_rejected_attempt_is_offered_again_with_its_username() {
        let (_scratch, mut app) = application();
        let attempt = Credential {
            url: "https://github.com".to_owned(),
            username: "me".to_owned(),
            password: "wrong".to_owned(),
            remember: true,
        };
        assert!(app.prompt_for_credentials(DENIED, Some(&attempt), Box::new(|_, _| {})));
        let prompt = app.credential_prompt.as_ref().unwrap();
        assert!(prompt.rejected && prompt.remember);
        assert_eq!(prompt.username, "me");
        assert!(prompt.password.is_empty());
    }

    #[test]
    fn kept_credentials_replace_their_host() {
        let (_scratch, mut app) = application();
        let credential = |user: &str| Credential {
            url: "https://github.com".to_owned(),
            username: user.to_owned(),
            ..Credential::default()
        };
        app.keep_credential(&credential("a"));
        app.keep_credential(&credential("b"));
        assert_eq!(app.credentials.len(), 1);
        assert_eq!(app.credentials[0].username, "b");
    }
}
