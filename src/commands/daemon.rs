use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{msg_send_id, sel, ClassType, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
};
use objc2_foundation::{
    NSData, NSDate, NSRunLoop, NSString,
};

use crate::app::App;
use crate::cli::DaemonArgs;
use crate::helpers::{normalize_email, received_email_matches_account, send_desktop_notification};
use crate::output::Format;

const ICON_PNG: &[u8] = include_bytes!("../../assets/menubar_icon.png");

impl App {
    pub fn daemon(&self, args: DaemonArgs) -> Result<()> {
        let interval = args.interval;
        let account_filter = args.account.clone();
        let db_path = self.db_path.clone();

        let initial_unread = self.count_unread(account_filter.as_deref()).unwrap_or(0);
        let account_label = account_filter
            .as_deref()
            .unwrap_or("All accounts")
            .to_string();

        // Shared state
        let unread_count = Arc::new(AtomicUsize::new(initial_unread));
        let sync_requested = Arc::new(AtomicBool::new(false));
        let mark_read_requested = Arc::new(AtomicBool::new(false));
        let is_syncing = Arc::new(AtomicBool::new(false));

        // Background sync thread
        let unread_bg = unread_count.clone();
        let sync_req_bg = sync_requested.clone();
        let mark_read_bg = mark_read_requested.clone();
        let syncing_bg = is_syncing.clone();
        let account_filter_bg = account_filter.clone();

        thread::spawn(move || {
            let Ok(app) = App::new(db_path, Format::Json) else {
                eprintln!("daemon: failed to open database");
                return;
            };

            loop {
                if mark_read_bg.swap(false, Ordering::Relaxed) {
                    let _ = app.mark_all_read(account_filter_bg.as_deref());
                    let c = app.count_unread(account_filter_bg.as_deref()).unwrap_or(0);
                    unread_bg.store(c, Ordering::Relaxed);
                }

                syncing_bg.store(true, Ordering::Relaxed);
                if let Err(e) = daemon_sync(&app, account_filter_bg.as_deref()) {
                    eprintln!("sync error: {}", e);
                }
                let count = app.count_unread(account_filter_bg.as_deref()).unwrap_or(0);
                unread_bg.store(count, Ordering::Relaxed);
                syncing_bg.store(false, Ordering::Relaxed);

                for _ in 0..(interval * 4) {
                    if sync_req_bg.swap(false, Ordering::Relaxed) {
                        break;
                    }
                    if mark_read_bg.swap(false, Ordering::Relaxed) {
                        let _ = app.mark_all_read(account_filter_bg.as_deref());
                        let c = app.count_unread(account_filter_bg.as_deref()).unwrap_or(0);
                        unread_bg.store(c, Ordering::Relaxed);
                    }
                    thread::sleep(Duration::from_millis(250));
                }
            }
        });

        // Main thread: Cocoa event loop
        // SAFETY: we are on the main thread
        let mtm = unsafe { MainThreadMarker::new_unchecked() };

        let _app = NSApplication::sharedApplication(mtm);

        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(-1.0);

        // Load icon as template image
        let icon = load_icon(ICON_PNG, mtm);
        update_status_display(&status_item, &icon, initial_unread, false, mtm);

        // Build menu
        let menu = NSMenu::new(mtm);

        let status_label = new_menu_item(
            &format!("{} unread \u{00b7} {}", initial_unread, account_label),
            mtm,
        );
        status_label.setEnabled(false);
        menu.addItem(&status_label);

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let sync_item = new_menu_item("Sync Now", mtm);
        menu.addItem(&sync_item);

        let mark_read_item = new_menu_item("Mark All Read", mtm);
        menu.addItem(&mark_read_item);

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let quit_item = new_menu_item("Quit", mtm);
        menu.addItem(&quit_item);

        status_item.setMenu(Some(&menu));

        // Event loop
        let sync_req_ui = sync_requested.clone();
        let mark_read_ui = mark_read_requested.clone();
        let unread_ui = unread_count.clone();
        let syncing_ui = is_syncing.clone();
        let mut last_count = initial_unread;
        let mut last_syncing = false;

        loop {
            // Process events for 200ms
            let date = unsafe { NSDate::dateWithTimeIntervalSinceNow(0.2) };
            let run_loop = NSRunLoop::currentRunLoop();
            let mode = unsafe { NSString::from_str("kCFRunLoopDefaultMode") };
            run_loop.runMode_beforeDate(&mode, &date);

            // Poll menu clicks via tag trick
            poll_click(&sync_item, &sync_req_ui);
            poll_click(&mark_read_item, &mark_read_ui);
            poll_quit(&quit_item);

            // Update display
            let count = unread_ui.load(Ordering::Relaxed);
            let syncing = syncing_ui.load(Ordering::Relaxed);

            if count != last_count || syncing != last_syncing {
                update_status_display(&status_item, &icon, count, syncing, mtm);

                let label = if syncing {
                    format!("Syncing\u{2026} \u{00b7} {}", account_label)
                } else {
                    format!("{} unread \u{00b7} {}", count, account_label)
                };
                status_label.setTitle(&NSString::from_str(&label));

                last_count = count;
                last_syncing = syncing;
            }
        }
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

    fn mark_all_read(&self, account_filter: Option<&str>) -> Result<()> {
        match account_filter {
            Some(acct) => {
                self.conn.execute(
                    "UPDATE messages SET is_read = 1 WHERE is_read = 0 AND direction = 'received' AND account_email = ?1",
                    [acct],
                )?;
            }
            None => {
                self.conn.execute(
                    "UPDATE messages SET is_read = 1 WHERE is_read = 0 AND direction = 'received'",
                    [],
                )?;
            }
        }
        Ok(())
    }
}

// ── AppKit helpers ──────────────────────────────────────────────────────────

fn load_icon(data: &[u8], mtm: MainThreadMarker) -> Retained<NSImage> {
    let ns_data = NSData::with_bytes(data);
    let image = NSImage::initWithData(mtm.alloc(), &ns_data).expect("failed to load icon");
    unsafe { image.setTemplate(true) };
    let size = objc2_foundation::NSSize::new(18.0, 18.0);
    unsafe { image.setSize(size) };
    image
}

fn update_status_display(
    item: &NSStatusItem,
    icon: &NSImage,
    unread: usize,
    syncing: bool,
    mtm: MainThreadMarker,
) {
    if let Some(button) = item.button(mtm) {
        button.setImage(Some(icon));
        let title = if syncing {
            "\u{21BB}".to_string() // ↻
        } else if unread > 99 {
            "99+".to_string()
        } else if unread > 0 {
            unread.to_string()
        } else {
            String::new()
        };
        button.setTitle(&NSString::from_str(&title));
    }
}

fn new_menu_item(title: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let ns_title = NSString::from_str(title);
    let ns_key = NSString::from_str("");
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &ns_title,
            None,
            &ns_key,
        )
    }
}

fn poll_click(item: &NSMenuItem, flag: &AtomicBool) {
    if item.isHighlighted() {
        if item.tag() == 0 {
            item.setTag(1);
        }
    } else if item.tag() == 1 {
        item.setTag(0);
        flag.store(true, Ordering::Relaxed);
    }
}

fn poll_quit(item: &NSMenuItem) {
    if item.isHighlighted() {
        if item.tag() == 0 {
            item.setTag(1);
        }
    } else if item.tag() == 1 {
        std::process::exit(0);
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
