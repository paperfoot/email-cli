use anyhow::Result;

use crate::app::App;
use crate::cli::*;
use crate::output::print_success_or;

impl App {
    pub fn email_list(&self, args: EmailListArgs) -> Result<()> {
        let client = self.default_client()?;
        let page = client.list_sent_emails_page(args.limit, args.after.as_deref())?;
        print_success_or(self.format, &page, |list| {
            for email in &list.data {
                let event = email.last_event.as_deref().unwrap_or("(unknown)");
                let subject = email.subject.as_deref().unwrap_or("(no subject)");
                let to = email.to.join(", ");
                let created = email.created_at.as_deref().unwrap_or("");
                println!("{} {} to={} subject={} created_at={}", email.id, event, to, subject, created);
            }
            if list.data.is_empty() {
                println!("no emails");
            }
        });
        Ok(())
    }
}
