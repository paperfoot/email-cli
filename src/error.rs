use std::fmt;

#[derive(Debug)]
#[allow(dead_code)]
pub enum CliError {
    InvalidInput(String),
    ConfigError(String),
    Transient(String),
    RateLimited(String),
    Internal(anyhow::Error),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidInput(_) => 3,
            Self::ConfigError(_) => 2,
            Self::Transient(_) => 1,
            Self::RateLimited(_) => 4,
            Self::Internal(_) => 1,
        }
    }

    pub fn error_code(&self) -> &str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::ConfigError(_) => "config_error",
            Self::Transient(_) => "transient_error",
            Self::RateLimited(_) => "rate_limited",
            Self::Internal(_) => "internal_error",
        }
    }

    pub fn suggestion(&self) -> &str {
        match self {
            Self::InvalidInput(_) => "Check arguments with --help",
            Self::ConfigError(_) => "Run profile add / account add to configure",
            Self::Transient(_) => "Retry the command",
            Self::RateLimited(_) => "Wait a moment and retry",
            Self::Internal(_) => "Retry or report the issue",
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(msg) => write!(f, "{}", msg),
            Self::ConfigError(msg) => write!(f, "{}", msg),
            Self::Transient(msg) => write!(f, "{}", msg),
            Self::RateLimited(msg) => write!(f, "{}", msg),
            Self::Internal(err) => write!(f, "{}", err),
        }
    }
}

impl std::error::Error for CliError {}

impl From<anyhow::Error> for CliError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }
}
