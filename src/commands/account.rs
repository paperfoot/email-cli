use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{OptionalExtension, params};
use serde_json::json;

use crate::app::App;
use crate::cli::{AccountAddArgs, AccountEditArgs, AccountRemoveArgs, AccountUseArgs};
use crate::helpers::normalize_email;
use crate::output::print_success_or;

impl App {
    fn validate_account_domain(&self, email: &str, profile: &str) -> Result<()> {
        let domain = email
            .split('@')
            .nth(1)
            .filter(|domain| !domain.is_empty())
            .ok_or_else(|| anyhow!("invalid email: {}", email))?;

        let client = self.client_for_profile(profile)?;
        let domains = client.list_domains()?;
        let matched = domains
            .data
            .into_iter()
            .find(|item| item.name.eq_ignore_ascii_case(domain))
            .ok_or_else(|| anyhow!("domain {} is not present in profile {}", domain, profile))?;
        let sending = matched
            .capabilities
            .as_ref()
            .and_then(|caps| caps.sending.clone())
            .unwrap_or_else(|| "unknown".to_string());
        if sending != "enabled" {
            bail!(
                "domain {} is not send-enabled in profile {}",
                domain,
                profile
            );
        }
        Ok(())
    }

    fn persist_account_add(&self, email: &str, args: &AccountAddArgs) -> Result<bool> {
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
                params![email],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            == 1;
        let is_default = args.default || existing_default || !has_default;
        if is_default {
            tx.execute("UPDATE accounts SET is_default = 0", [])?;
        }

        // Omitted identity fields preserve the existing value on conflict.
        // An explicitly supplied empty string is a value and clears the field.
        tx.execute(
            "
            INSERT INTO accounts (
                email, profile_name, display_name, signature, is_default, updated_at
            ) VALUES (?1, ?2, ?3, COALESCE(?4, ''), ?5, CURRENT_TIMESTAMP)
            ON CONFLICT(email) DO UPDATE SET
                profile_name = excluded.profile_name,
                display_name = COALESCE(?3, accounts.display_name),
                signature = COALESCE(?4, accounts.signature),
                is_default = excluded.is_default,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![
                email,
                args.profile,
                args.name,
                args.signature,
                if is_default { 1 } else { 0 }
            ],
        )?;
        tx.commit()?;
        Ok(is_default)
    }

    pub fn account_add(&self, args: AccountAddArgs) -> Result<()> {
        let email = normalize_email(&args.email);
        self.validate_account_domain(&email, &args.profile)?;
        let is_default = self.persist_account_add(&email, &args)?;
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
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE accounts SET is_default = 0", [])?;
        let changed = tx.execute(
            "UPDATE accounts SET is_default = 1, updated_at = CURRENT_TIMESTAMP WHERE email = ?1",
            params![email],
        )?;
        if changed != 1 {
            bail!("account {} not found", email);
        }
        tx.commit()?;

        let data = json!({"default_account": email});
        print_success_or(self.format, &data, |_d| {
            println!("default account {}", email);
        });
        Ok(())
    }

    pub fn account_edit(&self, args: AccountEditArgs) -> Result<()> {
        let email = normalize_email(&args.email);
        let existing = self.get_account(&email)?;

        if let Some(profile) = args.profile.as_deref() {
            if profile.trim().is_empty() {
                bail!("profile name must not be blank");
            }
            // Keep local identity edits available offline.
            if profile != existing.profile_name {
                self.validate_account_domain(&email, profile)?;
            }
        }

        let tx = self.conn.unchecked_transaction()?;
        if let Some(name) = args.name.as_deref() {
            tx.execute(
                "UPDATE accounts SET display_name = NULLIF(?1, ''), updated_at = CURRENT_TIMESTAMP
                 WHERE email = ?2",
                params![name, email],
            )?;
        }
        if let Some(profile) = args.profile.as_deref() {
            tx.execute(
                "UPDATE accounts SET profile_name = ?1, updated_at = CURRENT_TIMESTAMP
                 WHERE email = ?2",
                params![profile, email],
            )?;
        }
        tx.commit()?;

        let account = self.get_account(&email)?;
        print_success_or(self.format, &account, |_account| {
            println!("updated account {}", email);
        });
        Ok(())
    }

    pub fn account_remove(&self, args: AccountRemoveArgs) -> Result<()> {
        if !args.yes {
            bail!("--yes is required to remove an account");
        }
        let email = normalize_email(&args.email);
        let tx = self.conn.unchecked_transaction()?;
        let was_default = tx
            .query_row(
                "SELECT is_default FROM accounts WHERE email = ?1",
                params![email],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .with_context(|| format!("account {} not found", email))?
            == 1;

        let blocked: i64 = tx.query_row(
            "SELECT COUNT(*) FROM outbox WHERE account_email = ?1 AND status <> 'sent'",
            params![email],
            |row| row.get(0),
        )?;
        if blocked > 0 {
            bail!(
                "account {} has {} unsent outbox item(s); send or resolve them before removal",
                email,
                blocked
            );
        }

        let drafts_deleted = tx.execute(
            "DELETE FROM drafts WHERE account_email = ?1",
            params![email],
        )?;
        let reply_links_cleared = tx.execute(
            "UPDATE drafts SET reply_to_message_id = NULL, updated_at = CURRENT_TIMESTAMP
             WHERE account_email <> ?1
               AND reply_to_message_id IN (SELECT id FROM messages WHERE account_email = ?1)",
            params![email],
        )?;
        let messages_deleted = tx.execute(
            "DELETE FROM messages WHERE account_email = ?1",
            params![email],
        )?;
        let sync_rows_deleted = tx.execute(
            "DELETE FROM sync_state WHERE account_email = ?1",
            params![email],
        )?;
        let outbox_rows_deleted = tx.execute(
            "DELETE FROM outbox WHERE account_email = ?1 AND status = 'sent'",
            params![email],
        )?;
        tx.execute("DELETE FROM accounts WHERE email = ?1", params![email])?;

        let replacement_default = if was_default {
            let next = tx
                .query_row(
                    "SELECT email FROM accounts ORDER BY email COLLATE NOCASE LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(next_email) = next.as_deref() {
                tx.execute(
                    "UPDATE accounts SET is_default = 1, updated_at = CURRENT_TIMESTAMP
                     WHERE email = ?1",
                    params![next_email],
                )?;
            }
            next
        } else {
            None
        };
        tx.commit()?;

        let data = json!({
            "email": email,
            "removed": true,
            "remote_mailbox": "unchanged",
            "replacement_default": replacement_default,
            "deleted": {
                "drafts": drafts_deleted,
                "messages": messages_deleted,
                "sync_state": sync_rows_deleted,
                "completed_outbox": outbox_rows_deleted,
            },
            "reply_links_cleared": reply_links_cleared,
        });
        print_success_or(self.format, &data, |_data| {
            println!("removed local account {} (remote mailbox unchanged)", email);
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Format;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn test_app() -> (App, PathBuf) {
        let path =
            std::env::temp_dir().join(format!("email-cli-account-test-{}.db", Uuid::new_v4()));
        let app = App::new(path.clone(), Format::Json).unwrap();
        app.conn
            .execute(
                "INSERT INTO profiles (name, api_key) VALUES ('default', 'test'), ('other', 'test')",
                [],
            )
            .unwrap();
        (app, path)
    }

    fn seed_account(app: &App, email: &str, is_default: bool) {
        app.conn
            .execute(
                "INSERT INTO accounts (email, profile_name, display_name, signature, is_default)
                 VALUES (?1, 'default', 'Existing Name', 'Existing signature', ?2)",
                params![email, if is_default { 1 } else { 0 }],
            )
            .unwrap();
    }

    #[test]
    fn add_conflict_preserves_omitted_identity_fields_and_allows_explicit_clear() {
        let (app, path) = test_app();
        seed_account(&app, "agent@example.com", true);
        let mut args = AccountAddArgs {
            email: "agent@example.com".into(),
            profile: "other".into(),
            name: None,
            signature: None,
            default: false,
        };
        app.persist_account_add("agent@example.com", &args).unwrap();
        let saved = app.get_account("agent@example.com").unwrap();
        assert_eq!(saved.profile_name, "other");
        assert_eq!(saved.display_name.as_deref(), Some("Existing Name"));
        assert_eq!(saved.signature, "Existing signature");

        args.name = Some(String::new());
        args.signature = Some(String::new());
        app.persist_account_add("agent@example.com", &args).unwrap();
        let cleared = app.get_account("agent@example.com").unwrap();
        assert_eq!(cleared.display_name.as_deref(), Some(""));
        assert_eq!(cleared.signature, "");
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn local_name_edit_and_noop_do_not_need_remote_access() {
        let (app, path) = test_app();
        seed_account(&app, "agent@example.com", true);
        app.account_edit(AccountEditArgs {
            email: "agent@example.com".into(),
            name: Some("Local Name".into()),
            profile: None,
        })
        .unwrap();
        assert_eq!(
            app.get_account("agent@example.com")
                .unwrap()
                .display_name
                .as_deref(),
            Some("Local Name")
        );

        app.account_edit(AccountEditArgs {
            email: "agent@example.com".into(),
            name: Some(String::new()),
            profile: None,
        })
        .unwrap();
        assert_eq!(
            app.get_account("agent@example.com").unwrap().display_name,
            None
        );

        app.account_edit(AccountEditArgs {
            email: "agent@example.com".into(),
            name: None,
            profile: None,
        })
        .unwrap();
        assert_eq!(
            app.get_account("agent@example.com").unwrap().profile_name,
            "default"
        );
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unchanged_profile_and_name_edit_succeeds_offline() {
        let (app, path) = test_app();
        seed_account(&app, "agent@example.com", true);

        app.account_edit(AccountEditArgs {
            email: "agent@example.com".into(),
            name: Some("Offline Name".into()),
            profile: Some("default".into()),
        })
        .unwrap();

        let saved = app.get_account("agent@example.com").unwrap();
        assert_eq!(saved.display_name.as_deref(), Some("Offline Name"));
        assert_eq!(saved.profile_name, "default");
        assert!(saved.is_default);
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn selecting_missing_default_preserves_existing_default() {
        let (app, path) = test_app();
        seed_account(&app, "old@example.com", true);
        seed_account(&app, "other@example.com", false);

        let error = app
            .account_use(AccountUseArgs {
                email: "missing@example.com".into(),
            })
            .unwrap_err();

        assert!(error.to_string().contains("not found"));
        assert!(app.get_account("old@example.com").unwrap().is_default);
        assert!(!app.get_account("other@example.com").unwrap().is_default);
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failed_default_update_rolls_back_cleared_default() {
        let (app, path) = test_app();
        seed_account(&app, "old@example.com", true);
        seed_account(&app, "target@example.com", false);
        app.conn
            .execute_batch(
                "CREATE TRIGGER force_default_update_failure
                 BEFORE UPDATE OF is_default ON accounts
                 WHEN NEW.email = 'target@example.com' AND NEW.is_default = 1
                 BEGIN
                     SELECT RAISE(ABORT, 'forced default update failure');
                 END;",
            )
            .unwrap();

        let error = app
            .account_use(AccountUseArgs {
                email: "target@example.com".into(),
            })
            .unwrap_err();

        assert!(error.to_string().contains("forced default update failure"));
        assert!(app.get_account("old@example.com").unwrap().is_default);
        assert!(!app.get_account("target@example.com").unwrap().is_default);
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn removal_is_blocked_without_changes_when_outbox_is_not_sent() {
        let (app, path) = test_app();
        seed_account(&app, "agent@example.com", true);
        app.conn
            .execute(
                "INSERT INTO outbox (id, account_email, request_json, idempotency_key, status)
                 VALUES ('pending-one', 'agent@example.com', '{}', 'key', 'pending')",
                [],
            )
            .unwrap();
        let error = app
            .account_remove(AccountRemoveArgs {
                email: "agent@example.com".into(),
                yes: true,
            })
            .unwrap_err();
        assert!(error.to_string().contains("unsent outbox"));
        assert!(app.get_account("agent@example.com").is_ok());
        let count: i64 = app
            .conn
            .query_row("SELECT COUNT(*) FROM outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn removal_rolls_back_all_prior_changes_when_a_delete_fails() {
        let (app, path) = test_app();
        seed_account(&app, "z@example.com", true);
        seed_account(&app, "a@example.com", false);
        app.conn
            .execute(
                "INSERT INTO messages (id, remote_id, direction, account_email, from_addr,
                    to_json, cc_json, bcc_json, reply_to_json, subject, created_at, raw_json)
                 VALUES (42, 'remote', 'received', 'z@example.com', 'sender@example.com',
                    '[]', '[]', '[]', '[]', 'subject', '2026-01-01T00:00:00Z', '{}')",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, cc_json, bcc_json, reply_to_json,
                    attachment_paths_json)
                 VALUES ('own', 'z@example.com', '[]', '[]', '[]', '[]', '[]')",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, cc_json, bcc_json, reply_to_json,
                    reply_to_message_id, attachment_paths_json)
                 VALUES ('other', 'a@example.com', '[]', '[]', '[]', '[]', 42, '[]')",
                [],
            )
            .unwrap();
        app.conn
            .execute_batch(
                "CREATE TRIGGER force_message_delete_failure
                 BEFORE DELETE ON messages
                 BEGIN
                     SELECT RAISE(ABORT, 'forced message delete failure');
                 END;",
            )
            .unwrap();

        let error = app
            .account_remove(AccountRemoveArgs {
                email: "z@example.com".into(),
                yes: true,
            })
            .unwrap_err();
        assert!(error.to_string().contains("forced message delete failure"));
        assert!(app.get_account("z@example.com").is_ok());
        let own_draft_count: i64 = app
            .conn
            .query_row("SELECT COUNT(*) FROM drafts WHERE id = 'own'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(own_draft_count, 1);
        let link: Option<i64> = app
            .conn
            .query_row(
                "SELECT reply_to_message_id FROM drafts WHERE id = 'other'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(link, Some(42));
        let message_count: i64 = app
            .conn
            .query_row("SELECT COUNT(*) FROM messages WHERE id = 42", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(message_count, 1);
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn removal_cleans_local_rows_and_assigns_alphabetical_default() {
        let (app, path) = test_app();
        seed_account(&app, "z@example.com", true);
        seed_account(&app, "b@example.com", false);
        seed_account(&app, "a@example.com", false);
        app.conn
            .execute(
                "INSERT INTO messages (id, remote_id, direction, account_email, from_addr,
                    to_json, cc_json, bcc_json, reply_to_json, subject, created_at, raw_json)
                 VALUES (42, 'remote', 'received', 'z@example.com', 'sender@example.com',
                    '[]', '[]', '[]', '[]', 'subject', '2026-01-01T00:00:00Z', '{}')",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO attachments (message_id, filename, raw_json)
                 VALUES (42, 'file.pdf', '{}')",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, cc_json, bcc_json, reply_to_json,
                    attachment_paths_json)
                 VALUES ('own', 'z@example.com', '[]', '[]', '[]', '[]', '[]')",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO drafts (id, account_email, to_json, cc_json, bcc_json, reply_to_json,
                    reply_to_message_id, attachment_paths_json)
                 VALUES ('other', 'a@example.com', '[]', '[]', '[]', '[]', 42, '[]')",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO sync_state (account_email, direction)
                 VALUES ('z@example.com', 'received')",
                [],
            )
            .unwrap();
        app.conn
            .execute(
                "INSERT INTO outbox (id, account_email, request_json, idempotency_key, status)
                 VALUES ('done', 'z@example.com', '{}', 'done-key', 'sent')",
                [],
            )
            .unwrap();

        app.account_remove(AccountRemoveArgs {
            email: "z@example.com".into(),
            yes: true,
        })
        .unwrap();
        assert!(app.get_account("z@example.com").is_err());
        assert!(app.get_account("a@example.com").unwrap().is_default);
        assert!(!app.get_account("b@example.com").unwrap().is_default);
        let link: Option<i64> = app
            .conn
            .query_row(
                "SELECT reply_to_message_id FROM drafts WHERE id = 'other'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(link, None);
        for table in ["messages", "attachments", "sync_state", "outbox"] {
            let sql = format!("SELECT COUNT(*) FROM {}", table);
            let count: i64 = app.conn.query_row(&sql, [], |row| row.get(0)).unwrap();
            assert_eq!(count, 0, "expected {table} rows to be deleted");
        }
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}
