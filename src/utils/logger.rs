use colored::Colorize;
use env_logger::Builder;
use log::Level;
use std::io::Write;

pub fn setup_logging(verbose: bool) {
    use log::LevelFilter;

    let level = if verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    Builder::from_default_env()
        .filter_level(LevelFilter::Warn) // Default: only warnings from dependencies
        .filter_module(env!("CARGO_PKG_NAME"), level) // Our crate: use requested level
        .format(|buf, record| {
            let name = env!("CARGO_PKG_NAME");
            let line = match record.level() {
                Level::Error | Level::Warn => {
                    let level_str = match record.level() {
                        Level::Warn => "WARN".yellow(),
                        Level::Error => "ERROR".red(),
                        _ => unreachable!(),
                    };
                    let path = record.target().to_string().white();
                    format!("[{} {} {}] {}", name.cyan(), level_str, path, record.args())
                }
                _ => format!("[{}] {}", name.cyan(), record.args()),
            };
            writeln!(buf, "{}", line)
        })
        .init();
}
