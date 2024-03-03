use std::fmt;
use super::helpers::generate_timestamp;
pub struct Logger;

impl Logger {
    pub fn time_now() -> String {
        let now_millis = generate_timestamp();
        let now = now_millis / 1000; // Convert milliseconds to seconds
        format!("{:02}:{:02}:{:02}", now / 3600, (now / 60) % 60, now % 60)
    }

    pub fn log(&self, level: LogLevel, msg: &str) {
        println!("{} | {:<8} | {}", Self::time_now(), level, msg);
    }

    pub fn success(&self, msg: &str) {
        self.log(LogLevel::Success, msg);
    }

    pub fn info(&self, msg: &str) {
        self.log(LogLevel::Info, msg);
    }

    pub fn debug(&self, msg: &str) {
        self.log(LogLevel::Debug, msg);
    }

    pub fn warning(&self, msg: &str) {
        self.log(LogLevel::Warning, msg);
    }

    pub fn error(&self, msg: &str) {
        self.log(LogLevel::Error, msg);
    }

    pub fn critical(&self, msg: &str) {
        self.log(LogLevel::Critical, msg);
    }
}

pub enum LogLevel {
    Success,
    Info,
    Debug,
    Warning,
    Error,
    Critical,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                LogLevel::Success => "SUCCESS",
                LogLevel::Info => "INFO",
                LogLevel::Debug => "DEBUG",
                LogLevel::Warning => "WARNING",
                LogLevel::Error => "ERROR",
                LogLevel::Critical => "CRITICAL",
            }
        )
    }
}
