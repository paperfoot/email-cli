use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::app::App;
use crate::cli::*;
use crate::models::*;
use crate::output::print_success_or;

fn parse_properties_arg(raw: Option<String>) -> Result<Option<HashMap<String, serde_json::Value>>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| "--properties must be a JSON object, e.g. '{\"company\":\"Acme\"}'")?;
    let serde_json::Value::Object(map) = parsed else {
        anyhow::bail!("--properties must be a JSON object, got {}", parsed);
    };
    Ok(Some(map.into_iter().collect()))
}

fn split_csv(value: Option<String>) -> Vec<String> {
    value
        .map(|raw| {
            raw.split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// Parse `topic_id:opt_in,topic_id2:opt_out` into TopicRef list. Validates the subscription
/// state, returning an InvalidInput-shaped error message on bad input.
fn parse_topics_arg(value: Option<String>) -> Result<Option<Vec<TopicRef>>> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let mut refs = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (id, sub) = entry.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "--topics entries must be 'topic_id:opt_in' or 'topic_id:opt_out', got '{}'",
                entry
            )
        })?;
        let id = id.trim().to_string();
        let sub = sub.trim().to_string();
        if sub != "opt_in" && sub != "opt_out" {
            anyhow::bail!(
                "--topics subscription must be 'opt_in' or 'opt_out', got '{}'",
                sub
            );
        }
        refs.push(TopicRef { id, subscription: sub });
    }
    if refs.is_empty() {
        Ok(None)
    } else {
        Ok(Some(refs))
    }
}

impl App {
    pub fn contact_list(&self, args: ContactListArgs) -> Result<()> {
        let client = self.default_client()?;
        let page = client.list_contacts_page(args.limit, args.after.as_deref())?;
        print_success_or(self.format, &page, |c| {
            for contact in &c.data {
                let name = match (&contact.first_name, &contact.last_name) {
                    (Some(f), Some(l)) => format!("{} {}", f, l),
                    (Some(f), None) => f.clone(),
                    (None, Some(l)) => l.clone(),
                    (None, None) => String::new(),
                };
                println!("{} {} {}", contact.id, contact.email, name);
            }
            if c.data.is_empty() {
                println!("no contacts");
            }
        });
        Ok(())
    }

    pub fn contact_get(&self, args: ContactGetArgs) -> Result<()> {
        let client = self.default_client()?;
        let contact = client.get_contact(&args.id_or_email)?;
        print_success_or(self.format, &contact, |c| {
            println!("id: {}", c.id);
            println!("email: {}", c.email);
            if let Some(f) = &c.first_name { println!("first_name: {}", f); }
            if let Some(l) = &c.last_name { println!("last_name: {}", l); }
            if let Some(u) = c.unsubscribed { println!("unsubscribed: {}", u); }
            if let Some(props) = &c.properties {
                if !props.is_empty() {
                    println!("properties:");
                    for (k, v) in props {
                        println!("  {} = {}", k, v);
                    }
                }
            }
        });
        Ok(())
    }

    pub fn contact_create(&self, args: ContactCreateArgs) -> Result<()> {
        let properties = parse_properties_arg(args.properties)?;
        let segments = {
            let ids = split_csv(args.segments);
            if ids.is_empty() {
                None
            } else {
                Some(ids.into_iter().map(|id| SegmentRef { id }).collect())
            }
        };
        let topics = parse_topics_arg(args.topics)?;
        let client = self.default_client()?;
        let response = client.create_contact(&CreateContactRequest {
            email: args.email,
            first_name: args.first_name,
            last_name: args.last_name,
            unsubscribed: args.unsubscribed,
            properties,
            segments,
            topics,
        })?;
        print_success_or(self.format, &response, |r| {
            println!("created contact {}", r.id);
        });
        Ok(())
    }

    pub fn contact_update(&self, args: ContactUpdateArgs) -> Result<()> {
        let properties = parse_properties_arg(args.properties)?;
        let client = self.default_client()?;
        let response = client.update_contact(&args.id_or_email, &UpdateContactRequest {
            first_name: args.first_name,
            last_name: args.last_name,
            unsubscribed: args.unsubscribed,
            properties,
        })?;
        print_success_or(self.format, &response, |r| {
            println!("updated contact {}", r.id);
        });
        Ok(())
    }

    pub fn contact_delete(&self, args: ContactDeleteArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.delete_contact(&args.id_or_email)?;
        print_success_or(self.format, &response, |r| {
            println!("deleted: {}", r.deleted);
        });
        Ok(())
    }
}
