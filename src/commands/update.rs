use anyhow::Result;
use serde::Serialize;

use crate::app::App;
use crate::output::print_success_or;

#[derive(Serialize)]
struct UpdateResult {
    current_version: String,
    latest_version: String,
    status: String,
}

impl App {
    pub fn update(&self, check: bool) -> Result<()> {
        let current = env!("CARGO_PKG_VERSION");

        let updater = self_update::backends::github::Update::configure()
            .repo_owner("paperfoot")
            .repo_name("email-cli")
            .bin_name("email-cli")
            .current_version(current)
            .build()?;

        if check {
            let latest = updater.get_latest_release()?;
            let v = latest.version.trim_start_matches('v').to_string();
            let up_to_date = v == current;
            let result = UpdateResult {
                current_version: current.into(),
                latest_version: v.clone(),
                status: if up_to_date {
                    "up_to_date".into()
                } else {
                    "update_available".into()
                },
            };
            print_success_or(self.format, &result, |r| {
                if up_to_date {
                    println!("Up to date (v{})", r.current_version);
                } else {
                    println!(
                        "Update available: v{} -> v{}",
                        r.current_version, r.latest_version
                    );
                    println!("Run `email-cli update` to install");
                }
            });
        } else {
            match updater.update() {
                Ok(release) => {
                    let v = release.version().trim_start_matches('v').to_string();
                    let up_to_date = v == current;
                    let result = UpdateResult {
                        current_version: current.into(),
                        latest_version: v.clone(),
                        status: if up_to_date {
                            "up_to_date".into()
                        } else {
                            "updated".into()
                        },
                    };
                    print_success_or(self.format, &result, |r| {
                        if up_to_date {
                            println!("Already up to date (v{})", r.current_version);
                        } else {
                            println!("Updated: v{} -> v{}", r.current_version, r.latest_version);
                            println!("Run `email-cli skill install` to update agent skills");
                        }
                    });
                }
                Err(e) => {
                    anyhow::bail!("update failed: {}", e);
                }
            }
        }

        Ok(())
    }
}
