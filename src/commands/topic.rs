use anyhow::Result;

use crate::app::App;
use crate::cli::*;
use crate::models::*;
use crate::output::print_success_or;

impl App {
    pub fn topic_list(&self) -> Result<()> {
        let client = self.default_client()?;
        let list = client.list_topics()?;
        print_success_or(self.format, &list, |list| {
            for topic in &list.data {
                let default = topic.default_subscription.as_deref().unwrap_or("(unset)");
                println!("{} {} default={}", topic.id, topic.name, default);
            }
            if list.data.is_empty() {
                println!("no topics");
            }
        });
        Ok(())
    }

    pub fn topic_get(&self, args: TopicGetArgs) -> Result<()> {
        let client = self.default_client()?;
        let topic = client.get_topic(&args.id)?;
        print_success_or(self.format, &topic, |t| {
            println!("id: {}", t.id);
            println!("name: {}", t.name);
            if let Some(d) = &t.description { println!("description: {}", d); }
            if let Some(d) = &t.default_subscription { println!("default_subscription: {}", d); }
            if let Some(v) = &t.visibility { println!("visibility: {}", v); }
        });
        Ok(())
    }

    pub fn topic_create(&self, args: TopicCreateArgs) -> Result<()> {
        if let Some(ref sub) = args.default_subscription {
            if sub != "opt_in" && sub != "opt_out" {
                anyhow::bail!(
                    "--default-subscription must be 'opt_in' or 'opt_out', got '{}'",
                    sub
                );
            }
        }
        if let Some(ref vis) = args.visibility {
            if vis != "public" && vis != "private" {
                anyhow::bail!(
                    "--visibility must be 'public' or 'private', got '{}'",
                    vis
                );
            }
        }
        let client = self.default_client()?;
        let response = client.create_topic(&CreateTopicRequest {
            name: args.name,
            description: args.description,
            default_subscription: args.default_subscription,
            visibility: args.visibility,
        })?;
        print_success_or(self.format, &response, |r| {
            println!("created topic {}", r.id);
        });
        Ok(())
    }

    pub fn topic_update(&self, args: TopicUpdateArgs) -> Result<()> {
        if let Some(ref sub) = args.default_subscription {
            if sub != "opt_in" && sub != "opt_out" {
                anyhow::bail!(
                    "--default-subscription must be 'opt_in' or 'opt_out', got '{}'",
                    sub
                );
            }
        }
        if let Some(ref vis) = args.visibility {
            if vis != "public" && vis != "private" {
                anyhow::bail!(
                    "--visibility must be 'public' or 'private', got '{}'",
                    vis
                );
            }
        }
        let client = self.default_client()?;
        let response = client.update_topic(&args.id, &UpdateTopicRequest {
            name: args.name,
            description: args.description,
            default_subscription: args.default_subscription,
            visibility: args.visibility,
        })?;
        print_success_or(self.format, &response, |r| {
            println!("updated topic {}", r.id);
        });
        Ok(())
    }

    pub fn topic_delete(&self, args: TopicDeleteArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.delete_topic(&args.id)?;
        print_success_or(self.format, &response, |r| {
            println!("deleted: {}", r.deleted);
        });
        Ok(())
    }

    pub fn topic_contact_set(&self, args: TopicContactSetArgs) -> Result<()> {
        if args.subscription != "opt_in" && args.subscription != "opt_out" {
            anyhow::bail!("--subscription must be 'opt_in' or 'opt_out', got '{}'", args.subscription);
        }
        let client = self.default_client()?;
        let payload = UpdateContactTopicsRequest {
            topics: vec![ContactTopicSubscription {
                id: args.topic.clone(),
                subscription: args.subscription.clone(),
            }],
        };
        let response = client.update_contact_topics(&args.contact, &payload)?;
        print_success_or(self.format, &response, |_r| {
            println!("set contact={} topic={} subscription={}", args.contact, args.topic, args.subscription);
        });
        Ok(())
    }

    pub fn topic_contact_list(&self, args: TopicContactListArgs) -> Result<()> {
        let client = self.default_client()?;
        let list = client.list_contact_topics(&args.contact)?;
        print_success_or(self.format, &list, |list| {
            for topic in &list.data {
                let name = topic.name.as_deref().unwrap_or("");
                let sub = topic.subscription.as_deref().unwrap_or("(unknown)");
                println!("{} {} subscription={}", topic.id, name, sub);
            }
            if list.data.is_empty() {
                println!("no topic subscriptions");
            }
        });
        Ok(())
    }
}
