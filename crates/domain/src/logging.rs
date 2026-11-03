use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticLogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Off = 5,
}

impl DiagnosticLogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticLogLevel::Trace => "TRACE",
            DiagnosticLogLevel::Debug => "DEBUG",
            DiagnosticLogLevel::Info => "INFO",
            DiagnosticLogLevel::Warn => "WARN",
            DiagnosticLogLevel::Error => "ERROR",
            DiagnosticLogLevel::Off => "OFF",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSubsystem {
    Download,
    Playback,
    Storage,
    General,
}

impl DiagnosticSubsystem {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticSubsystem::Download => "Download",
            DiagnosticSubsystem::Playback => "Playback",
            DiagnosticSubsystem::Storage => "Storage",
            DiagnosticSubsystem::General => "General",
        }
    }

    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "download" | "remote" | "prefetch" => DiagnosticSubsystem::Download,
            "playback" | "audio" | "decoder" | "audiotrack" => DiagnosticSubsystem::Playback,
            "storage" | "cache" | "database" => DiagnosticSubsystem::Storage,
            _ => DiagnosticSubsystem::General,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticLogEntry {
    pub id: u64,
    pub timestamp_ms: u64,
    pub level: DiagnosticLogLevel,
    pub subsystem: DiagnosticSubsystem,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoggingSettings {
    #[serde(default = "default_logging_enabled")]
    pub enabled: bool,
    #[serde(default = "default_log_level")]
    pub min_level: DiagnosticLogLevel,
    #[serde(default = "default_true")]
    pub log_to_file: bool,
    #[serde(default = "default_true")]
    pub log_downloading: bool,
    #[serde(default = "default_true")]
    pub log_playback: bool,
    #[serde(default = "default_max_log_entries")]
    pub max_entries: usize,
}

fn default_logging_enabled() -> bool {
    true
}

fn default_log_level() -> DiagnosticLogLevel {
    DiagnosticLogLevel::Debug
}

fn default_true() -> bool {
    true
}

fn default_max_log_entries() -> usize {
    1000
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            enabled: default_logging_enabled(),
            min_level: default_log_level(),
            log_to_file: default_true(),
            log_downloading: default_true(),
            log_playback: default_true(),
            max_entries: default_max_log_entries(),
        }
    }
}

static LOG_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
static GLOBAL_LOGGER: OnceLock<DiagnosticLogger> = OnceLock::new();

pub struct DiagnosticLogger {
    settings: RwLock<LoggingSettings>,
    entries: RwLock<VecDeque<DiagnosticLogEntry>>,
    log_file_path: RwLock<Option<PathBuf>>,
    file_writer_lock: std::sync::Mutex<()>,
}

impl DiagnosticLogger {
    pub fn new(settings: LoggingSettings, log_dir: Option<PathBuf>) -> Self {
        let max_entries = settings.max_entries.max(100);
        let log_file_path = log_dir.map(|dir| dir.join("aria_diagnostics.log"));

        Self {
            settings: RwLock::new(settings),
            entries: RwLock::new(VecDeque::with_capacity(max_entries)),
            log_file_path: RwLock::new(log_file_path),
            file_writer_lock: std::sync::Mutex::new(()),
        }
    }

    pub fn global() -> &'static DiagnosticLogger {
        GLOBAL_LOGGER.get_or_init(|| {
            DiagnosticLogger::new(LoggingSettings::default(), None)
        })
    }

    pub fn set_log_dir(&self, dir: PathBuf) {
        let _ = std::fs::create_dir_all(&dir);
        let mut path_guard = self.log_file_path.write().unwrap();
        *path_guard = Some(dir.join("aria_diagnostics.log"));
    }

    pub fn get_log_file_path(&self) -> Option<PathBuf> {
        self.log_file_path.read().unwrap().clone()
    }

    pub fn get_settings(&self) -> LoggingSettings {
        self.settings.read().unwrap().clone()
    }

    pub fn update_settings(&self, new_settings: LoggingSettings) {
        let mut settings_guard = self.settings.write().unwrap();
        *settings_guard = new_settings;
    }

    pub fn log(
        &self,
        subsystem: DiagnosticSubsystem,
        level: DiagnosticLogLevel,
        message: impl Into<String>,
        details: Option<String>,
    ) {
        let (enabled, min_level, log_to_file, log_downloading, log_playback, max_entries) = {
            let s = self.settings.read().unwrap();
            (
                s.enabled,
                s.min_level,
                s.log_to_file,
                s.log_downloading,
                s.log_playback,
                s.max_entries,
            )
        };

        if !enabled || level < min_level || level == DiagnosticLogLevel::Off {
            return;
        }

        if subsystem == DiagnosticSubsystem::Download && !log_downloading {
            return;
        }
        if subsystem == DiagnosticSubsystem::Playback && !log_playback {
            return;
        }

        let message = message.into();
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let id = LOG_ID_COUNTER.fetch_add(1, Ordering::Relaxed);

        let entry = DiagnosticLogEntry {
            id,
            timestamp_ms,
            level,
            subsystem,
            message: message.clone(),
            details: details.clone(),
        };

        // 1. Android Logcat output
        #[cfg(target_os = "android")]
        {
            let prio = match level {
                DiagnosticLogLevel::Trace => 2, // VERBOSE
                DiagnosticLogLevel::Debug => 3, // DEBUG
                DiagnosticLogLevel::Info => 4,  // INFO
                DiagnosticLogLevel::Warn => 5,  // WARN
                DiagnosticLogLevel::Error => 6, // ERROR
                DiagnosticLogLevel::Off => 0,
            };
            let tag = format!("AriaDiag:{}\0", subsystem.as_str());
            let formatted = if let Some(ref d) = details {
                format!("{message} ({d})\0")
            } else {
                format!("{message}\0")
            };
            unsafe {
                extern "C" {
                    fn __android_log_print(
                        prio: std::os::raw::c_int,
                        tag: *const std::os::raw::c_char,
                        fmt: *const std::os::raw::c_char,
                        ...
                    ) -> std::os::raw::c_int;
                }
                let fmt = b"%s\0";
                __android_log_print(
                    prio,
                    tag.as_ptr() as *const std::os::raw::c_char,
                    fmt.as_ptr() as *const std::os::raw::c_char,
                    formatted.as_ptr() as *const std::os::raw::c_char,
                );
            }
        }

        // 2. Desktop stderr
        #[cfg(not(target_os = "android"))]
        {
            eprintln!(
                "[Aria][{}][{}] {}{}",
                level.as_str(),
                subsystem.as_str(),
                message,
                details.as_ref().map(|d| format!(" ({d})")).unwrap_or_default()
            );
        }

        // 3. In-memory ring buffer
        {
            let mut entries = self.entries.write().unwrap();
            while entries.len() >= max_entries {
                entries.pop_front();
            }
            entries.push_back(entry);
        }

        // 4. File append with rotation
        if log_to_file {
            let file_path = self.log_file_path.read().unwrap().clone();
            if let Some(path) = file_path {
                let _lock = self.file_writer_lock.lock().unwrap();
                self.append_to_file(&path, timestamp_ms, level, subsystem, &message, details.as_deref());
            }
        }
    }

    fn append_to_file(
        &self,
        path: &PathBuf,
        timestamp_ms: u64,
        level: DiagnosticLogLevel,
        subsystem: DiagnosticSubsystem,
        message: &str,
        details: Option<&str>,
    ) {
        // Rotate if file > 5MB
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.len() > 5 * 1024 * 1024 {
                let backup_path = path.with_extension("log.old");
                let _ = std::fs::rename(path, backup_path);
            }
        }

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let seconds = timestamp_ms / 1000;
            let millis = timestamp_ms % 1000;
            let time_str = format!("{}.{:03}", seconds, millis);

            let line = if let Some(d) = details {
                format!(
                    "[{}] [{:5}] [{:8}] {} | {}\n",
                    time_str,
                    level.as_str(),
                    subsystem.as_str(),
                    message,
                    d
                )
            } else {
                format!(
                    "[{}] [{:5}] [{:8}] {}\n",
                    time_str,
                    level.as_str(),
                    subsystem.as_str(),
                    message
                )
            };
            let _ = file.write_all(line.as_bytes());
        }
    }

    pub fn get_logs(
        &self,
        filter_subsystem: Option<DiagnosticSubsystem>,
        min_level: Option<DiagnosticLogLevel>,
        limit: Option<usize>,
    ) -> Vec<DiagnosticLogEntry> {
        let entries = self.entries.read().unwrap();
        let iter = entries.iter().filter(|entry| {
            if let Some(sub) = filter_subsystem {
                if entry.subsystem != sub {
                    return false;
                }
            }
            if let Some(lvl) = min_level {
                if entry.level < lvl {
                    return false;
                }
            }
            true
        });

        if let Some(limit) = limit {
            iter.rev().take(limit).cloned().collect::<Vec<_>>().into_iter().rev().collect()
        } else {
            iter.cloned().collect()
        }
    }

    pub fn clear(&self) {
        let mut entries = self.entries.write().unwrap();
        entries.clear();
    }

    pub fn export_text(&self) -> String {
        let entries = self.entries.read().unwrap();
        let mut out = String::with_capacity(entries.len() * 80);
        for entry in entries.iter() {
            let seconds = entry.timestamp_ms / 1000;
            let millis = entry.timestamp_ms % 1000;
            out.push_str(&format!(
                "[{}.{:03}] [{:5}] [{:8}] {}",
                seconds,
                millis,
                entry.level.as_str(),
                entry.subsystem.as_str(),
                entry.message
            ));
            if let Some(ref d) = entry.details {
                out.push_str(" | ");
                out.push_str(d);
            }
            out.push('\n');
        }
        out
    }
}

pub fn log_diagnostic(
    subsystem: DiagnosticSubsystem,
    level: DiagnosticLogLevel,
    message: impl Into<String>,
    details: Option<String>,
) {
    DiagnosticLogger::global().log(subsystem, level, message, details);
}

pub fn init_diagnostic_logger(log_dir: PathBuf, settings: Option<LoggingSettings>) {
    let logger = DiagnosticLogger::global();
    if let Some(settings) = settings {
        logger.update_settings(settings);
    }
    logger.set_log_dir(log_dir);
}

#[macro_export]
macro_rules! aria_trace {
    ($subsystem:expr, $($arg:tt)*) => {
        $crate::logging::log_diagnostic(
            $subsystem,
            $crate::logging::DiagnosticLogLevel::Trace,
            format!($($arg)*),
            None,
        );
    };
}

#[macro_export]
macro_rules! aria_debug {
    ($subsystem:expr, $($arg:tt)*) => {
        $crate::logging::log_diagnostic(
            $subsystem,
            $crate::logging::DiagnosticLogLevel::Debug,
            format!($($arg)*),
            None,
        );
    };
}

#[macro_export]
macro_rules! aria_info {
    ($subsystem:expr, $($arg:tt)*) => {
        $crate::logging::log_diagnostic(
            $subsystem,
            $crate::logging::DiagnosticLogLevel::Info,
            format!($($arg)*),
            None,
        );
    };
}

#[macro_export]
macro_rules! aria_warn {
    ($subsystem:expr, $($arg:tt)*) => {
        $crate::logging::log_diagnostic(
            $subsystem,
            $crate::logging::DiagnosticLogLevel::Warn,
            format!($($arg)*),
            None,
        );
    };
}

#[macro_export]
macro_rules! aria_error {
    ($subsystem:expr, $($arg:tt)*) => {
        $crate::logging::log_diagnostic(
            $subsystem,
            $crate::logging::DiagnosticLogLevel::Error,
            format!($($arg)*),
            None,
        );
    };
}
