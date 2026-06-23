//! Manage a macOS LaunchAgent that runs `email-cli daemon` at login.

use anyhow::{Context, Result, bail};
use serde_json::json;

use crate::app::App;
use crate::cli::{AutostartInstallArgs, AutostartStatusArgs};
use crate::output::print_success_or;

#[cfg(target_os = "macos")]
const LABEL: &str = "ai.paperfoot.email-cli.daemon";

#[cfg(target_os = "macos")]
fn plist_path() -> Result<std::path::PathBuf> {
    let home = dirs::home_dir().context("no home directory")?;
    Ok(home
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn render_plist(binary: &str, account: Option<&str>, interval: u64, log_path: &str) -> String {
    let mut args = format!(
        "        <string>{}</string>\n        <string>daemon</string>\n",
        xml_escape(binary)
    );
    if let Some(acct) = account {
        args.push_str(&format!(
            "        <string>--account</string>\n        <string>{}</string>\n",
            xml_escape(acct)
        ));
    }
    args.push_str(&format!(
        "        <string>--interval</string>\n        <string>{interval}</string>\n"
    ));

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
{args}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
        log = xml_escape(log_path)
    )
}

/// Per-user, agent-owned daemon log under ~/Library/Logs (Apple's convention).
/// Replaces the old fixed /tmp/email-cli-daemon.log, which lived in a
/// world-writable directory: a local attacker could pre-create that predictable
/// name as a symlink (launchd follows symlinks on O_CREAT), and on a multi-user
/// Mac the first user to create it owned it, silently breaking logging for the
/// rest.
#[cfg(target_os = "macos")]
fn log_path() -> Result<std::path::PathBuf> {
    let home = dirs::home_dir().context("no home directory")?;
    Ok(home.join("Library/Logs/email-cli/daemon.log"))
}

/// `gui/<uid>` domain for the current login session — the modern launchd
/// target. Replaces legacy `launchctl load/unload`, which is deprecated and
/// silently no-ops outside a proper GUI login session.
#[cfg(target_os = "macos")]
fn gui_domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

#[cfg(target_os = "macos")]
fn service_target() -> String {
    format!("{}/{LABEL}", gui_domain())
}

#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(target_os = "macos")]
fn launchctl(args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("launchctl")
        .args(args)
        .output()
        .context("failed to run launchctl")?;
    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("launchctl {} failed: {msg}", args.join(" "));
    }
    Ok(())
}

/// Best-effort launchctl call — ignores any error output (e.g. "not bootstrapped"
/// when removing a service that was never loaded).
#[cfg(target_os = "macos")]
fn launchctl_silent(args: &[&str]) {
    let _ = std::process::Command::new("launchctl")
        .args(args)
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status();
}

impl App {
    #[cfg(target_os = "macos")]
    pub fn autostart_install(&self, args: AutostartInstallArgs) -> Result<()> {
        let binary = std::env::current_exe()
            .context("could not resolve current executable path")?
            .to_string_lossy()
            .to_string();

        // Warn when the resolved binary lives inside an app bundle: a DMG /
        // drag-install replaces the whole bundle on update, so the baked path
        // becomes stale and the daemon silently never starts. (current_exe
        // canonicalizes symlinks, so pointing at a PATH symlink wouldn't help.)
        let bundle_warning = if binary.contains("/Contents/MacOS/") || binary.contains(".app/") {
            Some(format!(
                "email-cli is running from inside an app bundle ({binary}); the LaunchAgent path will break when the app is updated or moved. Re-run `email-cli autostart install` after each update, or install a standalone email-cli outside the bundle and run autostart from that."
            ))
        } else {
            None
        };

        // Per-user log directory, locked to the owner.
        let log = log_path()?;
        let log_dir = log.parent().unwrap();
        std::fs::create_dir_all(log_dir)
            .with_context(|| format!("failed to create {}", log_dir.display()))?;
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(log_dir, std::fs::Permissions::from_mode(0o700));
        }
        let log_str = log.to_string_lossy().to_string();

        let plist = render_plist(&binary, args.account.as_deref(), args.interval, &log_str);
        let path = plist_path()?;
        std::fs::create_dir_all(path.parent().unwrap())?;

        // Replace any previous instance, then bootstrap the new plist into the
        // GUI domain (modern launchd; load/unload are deprecated). bootstrap
        // errors if already loaded, so bootout first (best-effort).
        let domain = gui_domain();
        let target = service_target();
        launchctl_silent(&["bootout", &target]);
        std::fs::write(&path, plist).with_context(|| format!("failed to write {}", path.display()))?;
        launchctl(&["bootstrap", &domain, &path.to_string_lossy()])?;
        // Start now so the daemon comes up without waiting for the next login.
        launchctl_silent(&["kickstart", "-k", &target]);

        let data = json!({
            "label": LABEL,
            "plist": path.display().to_string(),
            "binary": binary,
            "account": args.account,
            "interval": args.interval,
            "log": log_str,
            "warning": bundle_warning.clone(),
        });
        print_success_or(self.format, &data, |_| {
            println!("installed LaunchAgent {LABEL}");
            println!("  plist : {}", path.display());
            println!("  logs  : {log_str}");
            println!("  start : launchctl kickstart {target}  (auto at next login)");
            if let Some(w) = &bundle_warning {
                eprintln!("  warning: {w}");
            }
        });
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub fn autostart_uninstall(&self) -> Result<()> {
        let path = plist_path()?;
        if !path.exists() {
            print_success_or(
                self.format,
                &json!({"status": "not_installed"}),
                |_| println!("LaunchAgent not installed"),
            );
            return Ok(());
        }
        launchctl_silent(&["bootout", &service_target()]);
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
        print_success_or(self.format, &json!({"status": "uninstalled"}), |_| {
            println!("removed LaunchAgent {LABEL}");
        });
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub fn autostart_status(&self, _args: AutostartStatusArgs) -> Result<()> {
        let path = plist_path()?;
        let installed = path.exists();
        let loaded = std::process::Command::new("launchctl")
            .args(["list", LABEL])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        let data = json!({
            "label": LABEL,
            "plist": path.display().to_string(),
            "installed": installed,
            "loaded": loaded,
        });
        print_success_or(self.format, &data, |_| {
            let state = match (installed, loaded) {
                (true, true) => "installed and loaded",
                (true, false) => "installed but not loaded",
                _ => "not installed",
            };
            println!("LaunchAgent {LABEL}: {state}");
            println!("  plist: {}", path.display());
        });
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn autostart_install(&self, _args: AutostartInstallArgs) -> Result<()> {
        bail!("autostart is macOS-only")
    }

    #[cfg(not(target_os = "macos"))]
    pub fn autostart_uninstall(&self) -> Result<()> {
        bail!("autostart is macOS-only")
    }

    #[cfg(not(target_os = "macos"))]
    pub fn autostart_status(&self, _args: AutostartStatusArgs) -> Result<()> {
        bail!("autostart is macOS-only")
    }
}
