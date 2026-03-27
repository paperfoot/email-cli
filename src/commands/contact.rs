use anyhow::Result;

use crate::app::App;
use crate::cli::*;
use crate::models::*;
use crate::output::print_success_or;

impl App {
    pub fn contact_list(&self, args: ContactListArgs) -> Result<()> {
        let client = self.default_client()?;
        let contacts = client.list_contacts(&args.audience)?;
        print_success_or(self.format, &contacts, |c| {
            for contact in &c.data {
                let name = match (&contact.first_name, &contact.last_name) {
                    (Some(f), Some(l)) => format!("{} {}", f, l),
                    (Some(f), None) => f.clone(),
                    (None, Some(l)) => l.clone(),
                    (None, None) => String::new(),
                };
                println!("{} {} {}", contact.id, contact.email, name);
            }
        });
        Ok(())
    }

    pub fn contact_get(&self, args: ContactGetArgs) -> Result<()> {
        let client = self.default_client()?;
        let contact = client.get_contact(&args.audience, &args.id)?;
        print_success_or(self.format, &contact, |c| {
            println!("id: {}", c.id);
            println!("email: {}", c.email);
            if let Some(f) = &c.first_name { println!("first_name: {}", f); }
            if let Some(l) = &c.last_name { println!("last_name: {}", l); }
            if let Some(u) = c.unsubscribed { println!("unsubscribed: {}", u); }
        });
        Ok(())
    }

    pub fn contact_create(&self, args: ContactCreateArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.create_contact(&args.audience, &CreateContactRequest {
            email: args.email,
            first_name: args.first_name,
            last_name: args.last_name,
            unsubscribed: args.unsubscribed,
        })?;
        print_success_or(self.format, &response, |r| {
            println!("created contact {}", r.id);
        });
        Ok(())
    }

    pub fn contact_update(&self, args: ContactUpdateArgs) -> Result<()> {
        let client = self.default_client()?;
        let contact = client.update_contact(&args.audience, &args.id, &UpdateContactRequest {
            first_name: args.first_name,
            last_name: args.last_name,
            unsubscribed: args.unsubscribed,
        })?;
        print_success_or(self.format, &contact, |c| {
            println!("updated contact {} {}", c.id, c.email);
        });
        Ok(())
    }

    pub fn contact_delete(&self, args: ContactDeleteArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.delete_contact(&args.audience, &args.id)?;
        print_success_or(self.format, &response, |r| {
            println!("deleted: {}", r.deleted);
        });
        Ok(())
    }
}
