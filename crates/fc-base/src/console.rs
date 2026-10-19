//! Minimal console logger, mirroring the spirit of FreeCAD `Base::Console`.

use std::io::Write;

/// Message severity, mirroring FreeCAD `Base::LogStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Debug,
    Log,
    Warn,
    Error,
}

/// Emit one line to stderr with a UTC timestamp and level tag.
///
/// Thread-safe; lines never interleave.
pub fn emit(level: Level, message: &str) {
    let _ = std::io::stderr().lock(); // serialize writers
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let tag = match level {
        Level::Debug => "DEBUG",
        Level::Log => "LOG",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    };
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "[{now:010}][{tag}] {message}");
}

pub fn debug(message: &str) {
    emit(Level::Debug, message);
}

pub fn log(message: &str) {
    emit(Level::Log, message);
}

pub fn warn(message: &str) {
    emit(Level::Warn, message);
}

pub fn error(message: &str) {
    emit(Level::Error, message);
}
