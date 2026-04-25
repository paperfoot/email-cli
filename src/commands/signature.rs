use anyhow::Result;
use rusqlite::params;
use serde_json::json;

use crate::app::App;
use crate::cli::{SignatureSetArgs, SignatureShowArgs};
use crate::helpers::normalize_email;
use crate::output::print_success_or;

impl App {
    pub fn signature_set(&self, args: SignatureSetArgs) -> Result<()> {
        let account = normalize_email(&args.account);
        let signature = args.html.or(args.text).unwrap_or_default();
        self.conn.execute(
            "UPDATE accounts SET signature = ?1, updated_at = CURRENT_TIMESTAMP WHERE email = ?2",
            params![signature, account],
        )?;
        let updated = self.get_account(&account)?;

        print_success_or(self.format, &updated, |_updated| {
            println!("updated signature for {}", account);
        });

        Ok(())
    }

    pub fn signature_show(&self, args: SignatureShowArgs) -> Result<()> {
        let account = self.get_account(&normalize_email(&args.account))?;

        let data = json!({
            "account": account.email,
            "signature": account.signature,
        });
        print_success_or(self.format, &data, |_d| {
            println!("{}", account.signature);
        });

        Ok(())
    }
}
