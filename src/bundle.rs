//! Embedded .app bundle for native macOS notifications.
//!
//! The Swift notification helper (`notify.swift` compiled to `email-cli-notify`)
//! must run from a codesigned `.app` bundle for `UNUserNotificationCenter` to
//! work on macOS 26+. Release tarballs and `cargo install` ship only the main
//! binary, so we embed the bundle here and extract it to the user's data dir
//! on first daemon launch (or when the user runs `email-cli daemon install-helper`).

#[cfg(target_os = "macos")]
use anyhow::{Context, Result};
#[cfg(target_os = "macos")]
use std::fs;
#[cfg(target_os = "macos")]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
const INFO_PLIST: &[u8] = include_bytes!("../assets/EmailCLI.app/Contents/Info.plist");
#[cfg(target_os = "macos")]
const HELPER_BIN: &[u8] =
    include_bytes!("../assets/EmailCLI.app/Contents/MacOS/email-cli-notify");
#[cfg(target_os = "macos")]
const APP_ICON: &[u8] = include_bytes!("../assets/EmailCLI.app/Contents/Resources/AppIcon.icns");
#[cfg(target_os = "macos")]
const NOTIFICATION_SOUND: &[u8] =
    include_bytes!("../assets/EmailCLI.app/Contents/Resources/EmailCLI.aiff");
#[cfg(target_os = "macos")]
const CODE_RESOURCES: &[u8] =
    include_bytes!("../assets/EmailCLI.app/Contents/_CodeSignature/CodeResources");

/// Path where the bundle should live for the helper lookup in helpers.rs to find it.
#[cfg(target_os = "macos")]
pub fn install_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|base| base.join("email-cli"))
}

/// Return the bundle path if it's already on disk.
#[cfg(target_os = "macos")]
pub fn installed_bundle_path() -> Option<PathBuf> {
    let dir = install_dir()?.join("EmailCLI.app");
    if dir.join("Contents/MacOS/email-cli-notify").exists() {
        Some(dir)
    } else {
        None
    }
}

/// Extract the embedded bundle to `<data_local_dir>/email-cli/EmailCLI.app`.
/// Overwrites any existing bundle so upgrades pick up new helper builds.
#[cfg(target_os = "macos")]
pub fn install() -> Result<PathBuf> {
    let base = install_dir().context("could not resolve data_local_dir")?;
    let app = base.join("EmailCLI.app");
    let contents = app.join("Contents");

    fs::create_dir_all(contents.join("MacOS"))?;
    fs::create_dir_all(contents.join("Resources"))?;
    fs::create_dir_all(contents.join("_CodeSignature"))?;

    write_file(&contents.join("Info.plist"), INFO_PLIST, 0o644)?;
    write_file(&contents.join("MacOS/email-cli-notify"), HELPER_BIN, 0o755)?;
    write_file(&contents.join("Resources/AppIcon.icns"), APP_ICON, 0o644)?;
    write_file(
        &contents.join("Resources/EmailCLI.aiff"),
        NOTIFICATION_SOUND,
        0o644,
    )?;
    write_file(
        &contents.join("_CodeSignature/CodeResources"),
        CODE_RESOURCES,
        0o644,
    )?;

    Ok(app)
}

/// Install the bundle if it isn't already on disk. Safe to call repeatedly.
#[cfg(target_os = "macos")]
pub fn ensure_installed() -> Result<PathBuf> {
    if let Some(path) = installed_bundle_path() {
        return Ok(path);
    }
    install()
}

#[cfg(target_os = "macos")]
fn write_file(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_installed() -> anyhow::Result<std::path::PathBuf> {
    anyhow::bail!("notification bundle is macOS-only")
}

#[cfg(not(target_os = "macos"))]
pub fn install() -> anyhow::Result<std::path::PathBuf> {
    anyhow::bail!("notification bundle is macOS-only")
}
