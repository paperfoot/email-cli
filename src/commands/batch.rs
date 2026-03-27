use anyhow::{Context, Result};
use std::fs;

use crate::app::App;
use crate::cli::*;
use crate::output::print_success_or;

impl App {
    pub fn batch_send(&self, args: BatchSendArgs) -> Result<()> {
        let client = self.default_client()?;
        let content = fs::read_to_string(&args.file)
            .with_context(|| format!("failed to read {}", args.file.display()))?;
        let emails: Vec<serde_json::Value> = serde_json::from_str(&content)
            .with_context(|| format!("invalid JSON in {}", args.file.display()))?;
        let response = client.send_batch(&emails)?;
        print_success_or(self.format, &response, |r| {
            for item in &r.data {
                println!("{}", item.id);
            }
            println!("sent {} emails", r.data.len());
        });
        Ok(())
    }
}
