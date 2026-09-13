//! CEF integration: pump, OSR paint path, shell host, and Bevy `CefPlugin`.
//!
//! **Supported API of this module:** [`CefPlugin`] only (registers CEF thread context, **OSR** render-graph
//! resources, [`osr`] hub/render types, and `Startup` browser wiring). The browser binary also uses
//! [`CefBootstrapPending`] + public CEF load/init helpers. Other `pub` items exist for Bevy `Res<T>` on
//! `pub` systems and internal call sites — not a stable extension surface.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bevy_app::{App, Plugin, Startup};
use bevy_ecs::prelude::Resource;
use bevy_ecs::schedule::IntoSystemConfigs;
use bevy_ecs::world::World;
use winit::event_loop::EventLoopProxy;

use crate::input::system::EditableFocusQueues;
use crate::runtime::AppUserEvent;

// ---------------------------------------------------------------------------
// Chromium external message pump (`do_message_loop_work`)
// ---------------------------------------------------------------------------

use ::cef::{ThreadId, currently_on, do_message_loop_work};

#[inline]
pub fn message_loop_pump(times: u32) {
    for _ in 0..times {
        do_message_loop_work();
    }
}

#[inline]
pub fn main_message_loop_tick(post_create_extra: u32) {
    message_loop_pump(1 + post_create_extra);
}

// ---------------------------------------------------------------------------
// CEF thread identity
// ---------------------------------------------------------------------------

#[derive(Resource, Clone, Copy)]
pub struct CefThreadContext {
    pub ui: ThreadId,
}

impl Default for CefThreadContext {
    fn default() -> Self {
        Self { ui: ThreadId::UI }
    }
}

impl CefThreadContext {
    #[inline]
    pub fn on_cef_ui_thread(&self) -> bool {
        on_cef_ui_thread()
    }
}

#[inline]
pub fn cef_ui_thread_id() -> ThreadId {
    ThreadId::UI
}

#[inline]
pub fn on_cef_ui_thread() -> bool {
    currently_on(ThreadId::UI) != 0
}

pub mod app;
pub mod input;

pub mod active;
pub(crate) mod client;
pub(crate) mod closeall;
pub(crate) mod context;
pub(crate) mod entity;
pub(crate) mod facet;
pub(crate) mod ffi;
pub(crate) mod focus;
pub(crate) mod frames;
pub(crate) mod hintsfeed;
pub(crate) mod lifecycle;
pub(crate) mod lookup;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod osr;
pub(crate) mod queue;
pub mod quit;
pub mod renderer;
pub mod shell;
pub mod stage;
pub mod titles;

use client::BrowserClient;
use entity::CefBrowserHandles;
use facet::DomFocusAncestorWalkMax;
use lifecycle::{BrowserCloseGuardsResource, BrowserLifecycleResource};
use osr::{CefAttach, CefRenderHandler, CefRenderInner, ForeignOsrIndex};
use queue::BrowserUiOpQueue;
use renderer::{
    ExtractedOsrBrowserIds, SharedGpu, VmuxOsrRenderGraphState, VmuxWindowsStoreResource,
};
use shell::OsrHostState;

// ---------------------------------------------------------------------------
// Bevy resources (mirrors runtime ffi statics)
// ---------------------------------------------------------------------------

/// WGPU device/queue shared with OSR; same `Arc` as [`crate::runtime::ffi_gpu`].
///
/// **macOS:** inserted after the first `winit` `resumed` (see [`init_browser_client_macos_after_first_window`]).
#[derive(Resource, Clone)]
pub struct GpuResource(pub Arc<SharedGpu>);

/// OSR foreign tab index; same `Arc` as [`crate::runtime::ffi_osr_index`].
#[derive(Resource, Clone)]
pub struct ForeignOsrIndexResource(pub Arc<ForeignOsrIndex>);

impl ForeignOsrIndexResource {
    pub fn inner(&self) -> &Arc<ForeignOsrIndex> {
        &self.0
    }
}

#[derive(Resource, Clone)]
pub struct DeviceScaleFactorResource(pub Arc<Mutex<f32>>);

pub fn set_device_scale_factor(dsf: f32) {
    if let Ok(mut g) = crate::runtime::ffi_device_scale_factor().lock() {
        *g = dsf.max(0.5);
    }
}

// ---------------------------------------------------------------------------
// Library load + pre-loop `cef::initialize`
// ---------------------------------------------------------------------------

/// Loaded CEF framework (browser process). Used by the `vmux` binary; not a stable public API.
pub type Library = ::cef::library_loader::LibraryLoader;

pub fn load_cef() -> Library {
    let exe = std::env::current_exe().unwrap();
    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        "load_cef: exe={}",
        exe.display()
    );
    let loader = ::cef::library_loader::LibraryLoader::new(&exe, false);
    assert!(
        loader.load(),
        "cef: failed to load Chromium Embedded Framework (bundle ../Frameworks or CEF_FRAMEWORK_PATH)"
    );
    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        "load_cef: framework load OK"
    );

    let _ = ::cef::api_hash(::cef::sys::CEF_API_VERSION_LAST, 0);

    loader
}

/// CEF + winit state after [`CefPlugin`] `Startup` applies [`CefBootstrapPending`] (adds [`CefAttach`]).
pub struct CefStartupState {
    pub cef_app: ::cef::App,
    pub client_holder: std::rc::Rc<std::cell::RefCell<Option<::cef::Client>>>,
    pub cef_attach: CefAttach,
    pub event_proxy: EventLoopProxy<AppUserEvent>,
}

/// Browser `main` inserts this with [`App::insert_non_send_resource`] before [`crate::VmuxPlugin`].
/// [`CefPlugin`] consumes it on `Startup` and inserts [`CefStartupState`].
pub struct CefBootstrapPending {
    pub cef_app: ::cef::App,
    pub client_holder: std::rc::Rc<std::cell::RefCell<Option<::cef::Client>>>,
    pub event_proxy: EventLoopProxy<AppUserEvent>,
}

fn vmux_cef_log_file_path() -> PathBuf {
    if let Ok(p) = std::env::var("VMUX_CEF_LOG") {
        let path = PathBuf::from(p.trim());
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(beside_app) = cef_log_beside_macos_app_bundle(&exe) {
            return beside_app;
        }
        if let Some(dir) = exe.parent() {
            return dir.join("debug.log");
        }
    }
    PathBuf::from("debug.log")
}

fn cef_log_beside_macos_app_bundle(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    if macos.file_name() != Some(OsStr::new("MacOS")) {
        return None;
    }
    let contents = macos.parent()?;
    let app = contents.parent()?;
    if app.extension() != Some(OsStr::new("app")) {
        return None;
    }
    app.parent().map(|d| d.join("debug.log"))
}

fn macos_bundled_cef_settings_paths(exe: &Path) -> Option<(String, String, Option<String>)> {
    let macos = exe.parent()?;
    if macos.file_name() != Some(OsStr::new("MacOS")) {
        return None;
    }
    let contents = macos.parent()?;
    let app = contents.parent()?;
    if app.extension() != Some(OsStr::new("app")) {
        return None;
    }
    let app = app.canonicalize().ok()?;
    let framework_dir = app.join("Contents/Frameworks/Chromium Embedded Framework.framework");
    let framework_dir = framework_dir.canonicalize().ok()?;
    let exe_stem = exe.file_name()?.to_str()?;
    let helper_exe = app.join(format!(
        "Contents/Frameworks/{exe_stem} Helper.app/Contents/MacOS/{exe_stem} Helper"
    ));
    let subprocess = helper_exe
        .canonicalize()
        .ok()
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned());
    Some((
        framework_dir.to_string_lossy().into_owned(),
        app.to_string_lossy().into_owned(),
        subprocess,
    ))
}

fn warn_macos_icudtl_if_bundle_incomplete() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(macos) = exe.parent() else {
        return;
    };
    if macos.file_name() != Some(OsStr::new("MacOS")) {
        return;
    }
    let Some(contents) = macos.parent() else {
        return;
    };
    let Some(app) = contents.parent() else {
        return;
    };
    if app.extension() != Some(OsStr::new("app")) {
        return;
    }
    let icu = app.join("Contents/Resources/icudtl.dat");
    if icu.is_file() {
        return;
    }
    let fw_icu =
        app.join("Contents/Frameworks/Chromium Embedded Framework.framework/Resources/icudtl.dat");
    bevy_log::warn!(
        target: "vmux",
        pid = std::process::id(),
        "cef: {} missing — copy from {} then re-launch, or re-run bundle-cef-app (it adds this file).",
        icu.display(),
        fw_icu.display()
    );
}

fn vmux_cef_fresh_profile_from_env() -> bool {
    std::env::var("VMUX_CEF_FRESH_PROFILE")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn vmux_user_home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

fn vmux_accept_language_list_from_lang(lang: Option<&str>) -> String {
    let raw = lang
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("en_US");
    let primary = raw
        .split('.')
        .next()
        .unwrap_or("en_US")
        .split('@')
        .next()
        .unwrap_or("en_US")
        .replace('_', "-");
    let primary = if primary.is_empty() {
        "en-US".to_string()
    } else {
        primary
    };
    let short = primary.split('-').next().unwrap_or("en").to_string();
    format!("{primary},{short}")
}

fn vmux_accept_language_list() -> String {
    vmux_accept_language_list_from_lang(std::env::var("LANG").ok().as_deref())
}

fn vmux_locale_from_accept_list(accept: &str) -> String {
    accept
        .split(',')
        .next()
        .unwrap_or("en-US")
        .trim()
        .to_string()
}

fn vmux_root_cache_path(home_dir: &str) -> String {
    if let Ok(p) = std::env::var("VMUX_CEF_ROOT_CACHE") {
        let t = p.trim();
        if !t.is_empty() {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                "cef: using VMUX_CEF_ROOT_CACHE={}",
                t
            );
            return t.to_string();
        }
    }
    let base = format!("{home_dir}/.local/share/vmux-cef");
    if vmux_cef_fresh_profile_from_env() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let path = format!("{base}-run-{ms}");
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "cef: VMUX_CEF_FRESH_PROFILE — root_cache_path={path}"
        );
        return path;
    }
    base
}

fn vmux_cef_disk_profile_path(home_dir: &str) -> String {
    Path::new(&vmux_root_cache_path(home_dir))
        .join("Default")
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn vmux_cef_disk_profile_abs_path() -> String {
    vmux_cef_disk_profile_path(&vmux_user_home_dir())
}

pub fn initialize_cef_after_execute(args: &::cef::args::Args, cef_app: &mut ::cef::App) {
    crate::log::record_startup_milestone("cef_initialize_enter");
    let home_dir = vmux_user_home_dir();
    let root_cache_path = vmux_root_cache_path(&home_dir);
    let disk_profile_path = vmux_cef_disk_profile_path(&home_dir);
    let _ = std::fs::create_dir_all(&root_cache_path);
    let _ = std::fs::create_dir_all(&disk_profile_path);
    let cef_log_path = vmux_cef_log_file_path();
    if let Some(parent) = cef_log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cef_log_str = cef_log_path.to_string_lossy().into_owned();
    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        "cef: log_file={}",
        cef_log_str
    );
    warn_macos_icudtl_if_bundle_incomplete();
    let accept_lang = vmux_accept_language_list();
    let locale = vmux_locale_from_accept_list(&accept_lang);
    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        "cef: disk_profile_path={} accept_language_list={} locale={}",
        disk_profile_path,
        accept_lang,
        locale
    );
    let mut settings = ::cef::Settings {
        no_sandbox: !cfg!(feature = "sandbox") as _,
        cache_path: ::cef::CefString::from(disk_profile_path.as_str()),
        root_cache_path: ::cef::CefString::from(root_cache_path.as_str()),
        persist_session_cookies: 1,
        accept_language_list: ::cef::CefString::from(accept_lang.as_str()),
        locale: ::cef::CefString::from(locale.as_str()),
        windowless_rendering_enabled: true as _,
        external_message_pump: true as _,
        log_file: ::cef::CefString::from(cef_log_str.as_str()),
        log_severity: ::cef::LogSeverity::INFO,
        ..Default::default()
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some((framework_dir, main_bundle, subprocess)) =
            macos_bundled_cef_settings_paths(&exe)
        {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                "cef: macOS explicit bundle paths — framework_dir_path={} main_bundle_path={} browser_subprocess_path={}",
                framework_dir,
                main_bundle,
                subprocess.as_deref().unwrap_or("(default)"),
            );
            settings.framework_dir_path = ::cef::CefString::from(framework_dir.as_str());
            settings.main_bundle_path = ::cef::CefString::from(main_bundle.as_str());
            if let Some(ref p) = subprocess {
                settings.browser_subprocess_path = ::cef::CefString::from(p.as_str());
            }
            let fw_resources = PathBuf::from(&framework_dir).join("Resources");
            if fw_resources.is_dir() {
                let fw_resources = fw_resources.canonicalize().unwrap_or(fw_resources);
                let res = fw_resources.to_string_lossy().into_owned();
                settings.resources_dir_path = ::cef::CefString::from(res.as_str());
                settings.locales_dir_path = ::cef::CefString::from(res.as_str());
                bevy_log::info!(
                    target: "vmux",
                    pid = std::process::id(),
                    "cef: resources_dir_path={} locales_dir_path={}",
                    res,
                    res,
                );
                let resources_pak = fw_resources.join("resources.pak");
                if !resources_pak.is_file() {
                    bevy_log::warn!(
                        target: "vmux",
                        pid = std::process::id(),
                        "cef: expected {} missing — CEF may fail loading packed resources",
                        resources_pak.display(),
                    );
                }
            } else {
                bevy_log::warn!(
                    target: "vmux",
                    pid = std::process::id(),
                    "cef: framework Resources directory missing at {}",
                    fw_resources.display(),
                );
            }
        }
    }
    crate::log::write_cef_log_path_pointer(&cef_log_path);
    let main_args = args.as_main_args();
    assert_eq!(
        ::cef::initialize(
            Some(main_args),
            Some(&settings),
            Some(cef_app),
            std::ptr::null_mut(),
        ),
        1
    );
    crate::log::record_startup_milestone("cef_initialize_ok");
}

// ---------------------------------------------------------------------------
// Client after first window (GPU from winit surface before CEF browser create)
// ---------------------------------------------------------------------------

pub(crate) fn init_browser_client_macos_after_first_window(
    client_cell: &std::cell::RefCell<Option<::cef::Client>>,
    cef_attach: CefAttach,
    window: Arc<winit::window::Window>,
) {
    if client_cell.borrow().is_some() {
        return;
    }
    let gpu = Arc::new(pollster::block_on(SharedGpu::new_with_window(window)));
    crate::runtime::register_gpu_only_for_ffi(gpu.clone());
    let osr_index = crate::runtime::ffi_osr_index();
    let dsf = crate::runtime::ffi_device_scale_factor();
    let render_inner = CefRenderInner {
        osr_index: osr_index.clone(),
        windows_attach: cef_attach.clone(),
        device: gpu.device.clone(),
        queue: gpu.queue.clone(),
        layout: gpu.texture_bind_group_layout.clone(),
        device_scale_factor: dsf.clone(),
        paint_redraw_throttle: osr_index.paint_redraw_throttle.clone(),
    };
    let rh = CefRenderHandler::build(render_inner);
    let client = BrowserClient::new(rh);
    *client_cell.borrow_mut() = Some(client);
}

// ---------------------------------------------------------------------------
// Startup systems (crate-internal)
// ---------------------------------------------------------------------------

fn cef_apply_bootstrap_to_startup_state(world: &mut World) {
    let Some(pending) = world.remove_non_send_resource::<CefBootstrapPending>() else {
        panic!(
            "vmux: insert_non_send_resource(CefBootstrapPending {{ ... }}) before VmuxPlugin (browser main)"
        );
    };
    let cef_attach = CefAttach::new();
    world.insert_non_send_resource(CefStartupState {
        cef_app: pending.cef_app,
        client_holder: pending.client_holder,
        cef_attach,
        event_proxy: pending.event_proxy,
    });
}

fn cef_startup_milestone_enter_system() {
    crate::log::record_startup_milestone("bevy_startup_enter");
}

fn cef_startup_register_browser_and_osr_host_system(world: &mut World) {
    let state = world.non_send_resource_mut::<CefStartupState>();
    let (client_holder, cef_attach) = (state.client_holder.clone(), state.cef_attach.clone());
    drop(state);

    let key_settings = world
        .get_resource::<crate::settings::SettingsResource>()
        .map(|s| s.0.clone())
        .unwrap_or_else(crate::settings::default_key_settings);
    world.insert_resource(DomFocusAncestorWalkMax(
        key_settings.dom_focus_ancestor_walk_max,
    ));
    let editable_queues = world.resource::<EditableFocusQueues>().clone();
    let browser_ui_ops = world.resource::<BrowserUiOpQueue>().clone();
    let cef_handles = world.resource::<CefBrowserHandles>().0.clone();
    let lifecycle = world.resource::<BrowserLifecycleResource>().0.clone();
    let close_guards = world.resource::<BrowserCloseGuardsResource>().0.clone();
    let windows_store = cef_attach.windows_store.clone();

    let client_cell = client_holder.as_ref();
    *client_cell.borrow_mut() = None;
    let osr_index = Arc::new(ForeignOsrIndex::default());
    let dsf = Arc::new(Mutex::new(1.0f32));
    crate::runtime::register_osr_index_and_scale_for_ffi(osr_index, dsf);
    ffi::register_browser_runtime_for_ffi(cef_handles, lifecycle, close_guards);
    ffi::register_cef_attach_for_ffi(Some(cef_attach.clone()));

    crate::log::record_startup_milestone("bevy_startup_client_ready");

    let osr_host = OsrHostState::new(
        client_holder,
        cef_attach,
        key_settings,
        editable_queues,
        browser_ui_ops,
    );
    world.insert_resource(VmuxWindowsStoreResource(windows_store));
    world.insert_resource(ForeignOsrIndexResource(crate::runtime::ffi_osr_index()));
    world.insert_resource(DeviceScaleFactorResource(
        crate::runtime::ffi_device_scale_factor(),
    ));
    world.insert_non_send_resource(osr_host);
}

// ---------------------------------------------------------------------------
// CefPlugin
// ---------------------------------------------------------------------------

pub struct CefPlugin;

impl Default for CefPlugin {
    fn default() -> Self {
        Self
    }
}

impl CefPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for CefPlugin {
    fn build(&self, app: &mut App) {
        // OSR compositor extract + render graph (Bevy) is part of the same CEF windowless stack.
        app.init_resource::<VmuxOsrRenderGraphState>()
            .init_resource::<ExtractedOsrBrowserIds>()
            .init_resource::<CefThreadContext>()
            .add_systems(
                Startup,
                (
                    cef_apply_bootstrap_to_startup_state,
                    cef_startup_milestone_enter_system,
                    cef_startup_register_browser_and_osr_host_system,
                )
                    .chain()
                    .after(crate::settings::VmuxSettingsStartup),
            );
    }
}

#[cfg(test)]
use bevy_ecs::prelude::{Res, ResMut};
#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;

#[cfg(test)]
#[derive(Resource)]
struct AcceptLangIn(Option<String>);

#[cfg(test)]
#[derive(Resource, Default)]
struct AcceptLangOut(String);

#[cfg(test)]
fn accept_lang_populate_system(inp: Res<AcceptLangIn>, mut out: ResMut<AcceptLangOut>) {
    out.0 = vmux_accept_language_list_from_lang(inp.0.as_deref());
}

#[cfg(test)]
#[test]
fn accept_language_parses_lang_utf8_suffix_via_system() {
    let mut world = World::default();
    world.insert_resource(AcceptLangIn(Some("en_CA.UTF-8".into())));
    world.insert_resource(AcceptLangOut::default());
    world.run_system_once(accept_lang_populate_system).unwrap();
    let s = &world.resource::<AcceptLangOut>().0;
    assert!(s.starts_with("en-CA,"), "got {s:?}");
}

#[cfg(test)]
#[test]
fn accept_language_default_when_missing_via_system() {
    let mut world = World::default();
    world.insert_resource(AcceptLangIn(None));
    world.insert_resource(AcceptLangOut::default());
    world.run_system_once(accept_lang_populate_system).unwrap();
    let s = &world.resource::<AcceptLangOut>().0;
    assert!(s.contains("en-US") || s.contains("en-us"), "got {s:?}");
}
