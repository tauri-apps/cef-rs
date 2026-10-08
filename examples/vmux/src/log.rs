//! Path helpers beside the macOS app bundle, file-based Bevy/tracing logs, panic capture, and lifecycle
//! breadcrumbs. Opt-in shell keyboard / IME tracing: set **`VMUX_VIMIUM_INPUT_LOG`** (same as
//! [`crate::input::shell`] / vimium dispatch).

// --- bundle path helpers (files beside `.app`) ---

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// `…/Foo.app/Contents/MacOS/exe` → parent of `Foo.app` (directory containing the bundle).
pub fn beside_macos_app_bundle_parent(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    if macos.file_name() != Some(OsStr::new("MacOS")) {
        return None;
    }
    let contents = macos.parent()?;
    let app = contents.parent()?;
    if app.extension() != Some(OsStr::new("app")) {
        return None;
    }
    app.parent()
}

/// Resolved directory for logs beside the bundle, or next to the executable, or `None` for cwd fallback.
pub fn log_dir_beside_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if let Some(parent) = beside_macos_app_bundle_parent(&exe) {
        return Some(parent.to_path_buf());
    }
    exe.parent().map(Path::to_path_buf)
}

/// True when **`VMUX_VIMIUM_INPUT_LOG`** is set to a non-empty value (shell keyboard / IME tracing).
#[inline]
pub fn shell_input_trace_enabled() -> bool {
    std::env::var_os("VMUX_VIMIUM_INPUT_LOG").is_some_and(|v| !v.is_empty())
}

/// Back-compat alias for [`shell_input_trace_enabled`].
#[inline]
pub fn vimium_input_trace_enabled() -> bool {
    shell_input_trace_enabled()
}

// --- panic log (Finder / `open` loses stderr) ---

use std::io::Write;

/// Install first thing in `main` (before other init). Writes panics beside the `.app` (`VMUX_PANIC_LOG` override).
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_panic_file(info);
        default_hook(info);
    }));
}

fn write_panic_file(info: &std::panic::PanicHookInfo<'_>) -> std::io::Result<()> {
    let path = panic_log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let _ = writeln!(f);
    let _ = writeln!(
        f,
        "--- vmux Rust panic pid={} --- {}",
        std::process::id(),
        chrono_like_timestamp()
    );
    let _ = writeln!(f, "{info}");
    let bt = std::backtrace::Backtrace::capture();
    let _ = writeln!(f, "{bt}");
    let _ = writeln!(f, "--- end ---");
    Ok(())
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let Ok(d) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return String::new();
    };
    format!("(unix_s={})", d.as_secs())
}

fn panic_log_path() -> PathBuf {
    if let Ok(p) = std::env::var("VMUX_PANIC_LOG") {
        let p = p.trim();
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Some(dir) = log_dir_beside_exe() {
        return dir.join("vmux-rust-panic.log");
    }
    PathBuf::from("vmux-rust-panic.log")
}

// --- Bevy / tracing file log beside bundle ---

use std::error::Error;
use std::sync::Mutex;

use bevy_app::App;
use bevy_log::tracing_subscriber::{
    EnvFilter, Layer,
    filter::{FromEnvError, ParseError},
    fmt,
};
use bevy_log::{BoxedLayer, Level};

/// Same filter string as [`crate::main`] passes to [`bevy_log::LogPlugin::filter`].
pub fn vmux_log_filter_string() -> String {
    format!(
        "vmux=info,bevy_app=info,bevy_ecs=info,bevy_render=info,{}",
        bevy_log::DEFAULT_FILTER
    )
}

fn file_env_filter() -> EnvFilter {
    let default_filter = format!("{},{}", Level::INFO, vmux_log_filter_string());
    EnvFilter::try_from_default_env()
        .or_else(|from_env_error| {
            _ = from_env_error
                .source()
                .and_then(|source| source.downcast_ref::<ParseError>())
                .map(|parse_err| {
                    eprintln!("vmux file LogPlugin: failed to parse RUST_LOG: {parse_err}");
                });
            Ok::<EnvFilter, FromEnvError>(EnvFilter::builder().parse_lossy(&default_filter))
        })
        .unwrap()
}

/// Path for the Bevy/tracing log file. Writes **`vmux-cef-log-path.txt`** next to the resolved Chromium log file.
pub fn write_cef_log_path_pointer(cef_log_path: &Path) {
    let Some(parent) = cef_log_path.parent() else {
        return;
    };
    let _ = std::fs::create_dir_all(parent);
    let pointer = parent.join("vmux-cef-log-path.txt");
    let line = cef_log_path
        .canonicalize()
        .unwrap_or_else(|_| cef_log_path.to_path_buf())
        .display()
        .to_string();
    let _ = std::fs::write(&pointer, format!("{line}\n"));
}

pub fn vmux_bevy_log_path() -> PathBuf {
    if let Ok(p) = std::env::var("VMUX_BEVY_LOG") {
        let p = p.trim();
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Some(dir) = log_dir_beside_exe() {
        return dir.join("vmux-bevy.log");
    }
    PathBuf::from("vmux-bevy.log")
}

fn try_append_log_file(path: &Path) -> Option<std::fs::File> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
}

/// Pass to [`bevy_log::LogPlugin::custom_layer`]. Appends filtered tracing output to [`vmux_bevy_log_path`].
pub fn vmux_bevy_file_custom_layer(_app: &mut App) -> Option<BoxedLayer> {
    let primary = vmux_bevy_log_path();
    let pid = std::process::id();
    let tmp_fallback = std::env::temp_dir().join(format!("vmux-bevy-{pid}.log"));

    let (used_path, mut file) = if let Some(f) = try_append_log_file(&primary) {
        (primary, f)
    } else if let Some(f) = try_append_log_file(&tmp_fallback) {
        eprintln!(
            "vmux: could not open {} for logging — using {}",
            primary.display(),
            tmp_fallback.display()
        );
        (tmp_fallback, f)
    } else {
        eprintln!(
            "vmux: could not open log file (tried {} and {})",
            primary.display(),
            tmp_fallback.display()
        );
        return None;
    };

    let path_line = std::fs::canonicalize(&used_path)
        .unwrap_or_else(|_| used_path.clone())
        .display()
        .to_string();
    let _ = writeln!(
        file,
        "\n--- vmux-bevy log session pid={pid} path={path_line} ---"
    );
    let _ = file.sync_all();
    if let Some(dir) = used_path.parent() {
        let pointer = dir.join("vmux-bevy-log-path.txt");
        let _ = std::fs::write(&pointer, format!("{path_line}\n"));
    }
    let writer = Mutex::new(file);
    let filter = file_env_filter();
    Some(Box::new(
        fmt::layer()
            .with_ansi(false)
            .with_writer(writer)
            .with_filter(filter),
    ))
}

// --- lifecycle trace (env + always-on milestones) ---

/// Synced lines when **`VMUX_LIFECYCLE_TRACE=1`** to **`vmux-lifecycle-trace.txt`** (tmp + beside bundle).
pub fn lifecycle_trace_line(msg: &str) {
    if std::env::var("VMUX_LIFECYCLE_TRACE").ok().as_deref() != Some("1") {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!("ts_ms={ts} pid={} {msg}", std::process::id());
    let mut paths = vec![std::env::temp_dir().join("vmux-lifecycle-trace.txt")];
    if let Some(dir) = log_dir_beside_exe() {
        paths.push(dir.join("vmux-lifecycle-trace.txt"));
    }
    for path in paths {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
            let _ = f.sync_all();
        }
    }
}

fn ts_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Writes startup progress to disk **without** env vars — survives Finder launches and CEF traps.
pub fn record_startup_milestone(msg: &str) {
    let line = format!("ts_ms={} pid={} {msg}\n", ts_ms(), std::process::id());
    let mut dirs = vec![std::env::temp_dir()];
    if let Some(d) = log_dir_beside_exe() {
        dirs.push(d);
    }
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        let _ = write_milestone_files(&dir, &line);
    }
}

fn write_milestone_files(dir: &Path, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let log_path = dir.join("vmux-startup-milestone.log");
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        f.write_all(line.as_bytes())?;
        f.sync_all()?;
    }
    let txt_path = dir.join("vmux-startup-milestone.txt");
    std::fs::write(txt_path, line.trim_end())?;
    Ok(())
}

/// Quit / composite / pump breadcrumbs (always on). Same directories as [`record_startup_milestone`].
pub fn record_runtime_event(msg: &str) {
    let line = format!("ts_ms={} pid={} {msg}\n", ts_ms(), std::process::id());
    let mut dirs = vec![std::env::temp_dir()];
    if let Some(d) = log_dir_beside_exe() {
        dirs.push(d);
    }
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        let _ = write_runtime_event_files(&dir, &line);
    }
}

fn write_runtime_event_files(dir: &Path, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let log_path = dir.join("vmux-runtime-events.log");
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        f.write_all(line.as_bytes())?;
        f.sync_all()?;
    }
    let txt_path = dir.join("vmux-runtime-events-last.txt");
    std::fs::write(txt_path, line.trim_end())?;
    Ok(())
}
