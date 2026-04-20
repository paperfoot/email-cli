use anyhow::Result;
use rusqlite::params;
use serde_json::json;

use crate::app::App;
use crate::cli::{ProfileAddArgs, ProfileTestArgs};
use crate::helpers::resolve_api_key;
use crate::keychain::{self, KEYCHAIN_SENTINEL};
use crate::models::ProfileRecord;
use crate::output::print_success_or;

impl App {
    pub fn profile_add(&self, args: ProfileAddArgs) -> Result<()> {
        let api_key = resolve_api_key(
            args.api_key,
            args.api_key_env,
            args.api_key_file,
            &args.api_key_name,
        )?;

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
            let where_ = if keychain::is_available() { "keychain" } else { "sqlite" };
            println!("saved profile {} ({})", args.name, where_);
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
