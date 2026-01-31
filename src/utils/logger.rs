use env_logger::Builder;

/// ANSI color codes for terminal output
pub struct Colors;

impl Colors {
    pub const ADDED: &'static str = "\x1b[32m"; // Green
    pub const REMOVED: &'static str = "\x1b[31m"; // Red
    pub const MODIFIED: &'static str = "\x1b[33m"; // Yellow
    pub const BRAND: &'static str = "\x1b[1;36m"; // Bold Cyan
    pub const RESET: &'static str = "\x1b[0m"; // Reset

    /// Helper to format a colored label with a value
    pub fn colorize(color: &str, text: &str) -> String {
        format!("{}{}{}", color, text, Self::RESET)
    }
}

pub fn setup_logging(verbose: bool) {
    use log::LevelFilter;
    use std::io::Write;

    let level = if verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    Builder::from_default_env()
        .filter_level(LevelFilter::Warn) // Default: only warnings from dependencies
        .filter_module(env!("CARGO_PKG_NAME"), level) // Our crate: use requested level
        .format(|buf, record| {
            writeln!(
                buf,
                "{}[{}]{} {}",
                Colors::BRAND,
                env!("CARGO_PKG_NAME"),
                Colors::RESET,
                record.args()
            )
        })
        .init();
}
