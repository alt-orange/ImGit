//! Configuration: the repository's (with the global and system levels behind it) and the global one.

use std::path::Path;

use super::{Repository, Result};

impl Repository {
    /// A key of the configuration (repository, global and system levels); `None` when unset or empty.
    #[must_use]
    pub fn config_string(&self, key: &str) -> Option<String> {
        let config = self.config_snapshot()?;
        config
            .get_string(key)
            .ok()
            .filter(|value| !value.is_empty())
    }

    /// A boolean key of the configuration; `fallback` when unset or not a boolean.
    #[must_use]
    pub fn config_bool(&self, key: &str, fallback: bool) -> bool {
        self.config_snapshot()
            .and_then(|config| config.get_bool(key).ok())
            .unwrap_or(fallback)
    }

    /// Writes `key` to the repository's own `.git/config`; `None` (or an empty value) removes it.
    pub fn set_local_config(&self, key: &str, value: Option<&str>) -> Result<()> {
        let mut local = self.repo.config()?.open_level(git2::ConfigLevel::Local)?;
        set_or_remove(&mut local, key, value)
    }

    /// The text of the file `commit.template` names (`~` expanded, relative to the working directory), its
    /// comment lines left out; `None` when there is none or it is empty.
    #[must_use]
    pub fn commit_template(&self) -> Option<String> {
        // get_path expands a leading ~/.
        let path = self.config_snapshot()?.get_path("commit.template").ok()?;
        read_message_file(&self.workdir.join(path))
    }

    /// The message git prepared for the commit that concludes a merge, cherry-pick or revert (`MERGE_MSG`, its
    /// comment lines left out); `None` when there is none.
    #[must_use]
    pub fn prepared_message(&self) -> Option<String> {
        read_message_file(&self.repo.path().join("MERGE_MSG"))
    }

    fn config_snapshot(&self) -> Option<git2::Config> {
        self.repo
            .config()
            .and_then(|mut config| config.snapshot())
            .ok()
    }
}

/// A key of the global / system git configuration, `None` when unset or empty.
#[must_use]
pub fn global_config(key: &str) -> Option<String> {
    let config = git2::Config::open_default().ok()?;
    config
        .get_string(key)
        .ok()
        .filter(|value| !value.is_empty())
}

/// A boolean key of the global / system git configuration; `fallback` when unset or not a boolean.
#[must_use]
pub fn global_config_bool(key: &str, fallback: bool) -> bool {
    git2::Config::open_default()
        .and_then(|config| config.get_bool(key))
        .unwrap_or(fallback)
}

/// `user.email` from the global / system git configuration, `None` when unset.
#[must_use]
pub fn global_user_email() -> Option<String> {
    global_config("user.email")
}

/// `user.name` from the global / system git configuration, `None` when unset.
#[must_use]
pub fn global_user_name() -> Option<String> {
    global_config("user.name")
}

/// Writes `key` to the global `.gitconfig`; `None` (or an empty value) removes it.
pub fn set_global_config(key: &str, value: Option<&str>) -> Result<()> {
    let mut global = git2::Config::open_default()?.open_level(git2::ConfigLevel::Global)?;
    set_or_remove(&mut global, key, value)
}

/// Writes `user.name` / `user.email` to the global `.gitconfig` (the profile's "Keep my .gitconfig updated").
/// An empty value is left as it is.
pub fn set_global_identity(name: &str, email: &str) -> Result<()> {
    let mut global = git2::Config::open_default()?.open_level(git2::ConfigLevel::Global)?;
    if !name.is_empty() {
        global.set_str("user.name", name)?;
    }
    if !email.is_empty() {
        global.set_str("user.email", email)?;
    }
    Ok(())
}

fn set_or_remove(config: &mut git2::Config, key: &str, value: Option<&str>) -> Result<()> {
    match value.filter(|value| !value.is_empty()) {
        Some(value) => config.set_str(key, value)?,
        None => match config.remove(key) {
            Err(error) if error.code() != git2::ErrorCode::NotFound => return Err(error.into()),
            _ => {}
        },
    }
    Ok(())
}

/// A commit message file without its comment lines and surrounding blank lines; `None` when it cannot be read
/// or nothing is left.
fn read_message_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let content = String::from_utf8_lossy(&bytes);
    let mut text = String::new();
    for line in content.split_inclusive('\n') {
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with('#') {
            continue;
        }
        text.push_str(line);
        text.push('\n');
    }
    let text = text.trim_end_matches(['\n', ' ']).trim_start_matches('\n');
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::read_message_file;

    #[test]
    fn message_files_lose_comments_and_surrounding_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("MERGE_MSG");
        std::fs::write(
            &file,
            "\n\nMerge branch 'x'\r\n\r\n# Conflicts:\n#\ta.txt\nbody  \n\n",
        )
        .unwrap();
        assert_eq!(
            read_message_file(&file).as_deref(),
            Some("Merge branch 'x'\n\nbody")
        );
        std::fs::write(&file, "# only comments\n\n").unwrap();
        assert_eq!(read_message_file(&file), None);
        assert_eq!(read_message_file(&dir.path().join("missing")), None);
    }
}
