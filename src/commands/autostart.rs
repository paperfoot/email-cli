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
fn render_plist(binary: &str, account: Option<&str>, interval: u64) -> String {
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
    <string>/tmp/email-cli-daemon.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/email-cli-daemon.log</string>
</dict>
</plist>
"#
    )
}

#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(target_os = "macos")]
fn launchctl(cmd: &str, arg: &str) -> Result<()> {
    let output = std::process::Command::new("launchctl")
        .args([cmd, arg])
        .output()
        .context("failed to run launchctl")?;
    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("launchctl {cmd} {arg} failed: {msg}");
    }
    Ok(())
}

/// Best-effort unload — ignores any error output (e.g. "not loaded" on first install).
#[cfg(target_os = "macos")]
fn launchctl_silent(cmd: &str, arg: &str) {
    let _ = std::process::Command::new("launchctl")
        .args([cmd, arg])
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
        let plist = render_plist(&binary, args.account.as_deref(), args.interval);
        let path = plist_path()?;
        std::fs::create_dir_all(path.parent().unwrap())?;

        // Unload any previous version so the new plist is picked up.
        launchctl_silent("unload", &path.to_string_lossy());
        std::fs::write(&path, plist).with_context(|| format!("failed to write {}", path.display()))?;
        launchctl("load", &path.to_string_lossy())?;

        let data = json!({
            "label": LABEL,
            "plist": path.display().to_string(),
            "binary": binary,
            "account": args.account,
            "interval": args.interval,
            "log": "/tmp/email-cli-daemon.log",
        });
        print_success_or(self.format, &data, |_| {
            println!("installed LaunchAgent {LABEL}");
            println!("  plist : {}", path.display());
            println!("  logs  : /tmp/email-cli-daemon.log");
            println!("  start : launchctl start {LABEL}  (auto at next login)");
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
        launchctl_silent("unload", &path.to_string_lossy());
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
