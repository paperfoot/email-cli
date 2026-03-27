use anyhow::Result;

use crate::app::App;
use crate::cli::*;
use crate::models::*;
use crate::output::print_success_or;

impl App {
    pub fn audience_list(&self) -> Result<()> {
        let client = self.default_client()?;
        let audiences = client.list_audiences()?;
        print_success_or(self.format, &audiences, |a| {
            for audience in &a.data {
                println!("{} {}", audience.id, audience.name);
            }
        });
        Ok(())
    }

    pub fn audience_get(&self, args: AudienceGetArgs) -> Result<()> {
        let client = self.default_client()?;
        let audience = client.get_audience(&args.id)?;
        print_success_or(self.format, &audience, |a| {
            println!("id: {}", a.id);
            println!("name: {}", a.name);
            if let Some(created) = &a.created_at {
                println!("created: {}", created);
            }
        });
        Ok(())
    }

    pub fn audience_create(&self, args: AudienceCreateArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.create_audience(&CreateAudienceRequest { name: args.name })?;
        print_success_or(self.format, &response, |r| {
            println!("created audience {} (id: {})", r.name, r.id);
        });
        Ok(())
    }

    pub fn audience_delete(&self, args: AudienceDeleteArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.delete_audience(&args.id)?;
        print_success_or(self.format, &response, |r| {
            println!("deleted: {}", r.deleted);
        });
        Ok(())
    }
}
