//! Profiles: a name and a commit author each, with their own preferences, tabs and repository lists.
//!
//! On disk: `<root>/profiles/<guid>/profile.txt` (plus that profile's other files next to it) and the app-wide
//! `<root>/config.txt` (the current profile and whether the global `.gitconfig` follows it). Both are
//! `key=value` lines.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::{fs, io};

const PROFILES_DIRECTORY: &str = "profiles";
const PROFILE_FILE: &str = "profile.txt";
const CONFIG_FILE: &str = "config.txt";
const DEFAULT_PROFILE_NAME: &str = "Default Profile";
const NEW_PROFILE_NAME: &str = "New Profile";

/// Replaces `path` with `text` through a temporary file next to it (`<path>.tmp`), so a crash or a full disk
/// never leaves the file empty. Creates the folder when it is missing.
///
/// # Errors
/// The file could not be written; what was there before is untouched.
pub fn write_text_file(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        // Best effort: when it fails, so does creating the file, with the error worth reporting.
        let _ = fs::create_dir_all(parent);
    }
    let mut temporary = OsString::from(path);
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    // rename replaces an existing file on POSIX and on Windows.
    let written = fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// A profile: a name and the commit author, with its own preferences, tabs and repository lists (the files in
/// its directory).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Profile {
    /// Its folder's name: 32 hex digits. Given by [`Profiles::create`].
    pub guid: String,
    pub name: String,
    pub user_name: String,
    pub user_email: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("there is no profile {0}")]
    NotFound(String),
    #[error("the current profile cannot be deleted")]
    IsCurrent,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// The profiles under `<root>/profiles/<guid>/` and the app-wide `<root>/config.txt`, one folder per profile.
/// There is always at least one profile, and one of them is the current one.
#[derive(Debug, Clone)]
pub struct Profiles {
    root: PathBuf,
    /// Sorted by name; never empty.
    profiles: Vec<Profile>,
    /// The current profile's guid.
    current: String,
    keep_git_config: bool,
}

impl Profiles {
    /// Loads the profiles under `root`. With none, creates "Default Profile" for the given author (the git
    /// configuration's) and moves `legacy_files` (written before profiles existed) from `root` into it.
    ///
    /// Loading does not fail: what cannot be read is skipped and what cannot be written is tried again by the
    /// next change, so ImGit starts with a usable profile even on a read-only disk.
    #[must_use]
    pub fn load(
        root: impl Into<PathBuf>,
        user_name: &str,
        user_email: &str,
        legacy_files: &[&str],
    ) -> Self {
        let root = root.into();
        let profiles_directory = root.join(PROFILES_DIRECTORY);
        let _ = fs::create_dir_all(&profiles_directory);

        let mut profiles: Vec<Profile> = fs::read_dir(&profiles_directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| {
                let mut values = read_values(&entry.path().join(PROFILE_FILE));
                // A profile without its required fields is skipped.
                let name = values.remove("name").filter(|name| !name.is_empty())?;
                Some(Profile {
                    guid: entry.file_name().to_string_lossy().into_owned(),
                    name,
                    user_name: values.remove("user_name").unwrap_or_default(),
                    user_email: values.remove("user_email").unwrap_or_default(),
                })
            })
            .collect();

        let mut config = read_values(&root.join(CONFIG_FILE));
        let mut loaded = Self {
            root,
            profiles: Vec::new(),
            current: config.remove("profile_guid").unwrap_or_default(),
            keep_git_config: config.get("keep_git_config").is_none_or(|keep| keep != "0"),
        };

        if profiles.is_empty() {
            // The default profile takes its author from the git configuration.
            let profile = Profile {
                guid: new_guid(),
                name: DEFAULT_PROFILE_NAME.to_owned(),
                user_name: user_name.to_owned(),
                user_email: user_email.to_owned(),
            };
            let directory = loaded.directory_of(&profile.guid);
            // profile.txt first: a crash between the moves and the save would leave a folder the next start
            // does not list, and it would create a second default profile.
            let _ = loaded.save(&profile);
            for file in legacy_files {
                let legacy = loaded.root.join(file);
                if legacy.exists() {
                    let _ = fs::rename(&legacy, directory.join(file));
                }
            }
            profiles.push(profile);
        }
        loaded.profiles = profiles;
        loaded.sort();
        if loaded.find(&loaded.current).is_none() {
            loaded.current = loaded.profiles[0].guid.clone();
        }
        let _ = loaded.save_config();
        loaded
    }

    /// Sorted by name, like the profile table and pickers.
    #[must_use]
    pub fn all(&self) -> &[Profile] {
        &self.profiles
    }

    #[must_use]
    pub fn current(&self) -> &Profile {
        self.find(&self.current).unwrap_or(&self.profiles[0])
    }

    #[must_use]
    pub fn find(&self, guid: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.guid == guid)
    }

    /// The folder a profile's files are in.
    #[must_use]
    pub fn directory_of(&self, guid: &str) -> PathBuf {
        self.root.join(PROFILES_DIRECTORY).join(guid)
    }

    /// The current profile's folder.
    #[must_use]
    pub fn directory(&self) -> PathBuf {
        self.directory_of(&self.current().guid)
    }

    /// Creates a profile ("New Profile" when unnamed; `values.guid` is ignored), starting from a copy of the
    /// current one's files when asked; returns its guid.
    ///
    /// # Errors
    /// Its folder could not be written: there is no new profile then.
    pub fn create(&mut self, values: Profile, copy_current: bool) -> Result<String, ProfileError> {
        let mut profile = Profile {
            guid: new_guid(),
            ..values
        };
        if profile.name.is_empty() {
            profile.name = NEW_PROFILE_NAME.to_owned();
        }
        let directory = self.directory_of(&profile.guid);
        let written = (|| {
            if copy_current {
                copy_directory(&self.directory(), &directory)?;
            }
            fs::create_dir_all(&directory)?;
            self.save(&profile)
        })();
        if let Err(error) = written {
            let _ = fs::remove_dir_all(&directory);
            return Err(error.into());
        }
        let guid = profile.guid.clone();
        self.profiles.push(profile);
        self.sort();
        Ok(guid)
    }

    /// Replaces the profile with `profile.guid`'s name and author.
    ///
    /// # Errors
    /// There is no such profile, or its file could not be written (the list has the new values all the same).
    pub fn update(&mut self, profile: &Profile) -> Result<(), ProfileError> {
        let existing = self
            .profiles
            .iter_mut()
            .find(|existing| existing.guid == profile.guid)
            .ok_or_else(|| ProfileError::NotFound(profile.guid.clone()))?;
        existing.clone_from(profile);
        self.sort();
        Ok(self.save(profile)?)
    }

    /// Deletes a profile other than the current one, with its files.
    ///
    /// # Errors
    /// It is the current profile, there is no such profile, or its folder could not be deleted (it stays
    /// listed then, and still loads: `profile.txt` is deleted last, so a failure part-way leaves it).
    pub fn remove(&mut self, guid: &str) -> Result<(), ProfileError> {
        if guid == self.current {
            return Err(ProfileError::IsCurrent);
        }
        if self.find(guid).is_none() {
            return Err(ProfileError::NotFound(guid.to_owned()));
        }
        remove_profile_directory(&self.directory_of(guid))?;
        self.profiles.retain(|profile| profile.guid != guid);
        Ok(())
    }

    /// # Errors
    /// There is no such profile, or `config.txt` could not be written (the profile is current all the same).
    pub fn set_current(&mut self, guid: &str) -> Result<(), ProfileError> {
        if self.find(guid).is_none() {
            return Err(ProfileError::NotFound(guid.to_owned()));
        }
        guid.clone_into(&mut self.current);
        Ok(self.save_config()?)
    }

    /// Whether switching or editing the current profile writes its author to the global `.gitconfig`.
    #[must_use]
    pub fn keep_git_config(&self) -> bool {
        self.keep_git_config
    }

    /// # Errors
    /// `config.txt` could not be written (the setting is changed all the same).
    pub fn set_keep_git_config(&mut self, keep: bool) -> io::Result<()> {
        self.keep_git_config = keep;
        self.save_config()
    }

    fn save(&self, profile: &Profile) -> io::Result<()> {
        write_text_file(
            &self.directory_of(&profile.guid).join(PROFILE_FILE),
            &format!(
                "name={}\nuser_name={}\nuser_email={}\n",
                one_line(&profile.name),
                one_line(&profile.user_name),
                one_line(&profile.user_email)
            ),
        )
    }

    fn save_config(&self) -> io::Result<()> {
        write_text_file(
            &self.root.join(CONFIG_FILE),
            &format!(
                "profile_guid={}\nkeep_git_config={}\n",
                self.current,
                u8::from(self.keep_git_config)
            ),
        )
    }

    /// By name, ignoring (ASCII) case; profiles of the same name keep their order.
    fn sort(&mut self) {
        self.profiles
            .sort_by_cached_key(|profile| profile.name.to_ascii_lowercase());
    }
}

/// Deletes a profile's folder with everything in it, `profile.txt` last: until it is gone the profile is still
/// there for the next load, so a failure part-way (a file in use) does not make it vanish half-deleted.
fn remove_profile_directory(directory: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name() == PROFILE_FILE {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    match fs::remove_file(directory.join(PROFILE_FILE)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    // The profile is gone from disk whether or not the empty folder can be removed.
    let _ = fs::remove_dir(directory);
    Ok(())
}

/// `key=value` lines (the value runs to the end of the line); empty when the file cannot be read.
fn read_values(path: &Path) -> HashMap<String, String> {
    let bytes = fs::read(path).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    // A UTF-8 byte order mark (an editor's) is not part of the first key.
    text.strip_prefix('\u{FEFF}')
        .unwrap_or(&text)
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// A value on the one line it is saved on.
fn one_line(text: &str) -> String {
    text.chars()
        .filter(|&c| c != '\r')
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect()
}

/// A random (version 4) uuid without its dashes.
fn new_guid() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Copies the folder `from` and everything in it to `to`.
fn copy_directory(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> Profile {
        Profile {
            name: name.to_owned(),
            ..Profile::default()
        }
    }

    #[test]
    fn write_text_file_replaces_and_creates_folders() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a").join("b").join("file.txt");
        write_text_file(&path, "one\n").unwrap();
        write_text_file(&path, "two\r\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two\r\n");
        assert!(!path.with_file_name("file.txt.tmp").exists());
        // A folder in the file's place: the error is reported and no temporary file is left.
        let taken = root.path().join("a").join("b");
        assert!(write_text_file(&taken, "x").is_err());
        assert!(!root.path().join("a").join("b.tmp").exists());
    }

    #[test]
    fn first_load_creates_the_default_profile() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("tabs.txt"), "legacy tabs").unwrap();
        let profiles = Profiles::load(
            root.path(),
            "Ada",
            "ada@example.com",
            &["tabs.txt", "missing.txt"],
        );

        assert_eq!(profiles.all().len(), 1);
        let profile = profiles.current().clone();
        assert_eq!(profile.name, "Default Profile");
        assert_eq!(profile.user_name, "Ada");
        assert_eq!(profile.user_email, "ada@example.com");
        assert!(profiles.keep_git_config());

        // The guid: a version 4 uuid without dashes.
        assert_eq!(profile.guid.len(), 32);
        assert!(
            profile
                .guid
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        );
        assert_eq!(profile.guid.as_bytes()[12], b'4');
        assert!(matches!(
            profile.guid.as_bytes()[16],
            b'8' | b'9' | b'a' | b'b'
        ));

        // The files, byte for byte.
        let directory = root.path().join("profiles").join(&profile.guid);
        assert_eq!(profiles.directory(), directory);
        assert_eq!(
            fs::read_to_string(directory.join("profile.txt")).unwrap(),
            "name=Default Profile\nuser_name=Ada\nuser_email=ada@example.com\n"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("config.txt")).unwrap(),
            format!("profile_guid={}\nkeep_git_config=1\n", profile.guid)
        );
        // The legacy file moved into the profile.
        assert!(!root.path().join("tabs.txt").exists());
        assert_eq!(
            fs::read_to_string(directory.join("tabs.txt")).unwrap(),
            "legacy tabs"
        );

        // The next load finds it again, whatever author it is given.
        let again = Profiles::load(root.path(), "Someone", "else@example.com", &[]);
        assert_eq!(again.all(), [profile]);
    }

    #[test]
    fn loads_what_is_on_disk() {
        let root = tempfile::tempdir().unwrap();
        let profiles_directory = root.path().join("profiles");
        let write = |guid: &str, text: &str| {
            fs::create_dir_all(profiles_directory.join(guid)).unwrap();
            fs::write(profiles_directory.join(guid).join("profile.txt"), text).unwrap();
        };
        write(
            "b1",
            "name=work\r\nuser_name=A=B\r\nuser_email=w@example.com\r\n",
        );
        write("a2", "name=Home\nuser_email=h@example.com");
        write("c3", "user_name=No Name\n"); // skipped: no name
        fs::write(profiles_directory.join("stray.txt"), "name=not a folder\n").unwrap();
        fs::write(
            root.path().join("config.txt"),
            "profile_guid=b1\nkeep_git_config=0\n",
        )
        .unwrap();

        let mut profiles = Profiles::load(root.path(), "", "", &[]);
        let names: Vec<&str> = profiles.all().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Home", "work"]); // case-insensitive order
        assert_eq!(profiles.current().guid, "b1");
        assert_eq!(profiles.current().user_name, "A=B");
        assert_eq!(profiles.find("a2").unwrap().user_name, "");
        assert!(profiles.find("c3").is_none());
        assert!(!profiles.keep_git_config());

        profiles.set_keep_git_config(true).unwrap();
        profiles.set_current("a2").unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("config.txt")).unwrap(),
            "profile_guid=a2\nkeep_git_config=1\n"
        );
        assert!(
            matches!(profiles.set_current("nope"), Err(ProfileError::NotFound(guid)) if guid == "nope")
        );
        assert_eq!(profiles.current().guid, "a2");
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_first_key() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("profiles").join("bom");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("profile.txt"),
            "\u{FEFF}name=Marked\r\nuser_name=A\r\n",
        )
        .unwrap();
        fs::write(root.path().join("config.txt"), "\u{FEFF}profile_guid=bom\n").unwrap();
        let profiles = Profiles::load(root.path(), "", "", &[]);
        assert_eq!(profiles.all().len(), 1);
        assert_eq!(profiles.current().guid, "bom");
        assert_eq!(profiles.current().name, "Marked");
        assert_eq!(profiles.current().user_name, "A");
    }

    #[test]
    fn a_failed_removal_leaves_a_loadable_profile() {
        let root = tempfile::tempdir().unwrap();
        let mut profiles = Profiles::load(root.path(), "Ada", "ada@example.com", &[]);
        let other = profiles.create(named("Other"), false).unwrap();
        let directory = profiles.directory_of(&other);
        fs::write(directory.join("a.txt"), "a").unwrap();
        fs::write(directory.join("z.txt"), "z").unwrap();
        // A file that cannot be deleted while it is held open (Windows) or sits in a read-only folder.
        #[cfg(windows)]
        let held = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::write(directory.join("m.txt"), "m").unwrap();
            fs::OpenOptions::new()
                .read(true)
                .share_mode(1) // FILE_SHARE_READ: no delete
                .open(directory.join("m.txt"))
                .unwrap()
        };
        #[cfg(unix)]
        let locked = {
            use std::os::unix::fs::PermissionsExt;
            let locked = directory.join("locked");
            fs::create_dir(&locked).unwrap();
            fs::write(locked.join("m.txt"), "m").unwrap();
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
            locked
        };

        assert!(profiles.remove(&other).is_err());
        assert!(profiles.find(&other).is_some());
        assert!(directory.join("profile.txt").exists());
        let reloaded = Profiles::load(root.path(), "", "", &[]);
        assert_eq!(
            reloaded.find(&other).map(|profile| profile.name.as_str()),
            Some("Other")
        );

        #[cfg(windows)]
        drop(held);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        }
        profiles.remove(&other).unwrap();
        assert!(profiles.find(&other).is_none());
        assert!(!directory.exists());
    }

    #[test]
    fn the_legacy_files_move_into_a_saved_profile() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("tabs.txt"), "t").unwrap();
        let profiles = Profiles::load(root.path(), "Ada", "a@example.com", &["tabs.txt"]);
        let directory = profiles.directory();
        assert!(directory.join("profile.txt").is_file());
        assert!(directory.join("tabs.txt").is_file());
        assert_eq!(Profiles::load(root.path(), "", "", &[]).all().len(), 1);
    }

    #[test]
    fn an_unknown_current_profile_falls_back_to_the_first() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("profiles").join("only");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("profile.txt"), "name=Only\n").unwrap();
        fs::write(root.path().join("config.txt"), "profile_guid=gone\n").unwrap();
        let profiles = Profiles::load(root.path(), "", "", &[]);
        assert_eq!(profiles.current().guid, "only");
        assert!(profiles.keep_git_config());
        assert_eq!(
            fs::read_to_string(root.path().join("config.txt")).unwrap(),
            "profile_guid=only\nkeep_git_config=1\n"
        );
    }

    #[test]
    fn create_update_remove() {
        let root = tempfile::tempdir().unwrap();
        let mut profiles = Profiles::load(root.path(), "Ada", "ada@example.com", &[]);
        let default_guid = profiles.current().guid.clone();
        fs::write(profiles.directory().join("preferences.txt"), "K\tV\n").unwrap();
        fs::create_dir_all(profiles.directory().join("themes")).unwrap();
        fs::write(profiles.directory().join("themes").join("mine.json"), "{}").unwrap();

        // A copy of the current profile's files, under its own name.
        let copy = profiles
            .create(
                Profile {
                    guid: "ignored".to_owned(),
                    name: "Copy".to_owned(),
                    user_name: "Line\r\nBreak".to_owned(),
                    user_email: String::new(),
                },
                true,
            )
            .unwrap();
        assert_ne!(copy, "ignored");
        let copy_directory = profiles.directory_of(&copy);
        assert_eq!(
            fs::read_to_string(copy_directory.join("preferences.txt")).unwrap(),
            "K\tV\n"
        );
        assert_eq!(
            fs::read_to_string(copy_directory.join("themes").join("mine.json")).unwrap(),
            "{}"
        );
        assert_eq!(
            fs::read_to_string(copy_directory.join("profile.txt")).unwrap(),
            "name=Copy\nuser_name=Line Break\nuser_email=\n"
        );

        // An unnamed one, starting empty.
        let unnamed = profiles.create(Profile::default(), false).unwrap();
        assert_eq!(profiles.find(&unnamed).unwrap().name, "New Profile");
        assert!(
            !profiles
                .directory_of(&unnamed)
                .join("preferences.txt")
                .exists()
        );
        let names: Vec<&str> = profiles.all().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Copy", "Default Profile", "New Profile"]);
        assert_eq!(profiles.current().guid, default_guid);

        // Renaming sorts again and is saved.
        let renamed = Profile {
            guid: unnamed.clone(),
            ..named("aardvark")
        };
        profiles.update(&renamed).unwrap();
        assert_eq!(profiles.all()[0], renamed);
        assert!(matches!(
            profiles.update(&named("x")),
            Err(ProfileError::NotFound(_))
        ));
        let reloaded = Profiles::load(root.path(), "", "", &[]);
        // (What is in memory keeps its line break; the file has the one-line form.)
        assert_eq!(reloaded.find(&copy).unwrap().user_name, "Line Break");
        let listed = |profiles: &Profiles| -> Vec<(String, String)> {
            let all = profiles.all().iter();
            all.map(|p| (p.guid.clone(), p.name.clone())).collect()
        };
        assert_eq!(listed(&reloaded), listed(&profiles));
        assert_eq!(reloaded.current().guid, default_guid);

        // Any profile but the current one can be deleted.
        assert!(matches!(
            profiles.remove(&default_guid),
            Err(ProfileError::IsCurrent)
        ));
        assert!(matches!(
            profiles.remove("nope"),
            Err(ProfileError::NotFound(_))
        ));
        profiles.remove(&copy).unwrap();
        assert!(profiles.find(&copy).is_none());
        assert!(!copy_directory.exists());
        assert_eq!(profiles.all().len(), 2);
    }
}
