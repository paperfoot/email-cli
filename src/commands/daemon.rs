use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tray_item::{IconSource, TrayItem};

use crate::app::App;
use crate::cli::DaemonArgs;
use crate::helpers::{normalize_email, received_email_matches_account, send_desktop_notification};
use crate::output::Format;

// Embed the menu bar icon at compile time
const ICON_PNG: &[u8] = include_bytes!("../../assets/menubar_icon.png");

impl App {
    pub fn daemon(&self, args: DaemonArgs) -> Result<()> {
        let interval = args.interval;
        let account_filter = args.account.clone();
        let db_path = self.db_path.clone();

        // Build the tray icon with embedded PNG
        let mut tray = TrayItem::new("", IconSource::Data {
            width: 32,
            height: 32,
            data: ICON_PNG.to_vec(),
        }).map_err(|e| anyhow::anyhow!("failed to create menu bar icon: {}", e))?;

        // Unread count label
        let unread = self.count_unread(account_filter.as_deref()).unwrap_or(0);
        tray.add_label(&format!("{} unread", unread))
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Sync Now — triggers via a shared flag
        let sync_flag = Arc::new(AtomicBool::new(false));
        let sync_flag_btn = sync_flag.clone();
        tray.add_menu_item("Sync Now", move || {
            sync_flag_btn.store(true, Ordering::Relaxed);
        })
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Quit
        tray.add_menu_item("Quit Email CLI", || {
            std::process::exit(0);
        })
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Spawn background sync thread with its own DB connection
        let sync_flag_bg = sync_flag.clone();
        thread::spawn(move || {
            let Ok(app) = App::new(db_path, Format::Json) else {
                eprintln!("daemon: failed to open database");
                return;
            };

            loop {
                // Sync
                if let Err(e) = daemon_sync(&app, account_filter.as_deref()) {
                    eprintln!("sync error: {}", e);
                }

                // Wait for interval, checking for manual sync trigger
                for _ in 0..(interval * 4) {
                    if sync_flag_bg.swap(false, Ordering::Relaxed) {
                        break; // "Sync Now" was clicked
                    }
                    thread::sleep(Duration::from_millis(250));
                }
            }
        });

        // display() starts the Cocoa event loop — blocks forever.
        // This is what actually puts the icon in the menu bar.
        tray.inner_mut().display();

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

fn daemon_sync(app: &App, account_filter: Option<&str>) -> Result<()> {
    let accounts = if let Some(account) = account_filter {
        vec![app.get_account(&normalize_email(account))?]
    } else {
        app.list_accounts()?
    };

    for account in accounts {
        let client = app.client_for_profile(&account.profile_name)?;
        let _ = app.sync_sent_account(&client, &account, 25);

        let cursor = app.get_sync_cursor(&account.email, "received")?;
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
                let message_id = app.store_received_message(&account, detail.clone())?;
                app.store_received_attachments(message_id, &detail.attachments)?;

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
            app.set_sync_cursor(&account.email, "received", &cursor_id)?;
        }
    }

    Ok(())
}
