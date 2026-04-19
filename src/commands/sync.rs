use anyhow::{Result, bail};
use std::collections::BTreeSet;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::app::App;
use crate::cli::SyncArgs;
use crate::helpers::{normalize_email, received_email_matches_account, send_desktop_notification};
use crate::models::{AccountRecord, SyncSummary};
use crate::output::{Format, print_success_or};
use crate::resend::ResendClient;

/// Coarse progress reporter, read by the daemon's status label.
/// `(done, total)` across the most recent sync invocation.
static SYNC_PROGRESS: OnceLock<(AtomicUsize, AtomicUsize)> = OnceLock::new();

fn progress() -> &'static (AtomicUsize, AtomicUsize) {
    SYNC_PROGRESS.get_or_init(|| (AtomicUsize::new(0), AtomicUsize::new(0)))
}

/// Read the current `(done, total)` progress of the most recent sync.
pub fn sync_progress() -> (usize, usize) {
    let p = progress();
    (p.0.load(Ordering::Relaxed), p.1.load(Ordering::Relaxed))
}

fn set_progress(done: usize, total: usize) {
    let p = progress();
    p.0.store(done, Ordering::Relaxed);
    p.1.store(total, Ordering::Relaxed);
}

fn inc_done() {
    progress().0.fetch_add(1, Ordering::Relaxed);
}

type AccountSyncOutcome = (AccountRecord, Result<(usize, usize, Vec<(String, String)>)>);

impl App {
    pub fn sync(&self, args: SyncArgs) -> Result<()> {
        let account_filter = args.account.clone();
        let limit = args.limit;
        let watch = args.watch;
        let notify = args.notify;
        let interval = args.interval.unwrap_or(60);

        self.sync_once(account_filter.as_deref(), limit, notify)?;

        if watch {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(interval));
                if matches!(self.format, Format::Human) {
                    eprintln!("polling...");
                }
                self.sync_once(account_filter.as_deref(), limit, notify)?;
            }
        }

        Ok(())
    }

    fn sync_once(&self, account_filter: Option<&str>, limit: usize, notify: bool) -> Result<()> {
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

        set_progress(0, accounts.len());

        // Fan out: one thread per account. Each thread opens its own SQLite
        // connection (rusqlite::Connection is !Sync). WAL mode handles the
        // concurrent writes via its internal write lock.
        let db_path = self.db_path.clone();
        let handles: Vec<_> = accounts
            .into_iter()
            .map(|account| {
                let db_path = db_path.clone();
                std::thread::spawn(move || -> AccountSyncOutcome {
                    let result = (|| -> Result<_> {
                        let app = App::new(db_path, Format::Json)?;
                        let client = app.client_for_profile(&account.profile_name)?;
                        let sent = app.sync_sent_account(&client, &account, limit)?;
                        let (received, new_messages) =
                            app.sync_received_account_with_details(&client, &account, limit)?;
                        Ok((sent, received, new_messages))
                    })();
                    inc_done();
                    (account, result)
                })
            })
            .collect();

        let mut summary = SyncSummary {
            profiles: unique_profiles.len(),
            sent_messages: 0,
            received_messages: 0,
        };
        let mut errors: Vec<(String, String)> = Vec::new();

        for handle in handles {
            match handle.join() {
                Ok((account, Ok((sent, received, new_messages)))) => {
                    summary.sent_messages += sent;
                    summary.received_messages += received;
                    if notify {
                        for (from, subject) in &new_messages {
                            send_desktop_notification(
                                &format!("New email to {}", account.email),
                                &format!("From: {}\n{}", from, subject),
                            );
                        }
                    }
                }
                Ok((account, Err(e))) => {
                    errors.push((account.email, e.to_string()));
                }
                Err(_) => {
                    errors.push(("?".to_string(), "sync thread panicked".to_string()));
                }
            }
        }

        // Reset progress so the daemon's status label stops showing N/M.
        set_progress(0, 0);

        if matches!(self.format, Format::Human) {
            for (email, err) in &errors {
                eprintln!("sync error ({}): {}", email, err);
            }
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
        // On cold start (no cursor yet) we only pull one page, then set the
        // cursor. Older history stays on Resend and can be backfilled by the
        // user explicitly if they want it. Prevents a cold start from walking
        // thousands of messages.
        let is_cold_start = cursor.is_none();
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
                    self.store_sent_message(account, detail, None, None)?;
                    total += 1;
                }
            }

            if is_cold_start
                || stop
                || !page.has_more.unwrap_or(false)
                || last_id.is_none()
            {
                break;
            }
            after = last_id;
        }

        if let Some(cursor_id) = newest_cursor {
            self.set_sync_cursor(&account.email, "sent", &cursor_id)?;
        }

        Ok(total)
    }

    /// Sync received messages and return (count, Vec<(from, subject)>) for notifications.
    pub fn sync_received_account_with_details(
        &self,
        client: &ResendClient,
        account: &AccountRecord,
        page_size: usize,
    ) -> Result<(usize, Vec<(String, String)>)> {
        let cursor = self.get_sync_cursor(&account.email, "received")?;
        let is_cold_start = cursor.is_none();
        let mut after = None;
        let mut newest_cursor = None;
        let mut total = 0usize;
        let mut new_messages = Vec::new();

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
                let from = detail.from.clone().unwrap_or_default();
                let subject = detail.subject.clone().unwrap_or_default();
                let message_id = self.store_received_message(account, detail.clone())?;
                self.store_received_attachments(message_id, &detail.attachments)?;
                new_messages.push((from, subject));
                total += 1;
            }

            if is_cold_start
                || stop
                || !page.has_more.unwrap_or(false)
                || last_id.is_none()
            {
                break;
            }
            after = last_id;
        }

        if let Some(cursor_id) = newest_cursor {
            self.set_sync_cursor(&account.email, "received", &cursor_id)?;
        }

        Ok((total, new_messages))
    }
}
