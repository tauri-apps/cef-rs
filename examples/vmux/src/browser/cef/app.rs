//! CEF **`cef::App`** implementation (`VmuxApp`): command-line switches, env-driven Chromium flags, and
//! **`AppBrowserProcessHandler`** (pump scheduling, child-process switches). OSR + winit + wgpu live in
//! [`crate::runtime`] and `browser::cef::shell` / `browser::cef::renderer`.
//!
//! ## Extra Chromium switches
//!
//! Set **`VMUX_CEF_EXTRA_SWITCHES`** to whitespace-separated flags (optional `--` prefix). Use
//! `name=value` when a value is required, e.g. `disable-features=First,Second`. Parsed **after** the
//! built-in switches below; if you rely on `disable-features` from vmux (e.g. `BackForwardCache` when
//! `accelerated_osr` is on), include those tokens in your env string or they may be overridden.
//! Core switches are applied for **browser** and **renderer** only. CEF warns that mutating
//! **GPU / utility / other** subprocess command lines is risky; those processes are left alone.
//!
//! ## Chromium GPU (stability experiments)
//!
//! Set **`VMUX_CEF_DISABLE_GPU=1`** to append **`disable-gpu`** + **`disable-gpu-compositing`**. Runtime
//! traces showed **`Trace/BPT`** still occurs with these on, so this is not a guaranteed fix for the
//! current failure — use for A/B tests only.
//!
//! ### Immediate quit / “crash” right after launch (macOS)
//!
//! If the window never really appears and the process exits (~1s), this is usually **Chromium**
//! hitting **`EXC_BREAKPOINT` in `fontations_ffi`** (exit code **133** / `SIGTRAP`), **not** a Rust
//! `panic`. Confirm with Terminal:
//! **`/path/to/vmux.app/Contents/MacOS/vmux`** (logs go to **`vmux-bevy.log`** next to the `.app`).
//!
//! **Confirm in LLDB** (stops on the `brk` in the framework; thread name is often `CrBrowserMain`):
//! `lldb -o run -o bt -o quit -- /path/to/vmux.app/Contents/MacOS/vmux` — stacks often show
//! `fontations_ffi$cxxbridge1$…BridgeMappingIndex$operator$alignof` (or similar) inside
//! **`Chromium Embedded Framework`**. **`debug.log`** may show **`Mach rendezvous failed`** /
//! **`SSL handshake failed`** from helpers after the browser process has already trapped; treat those
//! as follow-on noise unless the parent stays alive.
//!
//! **Reading `~/Library/Logs/DiagnosticReports/vmux*.ips`:** **`CrBrowserMain`** + **`EXC_BREAKPOINT`**
//! with frame **`fontations_ffi$cxxbridge1$…BridgeMappingIndex$operator$alignof`** in
//! **`Chromium Embedded Framework`** is the same **Skia Fontations** `brk` — **not** a vmux Rust bug.
//! Frames like **`cef_zip_reader_create`** / **`temporal_rs_`** are still inside **`org.cef.framework`**
//! (see **`usedImages`** for the build, e.g. **146.0.6**). Rust **`do_message_loop_work`** → **`vmux` `pump`**
//! only means the external pump ran Chromium work and the framework trapped there. **macOS 26+** with
//! **CEF 146** has been observed here; there is no reliable flag to turn Fontations off — you need a
//! **newer CEF/Chromium** (or an upstream fix) built for your OS.
//!
//! 1. **`Contents/Resources/icudtl.dat`** must exist (see warning in logs). **`bundle-cef-app`**
//!    copies it from the CEF framework; if missing, copy manually from
//!    **`…/Chromium Embedded Framework.framework/Resources/icudtl.dat`**.
//! 2. First window + wgpu surface are created from **`winit::ApplicationHandler::resumed`**; on **macOS**
//!    async **`browser_host_create_browser`** runs in that same callback (after wgpu), matching **`examples/osr`**
//!    ordering; **`browser_host_create_browser_sync`** has trapped inside Chromium on some CEF 146 + OS builds.
//!
//! 3. Rebuild the **whole** bundle: **`cargo run -p cef --bin bundle-cef-app -- vmux -o target/bundle`**
//!    — do not only replace **`Contents/MacOS/vmux`**; copy **`vmux_helper`** into every
//!    **`vmux Helper*.app`** under **`Contents/Frameworks`** as well, or subprocesses desync.
//!    **`macos.rs`** uses **`#[name = "VmuxApplication"]`** so any tooling that resolves
//!    that class by name matches the ObjC runtime (objc2’s default name is **`module_path::…` +
//!    version**). vmux intentionally omits **`NSMainNibFile`** / **`NSPrincipalClass`** from the
//!    bundled plist so Launch Services does not initialize AppKit ahead of Rust `main` (that order
//!    breaks CEF `CrApp` + winit). Use **`VmuxApplication::shared_application()`** in `main` and the
//!    winit shell for Cmd+Q.
//! 4. Clear **`~/.local/share/vmux-cef`** (stale profile).
//! 5. Other experiments: **`VMUX_CEF_SINGLE_PROCESS=1`**, **`VMUX_CEF_DISABLE_GPU=1`**,
//!    **`VMUX_CEF_EXTRA_SWITCHES`**. If it still traps in the **browser** process, the build may need a
//!    **newer CEF** (workspace / `download-cef`) than this repo pins.
//! 6. If it still dies, capture **`~/Library/Logs/DiagnosticReports/vmux*.ips`** and report to
//!    **CEF/Chromium** with **macOS version** and **CEF build** (see workspace `cef` crate version).
//!
//! **`VMUX_LIFECYCLE_TRACE=1`**: append one line per milestone to **`$TMPDIR/vmux-lifecycle-trace.txt`**
//! and, when writable, **`vmux-lifecycle-trace.txt`** next to the **`.app`** (same folder as
//! **`vmux-bevy.log`**). On **macOS**, **`open Foo.app`** often **does not pass environment variables**
//! to the app; run the binary directly, e.g.
//! **`VMUX_LIFECYCLE_TRACE=1 path/to/Foo.app/Contents/MacOS/vmux`**.
//!
//! **Always-on:** **`vmux-startup-milestone.txt`** / **`vmux-startup-milestone.log`** (beside the `.app`
//! and under **`$TMPDIR`**) record the last Rust checkpoint — useful when Chromium traps with no Rust panic.
//!
//! **`single-process` (opt-in)**: Stock CEF on **macOS** is **multiprocess** (like **`examples/osr`**).
//! Set **`VMUX_CEF_SINGLE_PROCESS=1`** to force **`single-process`** — on some macOS + **CEF 146** builds
//! this triggers Skia/fontation **`EXC_BREAKPOINT`** (exit **133**) in the browser process; avoid unless
//! you know you need it.
//!
//! **`VMUX_CEF_ROOT_CACHE`**: absolute path override for Chromium’s user-data / cache root (passed to
//! [`cef::Settings::root_cache_path`] in `initialize_cef_after_execute`).
//!
//! **`VMUX_CEF_FRESH_PROFILE=1`**: use a unique cache subdirectory under `~/.local/share/` each run
//! (avoids a poisoned profile when debugging startup).
//!
//! **`VMUX_CEF_DEV_INSECURE=1`**: append legacy dev switches to **child** processes (`disable-web-security`,
//! `ignore-certificate-errors`, etc.). **Off by default** so TLS and security behavior match normal
//! Chromium and sites like Google search are less likely to flag the client as automated.
//!
//! **`VMUX_CEF_USE_MOCK_KEYCHAIN=1`**: append **`use-mock-keychain`** (macOS). **Off by default** so behavior
//! is closer to Arc/Chrome; enable if you need CI or to avoid keychain prompts.
//!
//! **`VMUX_CEF_CHILD_STDERR_LOGGING=1`**: append **`enable-logging=stderr`** to child processes. **Off by default**.
use ::cef::*;
use std::cell::RefCell;
use std::rc::Rc;

fn process_type_label(process_type: Option<&CefStringUtf16>) -> Option<String> {
    process_type.map(|pt| CefStringUtf8::from(pt).to_string())
}

fn vmux_should_disable_chromium_gpu() -> bool {
    std::env::var("VMUX_CEF_DISABLE_GPU")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn vmux_single_process_from_env() -> bool {
    std::env::var("VMUX_CEF_SINGLE_PROCESS")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// **macOS:** default is **multiprocess** (same as **`examples/osr`** / stock CEF). Set
/// **`VMUX_CEF_SINGLE_PROCESS=1`** for single-process (can worsen Skia/fontations traps on some builds).
fn vmux_wants_single_process_browser() -> bool {
    vmux_single_process_from_env()
}

fn vmux_dev_insecure_chromium_from_env() -> bool {
    std::env::var("VMUX_CEF_DEV_INSECURE")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn vmux_use_mock_keychain_from_env() -> bool {
    std::env::var("VMUX_CEF_USE_MOCK_KEYCHAIN")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn vmux_child_stderr_logging_from_env() -> bool {
    std::env::var("VMUX_CEF_CHILD_STDERR_LOGGING")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// Switches for the browser and renderer processes (not GPU/utility helpers).
fn append_vmux_core_chromium_switches(command_line: &mut CommandLine) {
    command_line.append_switch(Some(&"no-startup-window".into()));
    command_line.append_switch(Some(&"noerrdialogs".into()));
    command_line.append_switch(Some(&"hide-crash-restore-bubble".into()));
    // Avoid `navigator.webdriver`-style automation signals that many sites (including Google) treat as bots.
    command_line.append_switch_with_value(
        Some(&"disable-blink-features".into()),
        Some(&"AutomationControlled".into()),
    );
    #[cfg(target_os = "macos")]
    if vmux_use_mock_keychain_from_env() {
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "cef: VMUX_CEF_USE_MOCK_KEYCHAIN — appending use-mock-keychain"
        );
        command_line.append_switch(Some(&"use-mock-keychain".into()));
    }
    #[cfg(feature = "accelerated_osr")]
    {
        command_line.append_switch_with_value(
            Some(&"disable-features".into()),
            Some(&"BackForwardCache".into()),
        );
    }

    if vmux_should_disable_chromium_gpu() {
        let reason = "VMUX_CEF_DISABLE_GPU";
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "cef: {} — appending disable-gpu / disable-gpu-compositing",
            reason
        );
        command_line.append_switch(Some(&"disable-gpu".into()));
        command_line.append_switch(Some(&"disable-gpu-compositing".into()));
    }
}

fn append_extra_cef_switches_from_env(command_line: &mut CommandLine) {
    let Ok(raw) = std::env::var("VMUX_CEF_EXTRA_SWITCHES") else {
        return;
    };
    for token in raw.split_whitespace() {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let token = token.trim_start_matches('-');
        if let Some((name, value)) = token.split_once('=') {
            if name.is_empty() {
                continue;
            }
            command_line.append_switch_with_value(Some(&name.into()), Some(&value.into()));
        } else {
            command_line.append_switch(Some(&token.into()));
        }
    }
}

wrap_app! {
    pub struct VmuxApp {
        client_holder: Rc<RefCell<Option<Client>>>,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefStringUtf16>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(command_line) = command_line else {
                return;
            };
            let kind = process_type_label(process_type);
            let is_browser = kind.is_none();
            let is_renderer = kind.as_deref() == Some("renderer");

            // Browser + renderer need the same core flags. (Previously only the browser received them;
            // the renderer saw almost nothing and could exit immediately.)
            if is_browser || is_renderer {
                append_vmux_core_chromium_switches(command_line);
                if is_browser {
                    append_extra_cef_switches_from_env(command_line);
                    if vmux_wants_single_process_browser() {
                        bevy_log::info!(
                            target: "vmux",
                            pid = std::process::id(),
                            "cef: VMUX_CEF_SINGLE_PROCESS — appending single-process (unstable on some macOS builds)"
                        );
                        command_line.append_switch(Some(&"single-process".into()));
                    }
                }
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(AppBrowserProcessHandler::new(self.client_holder.clone()))
        }
    }
}

wrap_browser_process_handler! {
    pub struct AppBrowserProcessHandler {
        client_holder: Rc<RefCell<Option<Client>>>,
    }

    impl BrowserProcessHandler {
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            crate::runtime::schedule_cef_work(delay_ms);
        }

        fn on_context_initialized(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            // `cef::initialize` runs in `main` before the winit event loop (see `initialize_cef_after_execute`).
            // Client + headless wgpu are filled in Bevy `Startup` after that, not here.
        }

        /// Optional dev switches for child processes — **off by default**; set **`VMUX_CEF_DEV_INSECURE=1`** to opt in.
        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            let Some(command_line) = command_line else {
                return;
            };
            command_line.append_switch(Some(&"disable-session-crashed-bubble".into()));
            if vmux_child_stderr_logging_from_env() {
                command_line.append_switch(Some(&"enable-logging=stderr".into()));
            }
            if vmux_dev_insecure_chromium_from_env() {
                bevy_log::warn!(
                    target: "vmux",
                    pid = std::process::id(),
                    "cef: VMUX_CEF_DEV_INSECURE — appending disable-web-security / ignore-certificate-errors to child processes"
                );
                command_line.append_switch(Some(&"disable-web-security".into()));
                command_line.append_switch(Some(&"allow-running-insecure-content".into()));
                command_line.append_switch(Some(&"ignore-certificate-errors".into()));
                command_line.append_switch(Some(&"ignore-ssl-errors".into()));
            }
        }

        fn default_client(&self) -> Option<Client> {
            self.client_holder.borrow().clone()
        }
    }
}
