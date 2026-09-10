use std::fmt;
use std::sync::{OnceLock, RwLock};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

struct Settings {
    enabled: bool,
    level: Level,
}

static SETTINGS: OnceLock<RwLock<Settings>> = OnceLock::new();

fn settings() -> &'static RwLock<Settings> {
    SETTINGS.get_or_init(|| RwLock::new(Settings {
        enabled: true,
        level: Level::Info,
    }))
}

pub fn init(enabled: bool, level: &str) {
    let level = match level.to_ascii_lowercase().as_str() {
        "error" => Level::Error,
        "warn" | "warning" => Level::Warn,
        "debug" => Level::Debug,
        _ => Level::Info,
    };
    let mut settings = settings().write().expect("logger settings poisoned");
    settings.enabled = enabled;
    settings.level = level;
}

pub fn write(level: Level, args: fmt::Arguments<'_>) {
    let settings = settings().read().expect("logger settings poisoned");
    if settings.enabled && level <= settings.level {
        match level {
            Level::Error | Level::Warn => std::eprintln!("{}", args),
            Level::Info | Level::Debug => std::println!("{}", args),
        }
    }
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::logger::write($crate::logger::Level::Debug, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::logger::write($crate::logger::Level::Info, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::logger::write($crate::logger::Level::Warn, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::logger::write($crate::logger::Level::Error, format_args!($($arg)*))
    };
}

// Keep existing print-style call sites routed through the logger.
#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        $crate::log_info!($($arg)*)
    };
}

#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => {
        $crate::log_error!($($arg)*)
    };
}
