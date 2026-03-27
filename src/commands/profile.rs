use anyhow::Result;
use rusqlite::params;
use serde_json::json;

use crate::app::App;
use crate::cli::{ProfileAddArgs, ProfileTestArgs};
use crate::helpers::resolve_api_key;
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

        self.conn.execute(
            "
            INSERT INTO profiles (name, api_key, updated_at)
            VALUES (?1, ?2, CURRENT_TIMESTAMP)
            ON CONFLICT(name) DO UPDATE SET
                api_key = excluded.api_key,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![args.name, api_key],
        )?;

        let data = json!({
            "name": args.name,
            "status": "saved",
            "db_path": self.db_path.display().to_string(),
        });
        print_success_or(self.format, &data, |_d| {
            println!("saved profile {}", args.name);
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
