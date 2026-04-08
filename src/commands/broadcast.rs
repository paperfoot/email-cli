use anyhow::Result;

use crate::app::App;
use crate::cli::*;
use crate::models::*;
use crate::output::print_success_or;

fn split_csv(value: Option<String>) -> Option<Vec<String>> {
    value.map(|raw| {
        raw.split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    })
}

impl App {
    pub fn broadcast_list(&self) -> Result<()> {
        let client = self.default_client()?;
        let broadcasts = client.list_broadcasts()?;
        print_success_or(self.format, &broadcasts, |b| {
            for broadcast in &b.data {
                let status = broadcast.status.as_deref().unwrap_or("(unknown)");
                let subject = broadcast.subject.as_deref().unwrap_or("");
                let name = broadcast.name.as_deref().unwrap_or("");
                println!("{} {} {} {}", broadcast.id, status, name, subject);
            }
            if b.data.is_empty() {
                println!("no broadcasts");
            }
        });
        Ok(())
    }

    pub fn broadcast_get(&self, args: BroadcastGetArgs) -> Result<()> {
        let client = self.default_client()?;
        let broadcast = client.get_broadcast(&args.id)?;
        print_success_or(self.format, &broadcast, |b| {
            println!("id: {}", b.id);
            if let Some(name) = &b.name { println!("name: {}", name); }
            if let Some(subject) = &b.subject { println!("subject: {}", subject); }
            if let Some(from) = &b.from { println!("from: {}", from); }
            if !b.reply_to.is_empty() { println!("reply_to: {}", b.reply_to.join(", ")); }
            if let Some(seg) = &b.segment_id { println!("segment_id: {}", seg); }
            if let Some(topic) = &b.topic_id { println!("topic_id: {}", topic); }
            if let Some(preview) = &b.preview_text { println!("preview_text: {}", preview); }
            if let Some(status) = &b.status { println!("status: {}", status); }
            if let Some(scheduled) = &b.scheduled_at { println!("scheduled_at: {}", scheduled); }
            if let Some(sent) = &b.sent_at { println!("sent_at: {}", sent); }
        });
        Ok(())
    }

    pub fn broadcast_create(&self, args: BroadcastCreateArgs) -> Result<()> {
        let client = self.default_client()?;
        let request = CreateBroadcastRequest {
            segment_id: args.segment_id,
            from: args.from,
            subject: args.subject,
            html: args.html,
            text: args.text,
            name: args.name,
            reply_to: split_csv(args.reply_to),
            topic_id: args.topic_id,
            scheduled_at: args.scheduled_at,
            send: if args.send { Some(true) } else { None },
        };
        let response = client.create_broadcast(&request)?;
        print_success_or(self.format, &response, |r| {
            println!("created broadcast {}", r.id);
        });
        Ok(())
    }

    pub fn broadcast_update(&self, args: BroadcastUpdateArgs) -> Result<()> {
        let client = self.default_client()?;
        let payload = UpdateBroadcastRequest {
            segment_id: args.segment_id,
            from: args.from,
            subject: args.subject,
            html: args.html,
            text: args.text,
            name: args.name,
            reply_to: split_csv(args.reply_to),
            topic_id: args.topic_id,
        };
        let response = client.update_broadcast(&args.id, &payload)?;
        print_success_or(self.format, &response, |r| {
            println!("updated broadcast {}", r.id);
        });
        Ok(())
    }

    pub fn broadcast_send(&self, args: BroadcastSendArgs) -> Result<()> {
        let client = self.default_client()?;
        let payload = SendBroadcastRequest { scheduled_at: args.scheduled_at };
        let response = client.send_broadcast(&args.id, &payload)?;
        print_success_or(self.format, &response, |r| {
            println!("sent broadcast {}", r.id);
        });
        Ok(())
    }

    pub fn broadcast_delete(&self, args: BroadcastDeleteArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.delete_broadcast(&args.id)?;
        print_success_or(self.format, &response, |r| {
            println!("deleted: {}", r.deleted);
        });
        Ok(())
    }
}
