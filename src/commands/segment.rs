use anyhow::Result;

use crate::app::App;
use crate::cli::*;
use crate::models::*;
use crate::output::print_success_or;

impl App {
    pub fn segment_list(&self) -> Result<()> {
        let client = self.default_client()?;
        let list = client.list_segments()?;
        print_success_or(self.format, &list, |list| {
            for segment in &list.data {
                let created = segment.created_at.as_deref().unwrap_or("");
                println!("{} {} created_at={}", segment.id, segment.name, created);
            }
            if list.data.is_empty() {
                println!("no segments");
            }
        });
        Ok(())
    }

    pub fn segment_get(&self, args: SegmentGetArgs) -> Result<()> {
        let client = self.default_client()?;
        let segment = client.get_segment(&args.id)?;
        print_success_or(self.format, &segment, |s| {
            println!("id: {}", s.id);
            println!("name: {}", s.name);
            if let Some(c) = &s.created_at { println!("created_at: {}", c); }
        });
        Ok(())
    }

    pub fn segment_create(&self, args: SegmentCreateArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.create_segment(&CreateSegmentRequest { name: args.name })?;
        print_success_or(self.format, &response, |r| {
            println!("created segment {} (id: {})", r.name, r.id);
        });
        Ok(())
    }

    pub fn segment_delete(&self, args: SegmentDeleteArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.delete_segment(&args.id)?;
        print_success_or(self.format, &response, |r| {
            println!("deleted: {}", r.deleted);
        });
        Ok(())
    }

    pub fn segment_contact_add(&self, args: SegmentContactArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.add_contact_to_segment(&args.contact, &args.segment)?;
        print_success_or(self.format, &response, |_r| {
            println!("added contact={} to segment={}", args.contact, args.segment);
        });
        Ok(())
    }

    pub fn segment_contact_remove(&self, args: SegmentContactArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.remove_contact_from_segment(&args.contact, &args.segment)?;
        print_success_or(self.format, &response, |_r| {
            println!("removed contact={} from segment={}", args.contact, args.segment);
        });
        Ok(())
    }

    pub fn segment_contact_list(&self, args: SegmentContactListArgs) -> Result<()> {
        let client = self.default_client()?;
        let list = client.list_contact_segments(&args.contact)?;
        print_success_or(self.format, &list, |list| {
            for segment in &list.data {
                println!("{} {}", segment.id, segment.name);
            }
            if list.data.is_empty() {
                println!("no segments for contact");
            }
        });
        Ok(())
    }

    pub fn segment_contacts(&self, args: SegmentContactsArgs) -> Result<()> {
        let client = self.default_client()?;
        let list = client.list_segment_contacts(&args.id)?;
        print_success_or(self.format, &list, |list| {
            for contact in &list.data {
                let name = match (&contact.first_name, &contact.last_name) {
                    (Some(f), Some(l)) => format!("{} {}", f, l),
                    (Some(f), None) => f.clone(),
                    (None, Some(l)) => l.clone(),
                    (None, None) => String::new(),
                };
                println!("{} {} {}", contact.id, contact.email, name);
            }
            if list.data.is_empty() {
                println!("no contacts in segment");
            }
        });
        Ok(())
    }
}
