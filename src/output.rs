use serde::Serialize;
use std::io::IsTerminal;

use crate::error::CliError;

#[derive(Clone, Copy)]
pub enum Format {
    Json,
    Human,
}

impl Format {
    pub fn detect(json_flag: bool) -> Self {
        if json_flag || !std::io::stdout().is_terminal() {
            Format::Json
        } else {
            Format::Human
        }
    }

    #[allow(dead_code)]
    pub fn is_json(self) -> bool {
        matches!(self, Format::Json)
    }
}

pub fn print_success<T: Serialize>(format: Format, data: &T) {
    if let Format::Json = format {
        let envelope = serde_json::json!({
            "version": "1",
            "status": "success",
            "data": data,
        });
        let output = serde_json::to_string_pretty(&envelope)
            .unwrap_or_else(|_| r#"{"version":"1","status":"error"}"#.to_string());
        println!("{}", output);
    }
}

pub fn print_success_or<T: Serialize, F: FnOnce(&T)>(format: Format, data: &T, human: F) {
    match format {
        Format::Json => print_success(format, data),
        Format::Human => human(data),
    }
}

pub fn print_error(format: Format, err: &CliError) {
    let envelope = serde_json::json!({
        "version": "1",
        "status": "error",
        "error": {
            "code": err.error_code(),
            "message": err.to_string(),
            "suggestion": err.suggestion(),
        },
    });
    match format {
        Format::Json => {
            let output = serde_json::to_string_pretty(&envelope)
                .unwrap_or_else(|_| r#"{"version":"1","status":"error"}"#.to_string());
            eprintln!("{}", output);
        }
        Format::Human => {
            eprintln!("error: {}", err);
            eprintln!("  {}", err.suggestion());
        }
    }
}

pub fn print_clap_error(format: Format, err: clap::Error) {
    match format {
        Format::Json => {
            let cli_err = CliError::InvalidInput(err.to_string());
            print_error(format, &cli_err);
        }
        Format::Human => {
            let _ = err.print();
        }
    }
}
