use std::fmt;
use super::helpers::generate_timestamp;

#[derive(Debug, Clone)]
pub struct Logger;

impl Logger {
    pub fn log(level: LogLevel, msg: &str) {
        let now = generate_timestamp() / 1000; // Convert milliseconds to seconds
        let (h, m, s) = (now / 3600, (now % 3600) / 60, now % 60);
        println!("{:02}:{:02}:{:02} | {:<8} | {}", h, m, s, level, msg);
    }

    pub fn success(&self, msg: &str) {
        Self::log(LogLevel::Success, msg);
    }

    pub fn info(&self, msg: &str) {
        Self::log(LogLevel::Info, msg);
    }

    pub fn debug(&self, msg: &str) {
        Self::log(LogLevel::Debug, msg);
    }

    pub fn warning(&self, msg: &str) {
        Self::log(LogLevel::Warning, msg);
    }

    pub fn error(&self, msg: &str) {
        Self::log(LogLevel::Error, msg);
    }

    pub fn critical(&self, msg: &str) {
        Self::log(LogLevel::Critical, msg);
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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
        const LEVEL_NAMES: [&str; 6] = ["SUCCESS", "INFO", "DEBUG", "WARNING", "ERROR", "CRITICAL"];
        let idx = *self as usize;
        write!(f, "{}", LEVEL_NAMES[idx])
    }
}
