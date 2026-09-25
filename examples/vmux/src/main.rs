//! `vmux` binary entry: browser-process startup (CEF execute path, Bevy + winit runner).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn main() -> Result<(), &'static str> {
    vmux::log::install_panic_hook();
    vmux::log::record_startup_milestone("main_after_panic_hook");
    vmux::log::lifecycle_trace_line("main_after_panic_hook");

    #[cfg(target_os = "macos")]
    if std::env::var_os("TZ").is_none() {
        // Default timezone before CEF/Chromium init; may stabilize ICU/temporal-style code paths.
        // SAFETY: `main` before other threads spawn; no concurrent `getenv` in this process yet.
        unsafe {
            std::env::set_var("TZ", "UTC");
        }
    }

    #[cfg(target_os = "macos")]
    vmux::browser::cef::macos::setup_vmux_application();

    let mut app = bevy_app::App::new();
    let log_filter = vmux::log::vmux_log_filter_string();
    app.add_plugins(bevy_log::LogPlugin {
        filter: log_filter,
        level: bevy_log::Level::INFO,
        custom_layer: vmux::log::vmux_bevy_file_custom_layer,
    })
    .add_event::<bevy_app::AppExit>();

    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        "main: start"
    );
    if vmux::log::shell_input_trace_enabled() {
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "shell_input_trace: VMUX_VIMIUM_INPUT_LOG is set; logging KeyboardInput / Ime for shell keyboard debugging"
        );
    }
    let _library = vmux::browser::cef::load_cef();
    vmux::log::record_startup_milestone("main_after_load_cef");
    vmux::log::lifecycle_trace_line("main_after_load_cef");

    let args = cef::args::Args::new();
    let shutdown = Arc::new(AtomicBool::new(false));
    let signal_quit = Arc::new(AtomicBool::new(false));
    {
        use signal_hook::{consts::SIGINT, consts::SIGTERM, flag};
        flag::register(SIGTERM, Arc::clone(&signal_quit)).expect("vmux: register SIGTERM");
        flag::register(SIGINT, Arc::clone(&signal_quit)).expect("vmux: register SIGINT");
    }

    let client_holder = Rc::new(RefCell::new(None));
    let mut cef_app = vmux::browser::cef::app::VmuxApp::new(client_holder.clone());
    let ret = cef::execute_process(
        Some(args.as_main_args()),
        Some(&mut cef_app),
        std::ptr::null_mut(),
    );
    if ret >= 0 {
        return Ok(());
    }
    assert_eq!(ret, -1, "cannot execute browser process");
    vmux::log::record_startup_milestone("main_browser_process_before_cef_initialize");
    vmux::log::lifecycle_trace_line("main_browser_process_before_cef_initialize");
    vmux::browser::cef::initialize_cef_after_execute(&args, &mut cef_app);
    vmux::log::record_startup_milestone("main_after_cef_initialize");
    vmux::log::lifecycle_trace_line("main_after_cef_initialize");

    let event_loop = vmux::runtime::build_event_loop();
    vmux::log::record_startup_milestone("main_after_build_event_loop");
    #[cfg(all(debug_assertions, target_os = "macos"))]
    vmux::browser::cef::macos::debug_assert_vmux_is_frontmost_app_subclass();
    let proxy = event_loop.create_proxy();
    app.insert_resource(vmux::runtime::ShutdownFlag(shutdown.clone()));
    app.insert_resource(vmux::runtime::SignalQuitFlag(signal_quit.clone()));
    app.insert_resource(vmux::runtime::CefPumpDeadline::default());
    vmux::runtime::register_winit_proxy_for_ffi(proxy.clone());

    app.insert_non_send_resource(vmux::runtime::VmuxMainEventLoop(event_loop))
        .insert_non_send_resource(vmux::browser::cef::CefBootstrapPending {
            cef_app,
            client_holder,
            event_proxy: proxy,
        })
        .add_plugins(vmux::VmuxPlugin::new())
        .run();

    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        "main: run_main returned"
    );
    Ok(())
}
