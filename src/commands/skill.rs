use anyhow::Result;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

use crate::output::{Format, print_success_or};

const SKILL_CONTENT: &str = "\
---
name: email-cli
description: >
  Agent-friendly email CLI for Resend. Run `email-cli agent-info` for full
  capabilities, flags, and exit codes.
---

## email-cli

Local-first email CLI backed by Resend. Manages profiles, accounts, signatures,
sending, receiving, drafts, attachments, and inbox sync.

Run `email-cli agent-info` for the machine-readable capability manifest.
";

fn skill_dirs() -> Vec<(&'static str, PathBuf)> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    vec![
        ("claude", home.join(".claude/skills/email-cli")),
        ("codex", home.join(".codex/skills/email-cli")),
        ("gemini", home.join(".gemini/skills/email-cli")),
    ]
}

pub fn install(format: Format) -> Result<()> {
    let mut installed = Vec::new();

    for (platform, dir) in skill_dirs() {
        fs::create_dir_all(&dir)?;
        let path = dir.join("SKILL.md");
        fs::write(&path, SKILL_CONTENT)?;
        installed.push(json!({
            "platform": platform,
            "path": path.display().to_string(),
        }));
    }

    let data = json!({
        "installed": installed,
    });
    print_success_or(format, &data, |_d| {
        for entry in &installed {
            println!(
                "installed {} -> {}",
                entry["platform"].as_str().unwrap_or("?"),
                entry["path"].as_str().unwrap_or("?"),
            );
        }
    });

    Ok(())
}

pub fn status(format: Format) -> Result<()> {
    let mut statuses = Vec::new();

    for (platform, dir) in skill_dirs() {
        let path = dir.join("SKILL.md");
        let exists = path.exists();
        statuses.push(json!({
            "platform": platform,
            "path": path.display().to_string(),
            "installed": exists,
        }));
    }

    let data = json!({
        "platforms": statuses,
    });
    print_success_or(format, &data, |_d| {
        for entry in &statuses {
            let marker = if entry["installed"].as_bool().unwrap_or(false) {
                "ok"
            } else {
                "missing"
            };
            println!(
                "{}: {} ({})",
                entry["platform"].as_str().unwrap_or("?"),
                marker,
                entry["path"].as_str().unwrap_or("?"),
            );
        }
    });

    Ok(())
}
