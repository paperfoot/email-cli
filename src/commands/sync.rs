use anyhow::{Result, bail};
use std::collections::BTreeSet;

use crate::app::App;
use crate::cli::SyncArgs;
use crate::helpers::{normalize_email, received_email_matches_account};
use crate::models::{AccountRecord, SyncSummary};
use crate::output::{Format, print_success_or};
use crate::resend::ResendClient;

impl App {
    pub fn sync(&self, args: SyncArgs) -> Result<()> {
        let account_filter = args.account.clone();
        let limit = args.limit;
        let watch = args.watch;
        let interval = args.interval.unwrap_or(60);

        self.sync_once(account_filter.as_deref(), limit)?;

        if watch {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(interval));
                if matches!(self.format, Format::Human) {
                    eprintln!("polling...");
                }
                self.sync_once(account_filter.as_deref(), limit)?;
            }
        }

        Ok(())
    }

    fn sync_once(&self, account_filter: Option<&str>, limit: usize) -> Result<()> {
        let accounts = if let Some(account) = account_filter {
            vec![self.get_account(&normalize_email(account))?]
        } else {
            self.list_accounts()?
        };
        if accounts.is_empty() {
            bail!("no accounts configured");
        }

        let unique_profiles = accounts
            .iter()
            .map(|account| account.profile_name.clone())
            .collect::<BTreeSet<_>>();

        let mut summary = SyncSummary {
            profiles: unique_profiles.len(),
            sent_messages: 0,
            received_messages: 0,
        };

        for account in accounts {
            let client = self.client_for_profile(&account.profile_name)?;
            summary.sent_messages += self.sync_sent_account(&client, &account, limit)?;
            summary.received_messages +=
                self.sync_received_account(&client, &account, limit)?;
        }

        print_success_or(self.format, &summary, |summary| {
            println!(
                "synced profiles={} sent={} received={}",
                summary.profiles, summary.sent_messages, summary.received_messages
            );
        });

        Ok(())
    }

    pub fn sync_sent_account(
        &self,
        client: &ResendClient,
        account: &AccountRecord,
        page_size: usize,
    ) -> Result<usize> {
        let cursor = self.get_sync_cursor(&account.email, "sent")?;
        let mut after = None;
        let mut newest_cursor = None;
        let mut total = 0usize;

        loop {
            let page = client.list_sent_emails_page(page_size, after.as_deref())?;
            if newest_cursor.is_none() {
                newest_cursor = page.data.first().map(|item| item.id.clone());
            }
            let mut stop = false;
            let mut last_id = None;

            for item in page.data {
                last_id = Some(item.id.clone());
                if cursor.as_deref() == Some(item.id.as_str()) {
                    stop = true;
                    break;
                }
                let from_email = item
                    .from
                    .as_deref()
                    .map(normalize_email)
                    .unwrap_or_default();
                if from_email == account.email {
                    let detail = client.get_sent_email(&item.id)?;
                    self.store_sent_message(account, detail, None)?;
                    total += 1;
                }
            }

            if stop || !page.has_more.unwrap_or(false) || last_id.is_none() {
                break;
            }
            after = last_id;
        }

        if let Some(cursor_id) = newest_cursor {
            self.set_sync_cursor(&account.email, "sent", &cursor_id)?;
        }

        Ok(total)
    }

    pub fn sync_received_account(
        &self,
        client: &ResendClient,
        account: &AccountRecord,
        page_size: usize,
    ) -> Result<usize> {
        let cursor = self.get_sync_cursor(&account.email, "received")?;
        let mut after = None;
        let mut newest_cursor = None;
        let mut total = 0usize;

        loop {
            let page = client.list_received_emails_page(page_size, after.as_deref())?;
            if newest_cursor.is_none() {
                newest_cursor = page.data.first().map(|item| item.id.clone());
            }
            let mut stop = false;
            let mut last_id = None;

            for item in page.data {
                last_id = Some(item.id.clone());
                if cursor.as_deref() == Some(item.id.as_str()) {
                    stop = true;
                    break;
                }
                let detail = client.get_received_email(&item.id)?;
                if !received_email_matches_account(&detail, &account.email) {
                    continue;
                }
                let message_id = self.store_received_message(account, detail.clone())?;
                self.store_received_attachments(message_id, &detail.attachments)?;
                total += 1;
            }

            if stop || !page.has_more.unwrap_or(false) || last_id.is_none() {
                break;
            }
            after = last_id;
        }

        if let Some(cursor_id) = newest_cursor {
            self.set_sync_cursor(&account.email, "received", &cursor_id)?;
        }

        Ok(total)
    }
}
