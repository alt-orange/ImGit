//! ImGit's build tasks, run as `cargo xtask <task>`.
//!
//! * `release`: builds the release executable (`cargo build --release -p imgit`) and packs it in place with
//!   UPX, which must be on `PATH`. Extra arguments go to `cargo build` (`cargo xtask release --locked`).

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// What `upx` is asked to do: the smallest output the Windows and Linux release packages use.
const UPX_PACK_ARGS: [&str; 2] = ["--best", "--lzma"];

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let result = match args.next().as_deref().and_then(|task| task.to_str()) {
        Some("release") => release(args.collect()),
        _ => Err(Error::Usage),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
enum Error {
    Usage,
    /// A tool could not be started.
    Spawn {
        tool: &'static str,
        source: io::Error,
    },
    /// A tool ran and reported failure.
    Failed(&'static str),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => write!(f, "usage: cargo xtask release [cargo build arguments]"),
            Self::Spawn {
                tool: "upx",
                source,
            } => write!(
                f,
                "could not run upx ({source}); install UPX (https://upx.github.io) and put it on PATH"
            ),
            Self::Spawn { tool, source } => write!(f, "could not run {tool}: {source}"),
            Self::Failed(tool) => write!(f, "{tool} failed"),
            Self::Io(source) => write!(f, "{source}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(source: io::Error) -> Self {
        Self::Io(source)
    }
}

/// Builds the release executable and packs it with UPX.
fn release(build_args: Vec<OsString>) -> Result<(), Error> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let root = workspace_root();
    run(
        "cargo",
        Command::new(cargo)
            .current_dir(&root)
            .args(["build", "--release", "--package", "imgit"])
            .args(build_args),
    )?;

    let executable = release_directory(&root).join(format!("imgit{}", env::consts::EXE_SUFFIX));
    let before = executable.metadata()?.len();
    // Cargo normally puts its own (unpacked) executable back on every build, so this is a safety net: UPX
    // refuses to pack a file twice.
    if is_packed(&executable)? {
        println!(
            "{} is already packed ({})",
            executable.display(),
            megabytes(before)
        );
        return Ok(());
    }
    run(
        "upx",
        Command::new("upx").args(UPX_PACK_ARGS).arg(&executable),
    )?;
    // The packed file must unpack to what went in.
    run("upx", Command::new("upx").arg("-t").arg(&executable))?;
    let after = executable.metadata()?.len();
    println!(
        "{}: {} -> {}",
        executable.display(),
        megabytes(before),
        megabytes(after)
    );
    Ok(())
}

/// The workspace's folder: two above this crate's.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/xtask lies two folders below the workspace root")
        .to_path_buf()
}

/// Where `cargo build --release` puts the executable: under `CARGO_TARGET_DIR` when set, else `target`.
fn release_directory(root: &Path) -> PathBuf {
    let target = env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("release")
}

/// Whether UPX has packed `executable` already (`upx -t` succeeds only on a packed file).
fn is_packed(executable: &Path) -> Result<bool, Error> {
    let status = Command::new("upx")
        .args(["-t", "-qq"])
        .arg(executable)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|source| Error::Spawn {
            tool: "upx",
            source,
        })?;
    Ok(status.success())
}

fn run(tool: &'static str, command: &mut Command) -> Result<(), Error> {
    let status = command
        .status()
        .map_err(|source| Error::Spawn { tool, source })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Failed(tool))
    }
}

fn megabytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // a file size, shown to two decimals
    let megabytes = bytes as f64 / (1024.0 * 1024.0);
    format!("{megabytes:.2} MB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workspace_root_holds_the_workspace_manifest() {
        assert!(workspace_root().join("Cargo.toml").is_file());
        assert!(workspace_root().join("crates").join("xtask").is_dir());
    }

    #[test]
    fn sizes_read_in_megabytes() {
        assert_eq!(megabytes(8_619_008), "8.22 MB");
        assert_eq!(megabytes(0), "0.00 MB");
    }
}
