//! Compiles every file under the repository's `assets/` directory into the executable: writes the table
//! `src/assets.rs` includes, one `(path, bytes)` entry per file, sorted by path. Cargo re-runs this when a file
//! is added, removed or changed. On Windows it also embeds the executable's icon and version information
//! (the C++ build's `res/imgit.rc`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

fn main() -> io::Result<()> {
    embed_windows_resources()?;
    omit_debug_info_from_release();
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let assets = manifest_dir.join("../../assets").canonicalize()?;
    println!("cargo:rerun-if-changed={}", assets.display());

    let mut files = Vec::new();
    collect(&assets, &mut files)?;
    files.sort();

    let mut table = String::from("&[\n");
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        let relative = file.strip_prefix(&assets).expect("found under assets/");
        let name = relative.to_string_lossy().replace('\\', "/");
        writeln!(
            table,
            "    ({name:?}, include_bytes!({:?}).as_slice()),",
            file.display().to_string()
        )
        .expect("writing to a String");
    }
    table.push_str("]\n");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo"));
    fs::write(out_dir.join("embedded_assets.rs"), table)
}

/// Every regular file under `directory`, recursively.
fn collect(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            println!("cargo:rerun-if-changed={}", path.display());
            collect(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// The executable's icon (Explorer, the taskbar) and its version information (from the package's metadata).
#[cfg(windows)]
fn embed_windows_resources() -> io::Result<()> {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    let icon = Path::new("../../res/imgit.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(&icon.to_string_lossy())
        .set("FileDescription", "ImGit Desktop")
        .set("ProductName", "ImGit Desktop")
        .set("InternalName", "imgit")
        .set("OriginalFilename", "imgit.exe");
    resource.compile()
}

#[cfg(not(windows))]
#[expect(clippy::unnecessary_wraps, reason = "the Windows build can fail")]
fn embed_windows_resources() -> io::Result<()> {
    Ok(())
}

/// A release executable carries no debug info at all. `strip = "symbols"` in the release profile is enough for
/// the GNU and Apple linkers; the MSVC linker still writes a `.pdb` and a debug directory pointing at it unless
/// it is told `/DEBUG:NONE`.
fn omit_debug_info_from_release() {
    let release = env::var("PROFILE").is_ok_and(|profile| profile == "release");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|target_env| target_env == "msvc");
    if release && msvc {
        println!("cargo:rustc-link-arg-bins=/DEBUG:NONE");
    }
}
