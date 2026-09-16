use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde_json::json;

use crate::app::App;
use crate::cli::{ProfileAddArgs, ProfileRemoveArgs, ProfileTestArgs};
use crate::helpers::resolve_api_key;
use crate::keychain::{self, KEYCHAIN_SENTINEL};
use crate::models::ProfileRecord;
use crate::output::print_success_or;

impl App {
    pub fn profile_add(&self, args: ProfileAddArgs) -> Result<()> {
        if args.name.trim().is_empty() {
            bail!("profile name must not be blank");
        }
        let api_key = resolve_api_key(
            args.api_key,
            args.api_key_env,
            args.api_key_file,
            &args.api_key_name,
        )?;

        // Validate the candidate key before mutating either Keychain or SQLite,
        // so a failed rotation cannot destroy a previously working profile.
        if args.validate {
            crate::resend::ResendClient::new(api_key.clone())?.list_domains()?;
        }

        // On macOS, store the real key in the Keychain and write a
        // sentinel into SQLite. On other platforms, fall back to the
        // legacy SQLite-resident key.
        let stored = if keychain::is_available() {
            keychain::store(&args.name, &api_key)?;
            KEYCHAIN_SENTINEL.to_string()
        } else {
            api_key
        };

        self.conn.execute(
            "
            INSERT INTO profiles (name, api_key, updated_at)
            VALUES (?1, ?2, CURRENT_TIMESTAMP)
            ON CONFLICT(name) DO UPDATE SET
                api_key = excluded.api_key,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![args.name, stored],
        )?;

        let data = json!({
            "name": args.name,
            "status": "saved",
            "storage": if keychain::is_available() { "keychain" } else { "sqlite" },
            "db_path": self.db_path.display().to_string(),
        });
        print_success_or(self.format, &data, |_d| {
            let where_ = if keychain::is_available() {
                "keychain"
            } else {
                "sqlite"
            };
            println!("saved profile {} ({})", args.name, where_);
        });

        Ok(())
    }

    pub fn profile_remove(&self, args: ProfileRemoveArgs) -> Result<()> {
        if !args.yes {
            bail!("--yes is required to remove a profile");
        }
        if args.name.trim().is_empty() {
            bail!("profile name must not be blank");
        }

        let tx = self.conn.unchecked_transaction()?;
        let stored = tx
            .query_row(
                "SELECT api_key FROM profiles WHERE name = ?1",
                params![args.name],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .with_context(|| format!("profile {} not found", args.name))?;
        let account_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM accounts WHERE profile_name = ?1",
            params![args.name],
            |row| row.get(0),
        )?;
        if account_count > 0 {
            bail!(
                "profile {} is used by {} account(s); move or remove those accounts first",
                args.name,
                account_count
            );
        }

        let keychain_secret = if stored == KEYCHAIN_SENTINEL {
            Some(keychain::load(&args.name)?)
        } else {
            None
        };
        tx.execute("DELETE FROM profiles WHERE name = ?1", params![args.name])?;
        if keychain_secret.is_some() {
            keychain::delete(&args.name)?;
        }
        if let Err(commit_error) = tx.commit() {
            if let Some(secret) = keychain_secret {
                keychain::store(&args.name, &secret).with_context(|| {
                    format!(
                        "database removal failed ({commit_error}); also failed to restore Keychain secret"
                    )
                })?;
            }
            return Err(commit_error.into());
        }

        let data = json!({
            "name": args.name,
            "removed": true,
            "storage": if stored == KEYCHAIN_SENTINEL { "keychain" } else { "sqlite" },
        });
        print_success_or(self.format, &data, |_data| {
            println!("removed profile {}", args.name);
        });
        Ok(())
    }

    pub fn profile_list(&self) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, created_at FROM profiles ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            Ok(ProfileRecord {
                name: row.get(0)?,
                created_at: row.get(1)?,
            })
        })?;
        let profiles = rows.collect::<std::result::Result<Vec<_>, _>>()?;

        print_success_or(self.format, &profiles, |profiles| {
            for profile in profiles {
                println!("{}", profile.name);
            }
        });

        Ok(())
    }

    pub fn profile_test(&self, args: ProfileTestArgs) -> Result<()> {
        let client = self.client_for_profile(&args.name)?;
        let domains = client.list_domains()?;

        print_success_or(self.format, &domains, |domains| {
            for domain in &domains.data {
                let sending = domain
                    .capabilities
                    .as_ref()
                    .and_then(|caps| caps.sending.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                let receiving = domain
                    .capabilities
                    .as_ref()
                    .and_then(|caps| caps.receiving.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                let status = domain
                    .status
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                println!(
                    "{} status={} sending={} receiving={}",
                    domain.name, status, sending, receiving
                );
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ProfileRemoveArgs;
    use crate::output::Format;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn test_app() -> (App, PathBuf) {
        let path =
            std::env::temp_dir().join(format!("email-cli-profile-test-{}.db", Uuid::new_v4()));
        let app = App::new(path.clone(), Format::Json).unwrap();
        (app, path)
    }

    #[test]
    fn add_rejects_blank_name_before_resolving_or_storing_a_key() {
        let (app, path) = test_app();
        let error = app
            .profile_add(crate::cli::ProfileAddArgs {
                name: "   ".into(),
                api_key: Some("test-key".into()),
                api_key_env: None,
                api_key_file: None,
                api_key_name: "RESEND_API_KEY".into(),
                validate: false,
            })
            .unwrap_err();
        assert!(error.to_string().contains("must not be blank"));
        let count: i64 = app
            .conn
            .query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn removal_refuses_referenced_profile_without_changes() {
        let (app, path) = test_app();
        app.conn
            .execute(
                "INSERT INTO profiles (name, api_key) VALUES ('used', 'test-key')",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO accounts (email, profile_name, is_default)
                 VALUES ('agent@example.com', 'used', 1)",
                [],
            )
            .unwrap();

        let error = app
            .profile_remove(ProfileRemoveArgs {
                name: "used".into(),
                yes: true,
            })
            .unwrap_err();
        assert!(error.to_string().contains("used by 1 account"));
        let count: i64 = app
            .conn
            .query_row(
                "SELECT COUNT(*) FROM profiles WHERE name = 'used'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn removal_deletes_unreferenced_sqlite_profile() {
        let (app, path) = test_app();
        app.conn
            .execute(
                "INSERT INTO profiles (name, api_key) VALUES ('unused', 'test-key')",
                [],
            )
            .unwrap();
        app.profile_remove(ProfileRemoveArgs {
            name: "unused".into(),
            yes: true,
        })
        .unwrap();
        let count: i64 = app
            .conn
            .query_row(
                "SELECT COUNT(*) FROM profiles WHERE name = 'unused'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}
