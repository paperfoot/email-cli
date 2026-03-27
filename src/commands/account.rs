use anyhow::{Result, anyhow, bail};
use rusqlite::{OptionalExtension, params};
use serde_json::json;

use crate::app::App;
use crate::cli::{AccountAddArgs, AccountUseArgs};
use crate::helpers::normalize_email;
use crate::output::print_success_or;

impl App {
    pub fn account_add(&self, args: AccountAddArgs) -> Result<()> {
        let email = normalize_email(&args.email);
        let domain = email
            .split('@')
            .nth(1)
            .ok_or_else(|| anyhow!("invalid email: {}", email))?;

        let client = self.client_for_profile(&args.profile)?;
        let domains = client.list_domains()?;
        let matched = domains
            .data
            .into_iter()
            .find(|item| item.name.eq_ignore_ascii_case(domain))
            .ok_or_else(|| {
                anyhow!(
                    "domain {} is not present in profile {}",
                    domain,
                    args.profile
                )
            })?;

        let sending = matched
            .capabilities
            .as_ref()
            .and_then(|caps| caps.sending.clone())
            .unwrap_or_else(|| "unknown".to_string());
        if sending != "enabled" {
            bail!(
                "domain {} is not send-enabled in profile {}",
                domain,
                args.profile
            );
        }

        let tx = self.conn.unchecked_transaction()?;
        let has_default = tx
            .query_row(
                "SELECT 1 FROM accounts WHERE is_default = 1 LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        let existing_default = tx
            .query_row(
                "SELECT is_default FROM accounts WHERE email = ?1",
                params![email.clone()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            == 1;
        let is_default = if args.default {
            true
        } else if existing_default {
            true
        } else {
            !has_default
        };
        if is_default {
            tx.execute("UPDATE accounts SET is_default = 0", [])?;
        }

        tx.execute(
            "
            INSERT INTO accounts (email, profile_name, display_name, signature, is_default, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
            ON CONFLICT(email) DO UPDATE SET
                profile_name = excluded.profile_name,
                display_name = excluded.display_name,
                signature = excluded.signature,
                is_default = excluded.is_default,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![
                email,
                args.profile,
                args.name,
                args.signature.as_deref().unwrap_or(""),
                if is_default { 1 } else { 0 }
            ],
        )?;
        tx.commit()?;

        let account = self.get_account(&email)?;

        print_success_or(self.format, &account, |_account| {
            println!(
                "saved account {} on profile {}{}",
                email,
                args.profile,
                if is_default { " (default)" } else { "" }
            );
        });

        Ok(())
    }

    pub fn account_list(&self) -> Result<()> {
        let accounts = self.list_accounts()?;

        print_success_or(self.format, &accounts, |accounts| {
            for account in accounts {
                let marker = if account.is_default { " *" } else { "" };
                println!("{} [{}]{}", account.email, account.profile_name, marker);
            }
        });

        Ok(())
    }

    pub fn account_use(&self, args: AccountUseArgs) -> Result<()> {
        let email = normalize_email(&args.email);
        self.get_account(&email)?;
        self.conn
            .execute("UPDATE accounts SET is_default = 0", [])?;
        self.conn.execute(
            "UPDATE accounts SET is_default = 1, updated_at = CURRENT_TIMESTAMP WHERE email = ?1",
            params![email],
        )?;

        let data = json!({"default_account": email});
        print_success_or(self.format, &data, |_d| {
            println!("default account {}", email);
        });

        Ok(())
    }
}
