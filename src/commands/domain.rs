use anyhow::{Context, Result};

use crate::app::App;
use crate::cli::*;
use crate::models::*;
use crate::output::print_success_or;

impl App {
    /// Get the API client for the first (or only) profile.
    /// Domain/contact/segment/topic/broadcast/api-key commands are profile-level.
    pub fn default_client(&self) -> Result<crate::resend::ResendClient> {
        let name: String = self.conn.query_row(
            "SELECT name FROM profiles ORDER BY name LIMIT 1",
            [],
            |row| row.get(0),
        ).context("no profiles configured \u{2014} run: email-cli profile add")?;
        self.client_for_profile(&name)
    }

    pub fn domain_list(&self) -> Result<()> {
        let client = self.default_client()?;
        let domains = client.list_domains()?;
        print_success_or(self.format, &domains, |d| {
            for domain in &d.data {
                let status = domain.status.as_deref().unwrap_or("unknown");
                println!("{} status={}", domain.name, status);
            }
        });
        Ok(())
    }

    pub fn domain_get(&self, args: DomainGetArgs) -> Result<()> {
        let client = self.default_client()?;
        let domain = client.get_domain(&args.id)?;
        print_success_or(self.format, &domain, |d| {
            println!("id: {}", d.id);
            println!("name: {}", d.name);
            println!("status: {}", d.status.as_deref().unwrap_or("unknown"));
            if !d.records.is_empty() {
                println!("records:");
                for record in &d.records {
                    let rtype = record.record_type.as_deref().unwrap_or("?");
                    let name = record.name.as_deref().unwrap_or("?");
                    let value = record.value.as_deref().unwrap_or("?");
                    let status = record.status.as_deref().unwrap_or("?");
                    println!("  {} {} {} ({})", rtype, name, value, status);
                }
            }
        });
        Ok(())
    }

    pub fn domain_create(&self, args: DomainCreateArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.create_domain(&CreateDomainRequest {
            name: args.name,
            region: args.region,
        })?;
        print_success_or(self.format, &response, |r| {
            println!("created domain {} (id: {})", r.name, r.id);
        });
        Ok(())
    }

    pub fn domain_verify(&self, args: DomainVerifyArgs) -> Result<()> {
        let client = self.default_client()?;
        let domain = client.verify_domain(&args.id)?;
        print_success_or(self.format, &domain, |d| {
            println!("verified {} status={}", d.name, d.status.as_deref().unwrap_or("unknown"));
        });
        Ok(())
    }

    pub fn domain_delete(&self, args: DomainDeleteArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.delete_domain(&args.id)?;
        print_success_or(self.format, &response, |r| {
            println!("deleted: {}", r.deleted);
        });
        Ok(())
    }

    pub fn domain_update(&self, args: DomainUpdateArgs) -> Result<()> {
        let client = self.default_client()?;
        let domain = client.update_domain(&args.id, &UpdateDomainRequest {
            open_tracking: args.open_tracking,
            click_tracking: args.click_tracking,
        })?;
        print_success_or(self.format, &domain, |d| {
            println!("updated {} status={}", d.name, d.status.as_deref().unwrap_or("unknown"));
        });
        Ok(())
    }
}
