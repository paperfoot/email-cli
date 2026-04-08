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
        let msg = err.to_string().to_lowercase();

        // "not found" errors are input errors, not internal failures.
        if msg.contains("not found")
            || msg.contains("no such")
            || msg.contains("does not exist")
            || msg.contains("404")
        {
            return Self::InvalidInput(err.to_string());
        }

        // CLI-side validation failures from arg parsing helpers (e.g. parse_properties_arg,
        // parse_topics_arg, topic create --default-subscription) all use phrases like
        // "must be ...", "must contain", "invalid value", "invalid json". Map them all to
        // InvalidInput so agents see exit code 3.
        if msg.contains("must be ")
            || msg.contains("must contain")
            || msg.contains("invalid json")
            || msg.contains("invalid value")
        {
            return Self::InvalidInput(err.to_string());
        }

        // Rate-limit exhaustion paths in resend.rs use phrases like
        // "kept rate limiting" and "429". Map them to RateLimited so callers get exit 4.
        if msg.contains("kept rate limiting")
            || msg.contains("429")
            || msg.contains("rate limit")
            || msg.contains("too many requests")
        {
            return Self::RateLimited(err.to_string());
        }

        // Connection / network errors are transient — retryable.
        if msg.contains("connection")
            || msg.contains("timed out")
            || msg.contains("timeout")
            || msg.contains("network")
            || msg.contains("dns")
            || msg.contains("unreachable")
            || msg.contains("reset by peer")
            || msg.contains("broken pipe")
            || msg.contains("eof")
        {
            return Self::Transient(err.to_string());
        }

        Self::Internal(err)
    }
}
