//! Crash reports.
//!
//! This project's code never panics (`#![deny(clippy::panic, …)]`), but a dependency can. On a
//! tablet, without a cable and logcat, nobody would ever know why the app closed. [`install`] adds
//! a panic hook that writes the panic to `crash.log` in the settings folder, besides the log: the
//! message, where it happened, the thread, the backtrace and the app's version. At the next launch,
//! [`take_report`] moves that file to `crash-last.log` and returns its path. The shell then tells
//! the user with a notice, and offers to send the report (Actions › Quick) where the platform can.

use std::path::{Path, PathBuf};

/// The report the last panic wrote.
pub const LOG: &str = "crash.log";
/// The report a launch found, kept for sending.
pub const LAST: &str = "crash-last.log";
/// `crash.log` keeps at most its last this many bytes (several panics in a row add up).
const MAX_BYTES: usize = 64 * 1024;

/// One panic's report.
pub fn report(version: &str, unix_secs: u64, thread: &str, message: &str, location: &str, backtrace: &str) -> String {
    format!("PhotoCraft Mobile {version}\ntime: {unix_secs} (Unix seconds)\nthread: {thread}\npanic: {message}\nat: {location}\n\nbacktrace:\n{backtrace}\n")
}

/// Appends `text` to the report at `path`, keeping only its last [`MAX_BYTES`] (cut at a line).
pub fn append(path: &Path, text: &str) -> std::io::Result<()> {
    let mut all = std::fs::read_to_string(path).unwrap_or_default();
    if !all.is_empty() {
        all.push_str("\n----\n\n");
    }
    all.push_str(text);
    if all.len() > MAX_BYTES {
        let mut cut = all.len() - MAX_BYTES;
        while !all.is_char_boundary(cut) {
            cut += 1;
        }
        let from = all.get(cut..).and_then(|rest| rest.find('\n')).map_or(cut, |n| cut + n + 1);
        all = all.get(from..).unwrap_or_default().to_string();
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, all)
}

/// Writes each panic to `<dir>/crash.log` (and the log), then lets the previous hook run.
/// `version` names the build in the report.
pub fn install(dir: Option<PathBuf>, version: String) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload.downcast_ref::<&str>().copied().or_else(|| payload.downcast_ref::<String>().map(String::as_str)).unwrap_or("(no message)");
        let location = info.location().map(|l| l.to_string()).unwrap_or_default();
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let text = report(&version, now, thread.name().unwrap_or("unnamed"), message, &location, &backtrace);
        log::error!("{text}");
        if let Some(dir) = &dir
            && let Err(e) = append(&dir.join(LOG), &text)
        {
            log::error!("couldn't write the crash report: {e}");
        }
        previous(info);
    }));
}

/// The report a crash left in `dir` since the last launch, moved to `crash-last.log` (its path).
pub fn take_report(dir: Option<&Path>) -> Option<PathBuf> {
    let dir = dir?;
    let log = dir.join(LOG);
    if !log.is_file() {
        return None;
    }
    let last = dir.join(LAST);
    match std::fs::rename(&log, &last) {
        Ok(()) => Some(last),
        Err(e) => {
            log::warn!("couldn't keep the crash report: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("photocraft-mobile-crash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_report_names_the_build_the_place_and_the_message() {
        let text = report("0.1.18", 1_800_000_000, "main", "index out of bounds", "src/x.rs:3:7", "0: frame");
        for part in ["0.1.18", "1800000000", "thread: main", "panic: index out of bounds", "at: src/x.rs:3:7", "0: frame"] {
            assert!(text.contains(part), "{part} in {text}");
        }
    }

    #[test]
    fn reports_add_up_within_the_size_cap() {
        let dir = temp_dir("append");
        let path = dir.join(LOG);
        append(&path, "first\n").unwrap();
        append(&path, "second\n").unwrap();
        let both = std::fs::read_to_string(&path).unwrap();
        assert!(both.starts_with("first") && both.contains("second"));
        let line = "x".repeat(99) + "\n";
        append(&path, &line.repeat(2000)).unwrap();
        let capped = std::fs::read_to_string(&path).unwrap();
        assert!(capped.len() <= MAX_BYTES);
        assert!(!capped.contains("first"), "the oldest report goes first");
        assert!(capped.starts_with('x'), "cut at a line");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_report_is_taken_once() {
        let dir = temp_dir("take");
        assert_eq!(take_report(Some(&dir)), None);
        assert_eq!(take_report(None), None);
        append(&dir.join(LOG), "boom\n").unwrap();
        let last = take_report(Some(&dir)).unwrap();
        assert_eq!(last, dir.join(LAST));
        assert_eq!(std::fs::read_to_string(&last).unwrap(), "boom\n");
        assert_eq!(take_report(Some(&dir)), None, "the next launch finds nothing new");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
