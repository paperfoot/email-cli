use anyhow::Result;

use crate::app::App;
use crate::cli::*;
use crate::models::*;
use crate::output::print_success_or;

impl App {
    pub fn api_key_list(&self) -> Result<()> {
        let client = self.default_client()?;
        let keys = client.list_api_keys()?;
        print_success_or(self.format, &keys, |k| {
            for key in &k.data {
                println!("{} {}", key.id, key.name);
            }
        });
        Ok(())
    }

    pub fn api_key_create(&self, args: ApiKeyCreateArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.create_api_key(&CreateApiKeyRequest {
            name: args.name,
            permission: Some(args.permission),
        })?;
        print_success_or(self.format, &response, |r| {
            println!("id: {}", r.id);
            println!("token: {}", r.token);
        });
        Ok(())
    }

    pub fn api_key_delete(&self, args: ApiKeyDeleteArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.delete_api_key(&args.id)?;
        print_success_or(self.format, &response, |r| {
            println!("deleted: {}", r.deleted);
        });
        Ok(())
    }
}
