use anyhow::{Context, Result};

use crate::app::App;
use crate::cli::*;
use crate::models::*;
use crate::output::print_success_or;

fn parse_fallback(raw: Option<String>, property_type: &str) -> Result<Option<serde_json::Value>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    match property_type {
        "number" => {
            let number: f64 = raw
                .parse()
                .with_context(|| format!("--fallback must be a number, got '{}'", raw))?;
            let value = serde_json::Number::from_f64(number)
                .map(serde_json::Value::Number)
                .ok_or_else(|| anyhow::anyhow!("invalid number {}", number))?;
            Ok(Some(value))
        }
        "string" => Ok(Some(serde_json::Value::String(raw))),
        other => anyhow::bail!("unknown --property-type '{}', expected string|number", other),
    }
}

impl App {
    pub fn contact_property_list(&self) -> Result<()> {
        let client = self.default_client()?;
        let list = client.list_contact_properties()?;
        print_success_or(self.format, &list, |list| {
            for prop in &list.data {
                let fallback = prop
                    .fallback_value
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "(none)".to_string());
                println!("{} key={} type={} fallback={}", prop.id, prop.key, prop.property_type, fallback);
            }
            if list.data.is_empty() {
                println!("no contact properties");
            }
        });
        Ok(())
    }

    pub fn contact_property_get(&self, args: ContactPropertyGetArgs) -> Result<()> {
        let client = self.default_client()?;
        let prop = client.get_contact_property(&args.id)?;
        print_success_or(self.format, &prop, |p| {
            println!("id: {}", p.id);
            println!("key: {}", p.key);
            println!("type: {}", p.property_type);
            if let Some(f) = &p.fallback_value {
                println!("fallback_value: {}", f);
            }
        });
        Ok(())
    }

    pub fn contact_property_create(&self, args: ContactPropertyCreateArgs) -> Result<()> {
        let fallback = parse_fallback(args.fallback, &args.property_type)?;
        let client = self.default_client()?;
        let response = client.create_contact_property(&CreateContactPropertyRequest {
            key: args.key,
            property_type: args.property_type,
            fallback_value: fallback,
        })?;
        print_success_or(self.format, &response, |r| {
            println!("created contact-property {}", r.id);
        });
        Ok(())
    }

    pub fn contact_property_update(&self, args: ContactPropertyUpdateArgs) -> Result<()> {
        let property_type = if args.as_number { "number" } else { "string" };
        let fallback = parse_fallback(args.fallback, property_type)?;
        let client = self.default_client()?;
        let response = client.update_contact_property(
            &args.id,
            &UpdateContactPropertyRequest { fallback_value: fallback },
        )?;
        print_success_or(self.format, &response, |r| {
            println!("updated contact-property {}", r.id);
        });
        Ok(())
    }

    pub fn contact_property_delete(&self, args: ContactPropertyDeleteArgs) -> Result<()> {
        let client = self.default_client()?;
        let response = client.delete_contact_property(&args.id)?;
        print_success_or(self.format, &response, |r| {
            println!("deleted: {}", r.deleted);
        });
        Ok(())
    }
}
