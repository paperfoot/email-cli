use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tray_item::{IconSource, TrayItem};

use crate::app::App;
use crate::cli::DaemonArgs;
use crate::helpers::{normalize_email, received_email_matches_account, send_desktop_notification};

impl App {
    pub fn daemon(&self, args: DaemonArgs) -> Result<()> {
        let interval = args.interval;
        let account_filter = args.account.clone();

        // Menu bar icon: envelope character as the title
        let mut tray = TrayItem::new("\u{2709}\u{FE0E}", IconSource::Resource(""))
            .map_err(|e| anyhow::anyhow!("failed to create menu bar icon: {}", e))?;

        let (tx, rx) = mpsc::channel::<DaemonMsg>();
        let running = Arc::new(AtomicBool::new(true));

        // Status label
        let unread = self.count_unread(account_filter.as_deref()).unwrap_or(0);
        tray.add_label(&format!("{} unread", unread))
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Sync Now
        let tx_sync = tx.clone();
        tray.add_menu_item("Sync Now", move || {
            let _ = tx_sync.send(DaemonMsg::Sync);
        })
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Quit
        tray.add_menu_item("Quit Email CLI", move || {
            std::process::exit(0);
        })
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Periodic sync thread
        let running_bg = running.clone();
        let tx_timer = tx.clone();
        thread::spawn(move || {
            while running_bg.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(interval));
                if running_bg.load(Ordering::Relaxed) {
                    let _ = tx_timer.send(DaemonMsg::Sync);
                }
            }
        });

        // Do an initial sync
        let _ = tx.send(DaemonMsg::Sync);

        // Main event loop — blocks, processes tray messages
        // tray-item on macOS handles the Cocoa run loop internally,
        // but we drive our sync from the mpsc channel
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(DaemonMsg::Sync) => {
                    if let Err(e) = self.daemon_sync(account_filter.as_deref()) {
                        eprintln!("sync error: {}", e);
                    }
                }
                Ok(DaemonMsg::Quit) => {
                    running.store(false, Ordering::Relaxed);
                    std::process::exit(0);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Keep the run loop alive
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        Ok(())
    }

    fn daemon_sync(&self, account_filter: Option<&str>) -> Result<()> {
        let accounts = if let Some(account) = account_filter {
            vec![self.get_account(&normalize_email(account))?]
        } else {
            self.list_accounts()?
        };

        for account in accounts {
            let client = self.client_for_profile(&account.profile_name)?;
            let _ = self.sync_sent_account(&client, &account, 25);

            let cursor = self.get_sync_cursor(&account.email, "received")?;
            let mut after = None;
            let mut newest_cursor = None;

            loop {
                let page = client.list_received_emails_page(25, after.as_deref())?;
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
                    let message_id = self.store_received_message(&account, detail.clone())?;
                    self.store_received_attachments(message_id, &detail.attachments)?;

                    send_desktop_notification(
                        &format!("New email to {}", account.email),
                        &format!("From: {}\n{}", from, subject),
                    );
                }

                if stop || !page.has_more.unwrap_or(false) || last_id.is_none() {
                    break;
                }
                after = last_id;
            }

            if let Some(cursor_id) = newest_cursor {
                self.set_sync_cursor(&account.email, "received", &cursor_id)?;
            }
        }

        Ok(())
    }

    fn count_unread(&self, account_filter: Option<&str>) -> Result<usize> {
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match account_filter {
            Some(acct) => (
                "SELECT COUNT(*) FROM messages WHERE is_read = 0 AND direction = 'received' AND account_email = ?1",
                vec![Box::new(acct.to_string())],
            ),
            None => (
                "SELECT COUNT(*) FROM messages WHERE is_read = 0 AND direction = 'received'",
                vec![],
            ),
        };
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let count: i64 = self.conn.query_row(sql, refs.as_slice(), |row| row.get(0))?;
        Ok(count as usize)
    }
}

enum DaemonMsg {
    Sync,
    Quit,
}
