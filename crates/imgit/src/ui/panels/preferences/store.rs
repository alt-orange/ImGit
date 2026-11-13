//! What the pages show and write besides the preferences file: the git configuration (global `.gitconfig` and
//! the open repository's `.git/config`), the signing keys, SSH / GPG key generation, the Gitflow and commit
//! template drafts. Also the panel's [`State`] (the C++ file-level statics).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use imgit_app::preferences::{pref, preferences};
use imgit_git::Author;
use imgit_git::cli;
use imgit_git::repository::{global_config, set_global_config};
use imgit_platform::process::{self, RunOptions};

use crate::app::action::Completion;
use crate::app::application::Application;
use crate::app::toasts::ToastKind;
use crate::ui::Vec2;

/// Where a setting lives in the git configuration: the global .gitconfig, or the open repository's .git/config
/// (Repo-Specific Preferences).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    Global,
    Repo,
}

/// The git configuration the pages show, read again every two seconds (another program may change it), when the
/// repository changes and after ImGit writes it.
#[derive(Debug, Default)]
pub(super) struct GitValues {
    values: HashMap<String, String>,
    read_at: f64,
    repo: Option<PathBuf>,
}

impl GitValues {
    /// Reads everything again at the next lookup.
    pub(super) fn invalidate(&mut self) {
        self.read_at = f64::NEG_INFINITY;
    }
}

/// The user and repository the pages describe.
#[derive(Debug, Clone, Default)]
pub(super) struct Context {
    pub author: Author,
    pub repo_name: String,
    pub hooks_path: String,
    pub home: String,
}

/// The Gitflow page's fields: the repository's gitflow.* configuration (git flow init's), or the defaults before
/// it is initialized; edited here until Initialize Gitflow writes them.
pub(super) struct GitflowField {
    pub label: &'static str,
    pub key: &'static str,
    pub fallback: &'static str,
}

pub(super) const GITFLOW_FIELDS: [GitflowField; 6] = [
    GitflowField {
        label: "Main/Master",
        key: "gitflow.branch.master",
        fallback: "main",
    },
    GitflowField {
        label: "Develop",
        key: "gitflow.branch.develop",
        fallback: "develop",
    },
    GitflowField {
        label: "Feature",
        key: "gitflow.prefix.feature",
        fallback: "feature/",
    },
    GitflowField {
        label: "Release",
        key: "gitflow.prefix.release",
        fallback: "release/",
    },
    GitflowField {
        label: "Hotfix",
        key: "gitflow.prefix.hotfix",
        fallback: "hotfix/",
    },
    GitflowField {
        label: "Version Tag",
        key: "gitflow.prefix.versiontag",
        fallback: "",
    },
];

/// The Gitflow fields being edited, for the repository they were read from.
#[derive(Debug, Default)]
pub(super) struct GitflowDraft {
    loaded: bool,
    workdir: Option<PathBuf>,
    pub values: [String; 6],
}

/// The commit template being edited, for the repository it was read from.
#[derive(Debug, Default)]
pub(super) struct TemplateDraft {
    loaded: bool,
    workdir: Option<PathBuf>,
    pub summary: String,
    pub description: String,
    pub saved_summary: String,
    pub saved_description: String,
}

/// The sparse-checkout paths being edited.
#[derive(Debug, Default)]
pub(super) struct SparseDraft {
    loaded: bool,
    workdir: Option<PathBuf>,
    pub paths: String,
}

/// The Preferences view's state between frames (the C++ function-local and file-level statics).
#[derive(Debug)]
pub struct State {
    /// The frame's time, for what runs after a menu closed.
    pub(super) now: f64,
    /// The text of the fields being edited, by their ImGui id.
    pub(super) buffers: HashMap<u32, String>,
    /// Default Branch Name's field.
    pub(super) default_branch: String,
    pub(super) git: GitValues,
    context: Option<(ContextKey, Context)>,
    /// The Signing Key options: "<None>" then gpg's secret keys.
    pub(super) signing_keys: Vec<String>,
    pub(super) signing_keys_read: bool,
    pub(super) gitflow: GitflowDraft,
    pub(super) template: TemplateDraft,
    pub(super) sparse: SparseDraft,
    /// The Generate new GPG Key passphrase field.
    pub(super) passphrase: String,
    /// The profile row whose menu is open, and where its button is.
    pub(super) menu_guid: String,
    pub(super) menu_button: (Vec2, Vec2),
    /// The Font selects' options (the monospace faces installed), read once.
    pub(super) font_options: Option<Vec<String>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            now: 0.0,
            buffers: HashMap::new(),
            default_branch: String::new(),
            git: GitValues::default(),
            context: None,
            signing_keys: vec!["<None>".to_owned()],
            signing_keys_read: false,
            gitflow: GitflowDraft::default(),
            template: TemplateDraft::default(),
            sparse: SparseDraft::default(),
            passphrase: String::new(),
            menu_guid: String::new(),
            menu_button: (Vec2::ZERO, Vec2::ZERO),
            font_options: None,
        }
    }
}

/// The active repository's working directory.
fn active_workdir(app: &Application) -> Option<PathBuf> {
    app.active().map(|state| state.repo.workdir().to_path_buf())
}

/// A key of the git configuration: the global one, or the open repository's (with the global one behind it).
/// Empty when unset.
pub(super) fn git_value(app: &mut Application, scope: Scope, key: &str) -> String {
    let repo = active_workdir(app);
    let now = app.panels.preferences.now;
    let git = &mut app.panels.preferences.git;
    if now - git.read_at > 2.0 || git.repo != repo {
        *git = GitValues {
            values: HashMap::new(),
            read_at: now,
            repo,
        };
    }
    let id = format!(
        "{}:{key}",
        if scope == Scope::Global {
            "global"
        } else {
            "repo"
        }
    );
    if let Some(value) = git.values.get(&id) {
        return value.clone();
    }
    let value = match scope {
        Scope::Global => global_config(key),
        Scope::Repo => app.active().and_then(|state| state.repo.config_string(key)),
    }
    .unwrap_or_default();
    app.panels.preferences.git.values.insert(id, value.clone());
    value
}

/// A boolean key; `fallback` while it is unset.
pub(super) fn git_flag(app: &mut Application, scope: Scope, key: &str, fallback: bool) -> bool {
    let value = git_value(app, scope, key);
    if value.is_empty() {
        return fallback;
    }
    matches!(
        value.to_ascii_lowercase().as_str(),
        "true" | "yes" | "on" | "1"
    )
}

/// Writes `key` (an empty value removes it); a failure is reported as a toast.
pub(super) fn set_git_value(app: &mut Application, scope: Scope, key: &str, value: &str) {
    let written = match scope {
        Scope::Global => set_global_config(key, Some(value)).map_err(|error| error.to_string()),
        Scope::Repo => match app.active() {
            Some(state) => state
                .repo
                .set_local_config(key, Some(value))
                .map_err(|error| error.to_string()),
            None => Err("Open a repository first.".to_owned()),
        },
    };
    if let Err(message) = written {
        app.toasts
            .push_with_detail(ToastKind::Error, format!("Could not set {key}"), message);
    }
    app.panels.preferences.git.invalidate();
}

pub(super) fn save_settings(app: &Application) {
    app.settings.save(&app.config_path("settings.txt"));
}

/// What [`context`] derives from: the active repository and the current profile's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContextKey {
    workdir: Option<PathBuf>,
    guid: String,
    user_name: String,
    user_email: String,
}

fn context_key(app: &Application) -> ContextKey {
    let profile = app.profiles.current();
    ContextKey {
        workdir: active_workdir(app),
        guid: profile.guid.clone(),
        user_name: profile.user_name.clone(),
        user_email: profile.user_email.clone(),
    }
}

/// The user and repository the pages describe, read now (the git configuration's user.name / user.email can
/// change under the cache of [`context`]).
fn read_context(app: &Application) -> Context {
    let mut context = Context::default();
    if let Some(state) = app.active() {
        context.author = state.repo.default_author().unwrap_or_default();
        context.repo_name = state.repo.name().to_owned();
        context.hooks_path = state
            .repo
            .workdir()
            .join(".git")
            .join("hooks")
            .to_string_lossy()
            .replace('/', std::path::MAIN_SEPARATOR_STR);
    }
    let profile = app.profiles.current();
    if context.author.email.is_empty() {
        context.author.email.clone_from(&profile.user_email);
    }
    if context.author.name.is_empty() {
        context.author.name.clone_from(&profile.user_name);
    }
    context.home = imgit_platform::system::home_directory()
        .to_string_lossy()
        .into_owned();
    context
}

/// The user and repository the pages describe, read again when the active repository or the current profile (or
/// its name / email) changes. What an action uses to make a key (an identity) is read with [`read_context`].
pub(super) fn context(app: &mut Application) -> Context {
    let key = context_key(app);
    if let Some((cached_for, context)) = &app.panels.preferences.context
        && *cached_for == key
    {
        return context.clone();
    }
    let context = read_context(app);
    app.panels.preferences.context = Some((key, context.clone()));
    context
}

// SSH and GPG keys: generated in the background (gpg can take a while), the results applied on the UI thread.

/// A secret key gpg holds: "<key id> <user id>" as the Signing Key options show it, and when it was created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SecretKey {
    pub label: String,
    pub created: i64,
}

/// `gpg --with-colons --list-secret-keys`: each "sec" record's key id (field 5) and creation time (field 6) with
/// its first "uid" record's user id (field 10).
pub(super) fn parse_secret_keys(text: &str) -> Vec<SecretKey> {
    let mut keys: Vec<SecretKey> = Vec::new();
    let mut want_uid = false;
    for line in text.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields[0] == "sec" && fields.len() > 5 {
            keys.push(SecretKey {
                label: fields[4].to_owned(),
                created: fields[5].trim().parse().unwrap_or(0),
            });
            want_uid = true;
        } else if fields[0] == "uid"
            && want_uid
            && fields.len() > 9
            && let Some(key) = keys.last_mut()
        {
            key.label.push(' ');
            key.label.push_str(fields[9]);
            want_uid = false;
        }
    }
    keys
}

/// gpg.program, the program `git commit -S` signs with ("gpg" when unset).
fn gpg_program(app: &mut Application) -> String {
    let program = git_value(app, Scope::Global, "gpg.program");
    if program.is_empty() {
        "gpg".to_owned()
    } else {
        program
    }
}

pub(super) fn ssh_signing(app: &mut Application) -> bool {
    git_value(app, Scope::Global, "gpg.format").eq_ignore_ascii_case("ssh")
}

/// `program` when it starts, else, for the default "gpg", the gpg Git for Windows ships (usr/bin beside its
/// libexec), which `git commit -S` finds even when it is not on PATH. Runs a process: call off the UI thread.
fn resolve_gpg(program: &str) -> String {
    let started = process::run_process(
        Path::new(""),
        &[program.to_owned(), "--version".to_owned()],
        RunOptions::default(),
    )
    .is_ok();
    if started || program != "gpg" {
        return program.to_owned();
    }
    let exec_path = cli::run(Path::new(""), ["--exec-path"], cli::RunOptions::default());
    let folder = exec_path.text.trim_end_matches(['\n', '\r']);
    // <git>/mingw64/libexec/git-core -> <git>/usr/bin/gpg.exe
    let bundled = Path::new(folder)
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|git| git.join("usr").join("bin").join("gpg.exe"));
    match bundled {
        Some(bundled) if exec_path.exit_code == 0 && bundled.exists() => {
            bundled.to_string_lossy().into_owned()
        }
        _ => program.to_owned(),
    }
}

/// Reads the Signing Key options again: gpg's secret keys (in the background), or with `gpg.format ssh` the SSH
/// public key files.
pub(super) fn refresh_signing_keys(app: &mut Application) {
    app.panels.preferences.signing_keys_read = true;
    if ssh_signing(app) {
        // gpg.format ssh signs with an SSH key: user.signingkey is a public key file. The ones in ~/.ssh and the
        // SSH page's.
        let mut options = vec!["<None>".to_owned()];
        let folder = imgit_platform::system::home_directory().join(".ssh");
        if let Ok(entries) = fs::read_dir(folder) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|extension| extension == "pub") {
                    options.push(path.to_string_lossy().into_owned());
                }
            }
        }
        let key = preferences().get(pref::SSH_PUBLIC_KEY);
        if !key.is_empty() && !options.contains(&key) {
            options.push(key);
        }
        app.panels.preferences.signing_keys = options;
        return;
    }
    if app.background_running("gpg-keys") {
        return;
    }
    let configured = gpg_program(app);
    app.run_in_background("gpg-keys", move || -> Completion {
        let program = resolve_gpg(&configured);
        let output = process::run_process(
            Path::new(""),
            &[
                program,
                "--batch".to_owned(),
                "--with-colons".to_owned(),
                "--list-secret-keys".to_owned(),
            ],
            RunOptions::default(),
        );
        let mut options = vec!["<None>".to_owned()];
        if let Ok(output) = output
            && output.exit_code == 0
        {
            options.extend(
                parse_secret_keys(&output.text)
                    .into_iter()
                    .map(|key| key.label),
            );
        }
        Box::new(move |app: &mut Application| app.panels.preferences.signing_keys = options)
    });
}

/// Makes `option` ("<key id> <user id>", an SSH public key file, or "<None>") the signing key: user.signingkey in
/// the global .gitconfig, which `git commit -S` reads.
pub(super) fn set_signing_key(app: &mut Application, option: &str) {
    if option == "<None>" {
        set_git_value(app, Scope::Global, "user.signingkey", "");
        return;
    }
    let key = if ssh_signing(app) {
        option
    } else {
        gpg_key_id(option)
    };
    set_git_value(app, Scope::Global, "user.signingkey", key);
}

/// The key id of a GPG option ("<key id> <user id>").
fn gpg_key_id(option: &str) -> &str {
    option.split(' ').next().unwrap_or(option)
}

/// The option `key` (user.signingkey) names among `options`: by its key id (long, short or the fingerprint) or
/// its file; the key itself when no option matches, "<None>" for none.
pub(super) fn signing_key_option(key: &str, options: &[String]) -> String {
    if key.is_empty() {
        return "<None>".to_owned();
    }
    for option in options {
        // The key may be named by its long id, a short one or the full fingerprint.
        let id = option.split(' ').next().unwrap_or(option);
        if option == key
            || id == key
            || (key.len() >= 8 && id.ends_with(key))
            || (id.len() >= 8 && key.ends_with(id))
        {
            return option.clone();
        }
    }
    key.to_owned()
}

/// ssh-keygen makes an ed25519 key pair in ~/.ssh under a name no key there has yet (id_ed25519_imgit,
/// id_ed25519_imgit_2, ...); the SSH page then uses it.
pub(super) fn generate_ssh_key(app: &mut Application) {
    if app.background_running("ssh-generate") {
        app.toasts
            .push(ToastKind::Info, "An SSH key is already being generated");
        return;
    }
    let context = read_context(app);
    app.run_in_background("ssh-generate", move || -> Completion {
        let folder = Path::new(&context.home).join(".ssh");
        let _ = fs::create_dir_all(&folder);
        let mut key = folder.join("id_ed25519_imgit");
        let mut n = 2;
        while key.exists() || PathBuf::from(format!("{}.pub", key.display())).exists() {
            key = folder.join(format!("id_ed25519_imgit_{n}"));
            n += 1;
        }
        let output = process::run_process(
            &folder,
            &[
                "ssh-keygen".to_owned(),
                "-q".to_owned(),
                "-t".to_owned(),
                "ed25519".to_owned(),
                "-N".to_owned(),
                String::new(),
                "-C".to_owned(),
                context.author.email,
                "-f".to_owned(),
                key.to_string_lossy().into_owned(),
            ],
            RunOptions::default(),
        );
        let detail = match output {
            Ok(output) if output.exit_code == 0 => {
                let private_key = key.to_string_lossy().into_owned();
                return Box::new(move |app: &mut Application| {
                    let _ = preferences().set(pref::SSH_PRIVATE_KEY, &private_key);
                    let _ = preferences().set(pref::SSH_PUBLIC_KEY, &format!("{private_key}.pub"));
                    app.toasts.push_with_detail(
                        ToastKind::Success,
                        "SSH key generated",
                        format!("{private_key}.pub"),
                    );
                });
            }
            Ok(output) => output.text,
            Err(_) => "ssh-keygen was not found.".to_owned(),
        };
        Box::new(move |app: &mut Application| {
            app.toasts
                .push_with_detail(ToastKind::Error, "Could not generate an SSH key", detail);
        })
    });
}

/// gpg makes a 4096-bit RSA signing key for the profile's name and email, expiring in 2 years, protected by
/// `passphrase` (none when empty), and it becomes the signing key.
pub(super) fn generate_gpg_key(app: &mut Application, passphrase: &str) {
    if app.background_running("gpg-generate") {
        app.toasts
            .push(ToastKind::Info, "A GPG key is already being generated");
        return;
    }
    if ssh_signing(app) {
        app.toasts.push_with_detail(
            ToastKind::Error,
            "Could not generate a GPG key",
            "Commits are signed with an SSH key (gpg.format is ssh); a GPG key could not sign them.",
        );
        return;
    }
    let author = read_context(app).author;
    if author.email.is_empty() {
        app.toasts.push_with_detail(
            ToastKind::Error,
            "Could not generate a GPG key",
            "Set your email in your profile first.",
        );
        return;
    }
    let name = if author.name.is_empty() {
        &author.email
    } else {
        &author.name
    };
    let user_id = format!("{name} <{}>", author.email);
    app.toasts.push_with_detail(
        ToastKind::Info,
        "Generating a GPG key",
        "This can take a minute.",
    );
    let configured = gpg_program(app);
    let passphrase = passphrase.to_owned();
    app.run_in_background("gpg-generate", move || -> Completion {
        let program = resolve_gpg(&configured);
        let input = format!("{passphrase}\n");
        let made = process::run_process(
            Path::new(""),
            &[
                program.clone(),
                "--batch".to_owned(),
                "--pinentry-mode".to_owned(),
                "loopback".to_owned(),
                "--passphrase-fd".to_owned(),
                "0".to_owned(),
                "--quick-gen-key".to_owned(),
                user_id.clone(),
                "rsa4096".to_owned(),
                "sign".to_owned(),
                "2y".to_owned(),
            ],
            RunOptions {
                input: input.as_bytes(),
                ..RunOptions::default()
            },
        );
        match made {
            Ok(output) if output.exit_code == 0 => {}
            Ok(output) => return gpg_failure(output.text),
            Err(_) => return gpg_failure(format!("{program} was not found.")),
        }
        let listed = process::run_process(
            Path::new(""),
            &[
                program,
                "--batch".to_owned(),
                "--with-colons".to_owned(),
                "--list-secret-keys".to_owned(),
                user_id,
            ],
            RunOptions::default(),
        );
        let keys = listed
            .map(|output| parse_secret_keys(&output.text))
            .unwrap_or_default();
        let Some(newest) = keys.into_iter().max_by_key(|key| key.created) else {
            return Box::new(|app: &mut Application| {
                app.toasts
                    .push(ToastKind::Error, "Could not find the new GPG key");
            });
        };
        Box::new(move |app: &mut Application| {
            // Only the key id is user.signingkey, whatever the label (or gpg.format) says.
            set_git_value(
                app,
                Scope::Global,
                "user.signingkey",
                gpg_key_id(&newest.label),
            );
            refresh_signing_keys(app);
            app.toasts.push_with_detail(
                ToastKind::Success,
                "GPG key generated",
                format!("{} is your signing key now.", newest.label),
            );
        })
    });
}

fn gpg_failure(detail: String) -> Completion {
    Box::new(move |app: &mut Application| {
        app.toasts
            .push_with_detail(ToastKind::Error, "Could not generate a GPG key", detail);
    })
}

/// The Gitflow fields for the active repository: its gitflow.* configuration once initialized, the defaults
/// before; read again when the repository changes.
pub(super) fn load_gitflow_draft(app: &mut Application) {
    let workdir = active_workdir(app);
    let draft = &app.panels.preferences.gitflow;
    if draft.loaded && draft.workdir == workdir {
        return;
    }
    let mut values: [String; 6] = Default::default();
    if let Some(state) = app.active() {
        let initialized = state.repo.config_string("gitflow.branch.develop").is_some();
        for (value, field) in values.iter_mut().zip(&GITFLOW_FIELDS) {
            *value = if initialized {
                state.repo.config_string(field.key).unwrap_or_default()
            } else {
                field.fallback.to_owned()
            };
        }
    } else {
        for (value, field) in values.iter_mut().zip(&GITFLOW_FIELDS) {
            *value = field.fallback.to_owned();
        }
    }
    app.panels.preferences.gitflow = GitflowDraft {
        loaded: true,
        workdir,
        values,
    };
}

/// What `git flow init` sets up: the branch names and prefixes from the Gitflow page in the repository's
/// configuration, and the develop branch (from main) when it is missing.
pub(super) fn initialize_gitflow(app: &mut Application) {
    if app.active().is_none() {
        app.toasts.push_with_detail(
            ToastKind::Error,
            "Could not initialize Gitflow",
            "Open a repository first.",
        );
        return;
    }
    load_gitflow_draft(app);
    let values = app.panels.preferences.gitflow.values.clone();
    let setting = |i: usize| {
        if values[i].is_empty() {
            GITFLOW_FIELDS[i].fallback.to_owned()
        } else {
            values[i].clone()
        }
    };
    let main_branch = setting(0);
    let develop = setting(1);
    let fail = |app: &mut Application, detail: String| {
        app.toasts
            .push_with_detail(ToastKind::Error, "Could not initialize Gitflow", detail);
    };
    // The version tag prefix may be empty; the others fall back to their defaults.
    let prefixes: [String; 4] = [2, 3, 4, 5].map(|i| {
        if i == 5 {
            values[i].clone()
        } else {
            setting(i)
        }
    });
    if let Err(detail) = check_gitflow_fields(&main_branch, &develop, &prefixes) {
        fail(app, detail);
        return;
    }
    app.with_active(|app, state| {
        if state
            .find_ref(&format!("refs/heads/{main_branch}"))
            .is_none()
        {
            fail(app, format!("There is no local branch '{main_branch}'."));
            return;
        }
        let mut runs: Vec<Vec<String>> = Vec::new();
        // The branch first: the sequence stops at a failure, so the configuration is only written once the
        // develop branch exists.
        if state.find_ref(&format!("refs/heads/{develop}")).is_none() {
            runs.push(vec![
                "branch".to_owned(),
                "--".to_owned(),
                develop.clone(),
                main_branch.clone(),
            ]);
        }
        let configured = [main_branch.as_str(), develop.as_str()]
            .into_iter()
            .chain(prefixes.iter().map(String::as_str));
        for (field, value) in GITFLOW_FIELDS.iter().zip(configured) {
            runs.push(vec![
                "config".to_owned(),
                "--".to_owned(),
                field.key.to_owned(),
                value.to_owned(),
            ]);
        }
        runs.push(
            ["config", "--", "gitflow.prefix.support", "support/"]
                .map(str::to_owned)
                .to_vec(),
        );
        app.run_git_cli_sequence(state, "Initialize Gitflow", runs);
    });
    app.panels.preferences.git.invalidate();
}

/// Whether `name` is a valid branch name by `git check-ref-format --branch`'s rules (and not an option).
fn is_valid_branch_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('-') || name == "@" {
        return false;
    }
    if name.starts_with('/') || name.ends_with('/') || name.ends_with('.') || name.contains("//") {
        return false;
    }
    if name.contains("..") || name.contains("@{") {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
    {
        return false;
    }
    name.split('/')
        .all(|part| !part.starts_with('.') && !part.ends_with(".lock"))
}

/// The Gitflow page's values checked before anything runs: both branch names must be valid, distinct branch
/// names (they are passed to `git branch`), and each prefix (feature, release, hotfix, version tag; empty
/// allowed) must not look like an option, hold whitespace or control characters, and keep names valid.
pub(super) fn check_gitflow_fields(
    main_branch: &str,
    develop: &str,
    prefixes: &[String; 4],
) -> Result<(), String> {
    for (label, name) in [("main", main_branch), ("develop", develop)] {
        if !is_valid_branch_name(name) {
            return Err(format!("'{name}' is not a valid {label} branch name."));
        }
    }
    if main_branch == develop {
        return Err("The main and develop branches must differ.".to_owned());
    }
    for (label, prefix) in ["feature", "release", "hotfix", "version tag"]
        .into_iter()
        .zip(prefixes)
    {
        let bad = prefix.starts_with('-')
            || prefix.chars().any(|c| c.is_control() || c.is_whitespace())
            || (!prefix.is_empty() && !is_valid_branch_name(&format!("{prefix}x")));
        if bad {
            return Err(format!("'{prefix}' is not a valid {label} prefix."));
        }
    }
    Ok(())
}

/// A commit template's text as the summary line and the description (the blank lines between them dropped).
pub(super) fn split_template(text: &str) -> (String, String) {
    match text.split_once('\n') {
        Some((summary, rest)) => (summary.to_owned(), rest.trim_start_matches('\n').to_owned()),
        None => (text.to_owned(), String::new()),
    }
}

/// The commit template of the active repository, read again when the repository changes.
pub(super) fn load_template_draft(app: &mut Application) {
    let workdir = active_workdir(app);
    let draft = &app.panels.preferences.template;
    if draft.loaded && draft.workdir == workdir {
        return;
    }
    let (summary, description) = app
        .active()
        .and_then(|state| state.repo.commit_template())
        .map(|text| split_template(&text))
        .unwrap_or_default();
    app.panels.preferences.template = TemplateDraft {
        loaded: true,
        workdir,
        saved_summary: summary.clone(),
        saved_description: description.clone(),
        summary,
        description,
    };
}

/// The paths a sparse checkout includes, as .git/info/sparse-checkout lists them (cone mode's "/*" and "!/*/"
/// patterns left out, the slashes around folders dropped).
pub(super) fn sparse_paths(file: &str) -> String {
    let mut paths = String::new();
    for line in file.lines() {
        if line.is_empty() || line == "/*" || line.starts_with(['!', '#']) {
            continue;
        }
        paths.push_str(line.strip_prefix('/').unwrap_or(line));
        paths.push('\n');
    }
    paths
}

/// The sparse-checkout paths of the active repository, read again when the repository changes.
pub(super) fn load_sparse_draft(app: &mut Application) {
    let workdir = active_workdir(app);
    let draft = &app.panels.preferences.sparse;
    if draft.loaded && draft.workdir == workdir {
        return;
    }
    let paths = app
        .active()
        .and_then(|state| {
            fs::read_to_string(state.repo.git_dir().join("info").join("sparse-checkout")).ok()
        })
        .map(|file| sparse_paths(&file))
        .unwrap_or_default();
    app.panels.preferences.sparse = SparseDraft {
        loaded: true,
        workdir,
        paths,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_keys_pair_with_their_first_user_id() {
        let listing = "tru::1:1700000000:0:3:1:5\r\n\
            sec:u:4096:1:ABCDEF0123456789:1700000100:::u:::scESC:::+::::\r\n\
            fpr:::::::::0000000000000000000000000000000000000000:\r\n\
            uid:u::::1700000100::HASH::Ada <ada@example.com>::::::::::0:\r\n\
            uid:u::::1700000100::HASH2::Other <o@example.com>::::::::::0:\r\n\
            sec:u:4096:1:1111111111111111:1800000000:::u:::scESC:::+::::\r\n\
            uid:u::::1800000000::HASH3::Bob <bob@example.com>::::::::::0:\r\n";
        assert_eq!(
            parse_secret_keys(listing),
            [
                SecretKey {
                    label: "ABCDEF0123456789 Ada <ada@example.com>".to_owned(),
                    created: 1_700_000_100,
                },
                SecretKey {
                    label: "1111111111111111 Bob <bob@example.com>".to_owned(),
                    created: 1_800_000_000,
                },
            ]
        );
        assert!(parse_secret_keys("").is_empty());
    }

    #[test]
    fn gitflow_fields_are_checked() {
        let ok = ["feature/", "release/", "hotfix/", ""].map(str::to_owned);
        assert!(check_gitflow_fields("main", "develop", &ok).is_ok());
        for bad in [
            "-D", "", "a b", "a..b", "x.lock", "/a", "a/", "@", "a~1", "a:b",
        ] {
            assert!(check_gitflow_fields(bad, "develop", &ok).is_err(), "{bad}");
            assert!(check_gitflow_fields("main", bad, &ok).is_err(), "{bad}");
        }
        assert!(check_gitflow_fields("main", "main", &ok).is_err());
        for bad in ["-f", "a b", "a\tb", "x..", ".hid/"] {
            let prefixes = [bad, "r/", "h/", ""].map(str::to_owned);
            assert!(
                check_gitflow_fields("main", "develop", &prefixes).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_gpg_option_stores_only_its_key_id() {
        assert_eq!(gpg_key_id("ABCD1234EF Name <a@b.c>"), "ABCD1234EF");
        assert_eq!(gpg_key_id("ABCD1234EF"), "ABCD1234EF");
    }

    #[test]
    fn the_signing_key_finds_its_option() {
        let options = vec![
            "<None>".to_owned(),
            "ABCDEF0123456789 Ada <ada@example.com>".to_owned(),
            "C:\\keys\\id.pub".to_owned(),
        ];
        assert_eq!(signing_key_option("", &options), "<None>");
        assert_eq!(signing_key_option("ABCDEF0123456789", &options), options[1]);
        // A short id and a fingerprint both name it.
        assert_eq!(signing_key_option("23456789", &options), options[1]);
        assert_eq!(
            signing_key_option("00000000ABCDEF0123456789", &options),
            options[1]
        );
        assert_eq!(signing_key_option("C:\\keys\\id.pub", &options), options[2]);
        assert_eq!(signing_key_option("unknown", &options), "unknown");
        assert_eq!(signing_key_option("short", &options), "short");
    }

    #[test]
    fn templates_split_at_the_first_line() {
        assert_eq!(
            split_template("Summary\n\nBody line\nmore"),
            ("Summary".to_owned(), "Body line\nmore".to_owned())
        );
        assert_eq!(split_template("Only"), ("Only".to_owned(), String::new()));
        assert_eq!(split_template(""), (String::new(), String::new()));
    }

    #[test]
    fn sparse_paths_drop_cone_patterns() {
        let file = "/*\n!/*/\n/src/\n/docs/guide/\nREADME.md\n# comment\n\n";
        assert_eq!(sparse_paths(file), "src/\ndocs/guide/\nREADME.md\n");
        assert_eq!(sparse_paths(""), "");
    }
}
