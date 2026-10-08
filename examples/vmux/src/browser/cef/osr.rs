//! CEF off-screen rendering: window hub, foreign tab index, view geometry, [`CefRenderHandler`].
//!
//! Hub, foreign index, geometry helpers, and the render handler share this module.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use winit::dpi::LogicalSize;
use winit::window::WindowId;

use crate::browser::cef::renderer::WindowSurface;

use ::cef::*;

/// One vmux window: winit + wgpu surface and its CEF browser (after `on_after_created`).
pub struct WindowEntry {
    pub surface: WindowSurface,
    pub browser: Browser,
    pub size: Arc<Mutex<LogicalSize<f32>>>,
}

/// Queued in the main loop before async `browser_host_create_browser`; matched in `on_after_created`.
pub struct PendingCefWindow {
    pub surface: WindowSurface,
    pub logical: LogicalSize<f32>,
    /// Initial navigation URL passed to `browser_host_create_browser`.
    pub url: String,
    /// When set, after the first load completes we navigate to this URL (empty-config → default URL).
    pub deferred_url: Option<String>,
}

/// Shared between [`OsrHostState`](crate::browser::cef::shell::OsrHostState) (enqueue window + pump) and `LifeSpanHandler::on_after_created` (attach browser).
#[derive(Clone)]
pub struct CefAttach {
    pub shell_fifo: Arc<Mutex<VecDeque<PendingCefWindow>>>,
    pub windows_store: Arc<Mutex<HashMap<WindowId, WindowEntry>>>,
    /// Winit shells created for OSR that are not yet paired in `on_after_created` (spawn → +1, attach → -1).
    /// Prevents `on_before_close` from shutting down when the only live browser closes while others
    /// are still queued or in flight.
    pub unpaired_cef_shells: Arc<AtomicUsize>,
    /// `browser_id` → URL to load after the initial blank load finishes (empty startup only).
    pub deferred_url_after_blank: Arc<Mutex<HashMap<i32, String>>>,
}

impl CefAttach {
    pub fn new() -> Self {
        Self {
            shell_fifo: Arc::new(Mutex::new(VecDeque::new())),
            windows_store: Arc::new(Mutex::new(HashMap::new())),
            unpaired_cef_shells: Arc::new(AtomicUsize::new(0)),
            deferred_url_after_blank: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// Plain OSR tab index for CEF / winit paths that cannot hold `Res` / `&World`.
// Updated from Bevy `apply_*` systems when browsers attach or detach; read from
// [`crate::runtime::ffi_osr_index`] in `RenderHandler` and shell code.
//
// Tab identity for app logic lives in ECS ([`crate::browser::cef::entity`]); this structure
// only holds what synchronous CEF callbacks need (sizes, bind groups, id→window).
//
// **Spike note:** cef-rs does not expose per-`Browser` user-data hooks usable from `RenderHandler`
// to stash an `Arc` of tab GPU state (unlike e.g. V8 `set_user_data`). So we keep this minimal
// mutex-backed index (plan “B”) rather than a global id map hidden behind `CefHub` methods.

pub struct TabPaintSlot {
    pub size: Arc<Mutex<LogicalSize<f32>>>,
    /// Same `Arc` as [`crate::browser::cef::entity::OsrPaintBindGroup`] for this tab.
    pub bind_group: Arc<Mutex<Option<wgpu::BindGroup>>>,
}

/// Handles created in [`register_tab`], shared with Bevy [`crate::browser::cef::entity::BrowserBundle`].
pub struct OsrTabShared {
    pub view_logical_size: Arc<Mutex<LogicalSize<f32>>>,
    pub paint_bind_group: Arc<Mutex<Option<wgpu::BindGroup>>>,
}

pub struct ForeignOsrIndex {
    pub tabs: Mutex<HashMap<i32, TabPaintSlot>>,
    pub browser_to_window: Mutex<HashMap<i32, WindowId>>,
    pub paint_redraw_throttle: Arc<Mutex<HashMap<i32, Instant>>>,
    /// Until [`register_tab`] runs, CEF may call `view_rect` / `screen_info` with no tab entry.
    /// `examples/osr` avoids this with a RefCell size on the handler; vmux stages the pending shell’s
    /// logical size here from [`crate::browser::cef::shell::OsrHostState::finish_next_pending_browser_if_any`]
    /// until attach (single-window MVP).
    pub pre_attach_view_logical: Mutex<Option<LogicalSize<f32>>>,
}

impl Default for ForeignOsrIndex {
    fn default() -> Self {
        Self {
            tabs: Mutex::new(HashMap::new()),
            browser_to_window: Mutex::new(HashMap::new()),
            paint_redraw_throttle: Arc::new(Mutex::new(HashMap::new())),
            pre_attach_view_logical: Mutex::new(None),
        }
    }
}

pub fn register_tab(
    index: &ForeignOsrIndex,
    browser_id: i32,
    window_id: WindowId,
    logical_size: LogicalSize<f32>,
) -> OsrTabShared {
    let view_logical_size = Arc::new(Mutex::new(logical_size));
    let paint_bind_group = Arc::new(Mutex::new(None));
    index.tabs.lock().unwrap_or_else(|e| e.into_inner()).insert(
        browser_id,
        TabPaintSlot {
            size: view_logical_size.clone(),
            bind_group: paint_bind_group.clone(),
        },
    );
    index
        .browser_to_window
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(browser_id, window_id);
    if let Ok(mut p) = index.pre_attach_view_logical.lock() {
        p.take();
    }
    OsrTabShared {
        view_logical_size,
        paint_bind_group,
    }
}

pub fn unregister_tab(index: &ForeignOsrIndex, browser_id: i32) {
    index
        .tabs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&browser_id);
    index
        .browser_to_window
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&browser_id);
    reset_paint_redraw_throttle(index, browser_id);
}

pub fn window_id_for_browser(index: &ForeignOsrIndex, browser_id: i32) -> Option<WindowId> {
    index
        .browser_to_window
        .lock()
        .ok()
        .and_then(|m| m.get(&browser_id).copied())
}

pub fn browser_window_pairs(index: &ForeignOsrIndex) -> Vec<(i32, WindowId)> {
    index
        .browser_to_window
        .lock()
        .map(|m| {
            m.iter()
                .map(|(&browser_id, &window_id)| (browser_id, window_id))
                .collect()
        })
        .unwrap_or_default()
}

pub fn with_tab<R>(
    index: &ForeignOsrIndex,
    browser_id: i32,
    f: impl FnOnce(&TabPaintSlot) -> R,
) -> Option<R> {
    let map = index.tabs.lock().ok()?;
    let slot = map.get(&browser_id)?;
    Some(f(slot))
}

pub fn replace_bind_group(index: &ForeignOsrIndex, browser_id: i32, bind_group: wgpu::BindGroup) {
    if let Ok(map) = index.tabs.lock() {
        if let Some(slot) = map.get(&browser_id) {
            if let Ok(mut g) = slot.bind_group.lock() {
                *g = Some(bind_group);
            }
        }
    }
}

pub fn with_bind_group<R>(
    index: &ForeignOsrIndex,
    browser_id: i32,
    f: impl FnOnce(&wgpu::BindGroup) -> R,
) -> Option<R> {
    let map = index.tabs.lock().ok()?;
    let slot = map.get(&browser_id)?;
    let g = slot.bind_group.lock().ok()?;
    g.as_ref().map(f)
}

pub fn reset_paint_redraw_throttle(index: &ForeignOsrIndex, browser_id: i32) {
    let _ = index
        .paint_redraw_throttle
        .lock()
        .ok()
        .map(|mut m| m.remove(&browser_id));
}

// View sizing for CEF OSR [`RenderHandler`] (`view_rect` / `screen_info`).
// Resolves the shared logical-size [`Arc`] via [`ForeignOsrIndex`]; geometry is implemented on
// [`crate::browser::cef::entity::OsrViewLogicalSize`] for Bevy ECS parity.

use crate::browser::cef::entity::OsrViewLogicalSize;

fn fill_view_rect_from_logical(sz: LogicalSize<f32>, rect: &mut Rect) {
    rect.x = 0;
    rect.y = 0;
    rect.width = sz.width.max(1.0).ceil() as i32;
    rect.height = sz.height.max(1.0).ceil() as i32;
}

fn fill_screen_info_from_logical(
    sz: LogicalSize<f32>,
    device_scale_factor: f32,
    screen_info: &mut ScreenInfo,
) {
    let w_px = (sz.width.max(1.0) * device_scale_factor).ceil() as i32;
    let h_px = (sz.height.max(1.0) * device_scale_factor).ceil() as i32;
    screen_info.rect.x = 0;
    screen_info.rect.y = 0;
    screen_info.rect.width = w_px;
    screen_info.rect.height = h_px;
    screen_info.available_rect = screen_info.rect.clone();
}

pub fn apply_view_rect(index: &ForeignOsrIndex, browser: &Browser, rect: &mut Rect) {
    let id = browser.identifier();
    if with_tab(index, id, |slot| {
        OsrViewLogicalSize::fill_cef_view_rect_from_arc(&slot.size, rect);
    })
    .is_some()
    {
        return;
    }
    if let Ok(g) = index.pre_attach_view_logical.lock() {
        if let Some(sz) = g.as_ref() {
            fill_view_rect_from_logical(*sz, rect);
        }
    }
}

pub fn apply_screen_info(
    index: &ForeignOsrIndex,
    browser: Option<&Browser>,
    device_scale_factor: f32,
    screen_info: &mut ScreenInfo,
) {
    screen_info.device_scale_factor = device_scale_factor;
    let Some(browser) = browser else {
        return;
    };
    let id = browser.identifier();
    if with_tab(index, id, |slot| {
        OsrViewLogicalSize::fill_cef_screen_rect_from_arc(
            &slot.size,
            device_scale_factor,
            screen_info,
        );
    })
    .is_some()
    {
        return;
    }
    if let Ok(g) = index.pre_attach_view_logical.lock() {
        if let Some(sz) = g.as_ref() {
            fill_screen_info_from_logical(*sz, device_scale_factor, screen_info);
        }
    }
}

// CEF [`RenderHandler`] implementation for OSR. [`CefRenderInner`] holds the **same** `Arc`
// handles as Bevy [`Resource`]s / OSR host state: CEF callbacks have no [`bevy_ecs::world::World`], so
// GPU + tab + scale + host data are shared by reference with [`crate::browser::cef`] resources
// (`GpuResource`, [`ForeignOsrIndexResource`](crate::browser::cef::ForeignOsrIndexResource),
// [`DeviceScaleFactorResource`](crate::browser::cef::DeviceScaleFactorResource), `OsrHostState`’s
// [`CefAttach`](CefAttach)). This is the same “final state bridge” pattern as
// [`crate::browser::cef::entity::OsrViewLogicalSize`] ↔ [`crate::browser::cef::osr`].

use crate::browser::cef::renderer::{
    CEF_VIEW_TEXTURE_BASE_FORMAT_BGRA, CEF_VIEW_TEXTURE_FORMATS_BGRA,
    CEF_VIEW_TEXTURE_VIEW_FORMAT_BGRA,
};

static PAINT_DEBUG_TICK: AtomicUsize = AtomicUsize::new(0);
/// One log line proving CEF delivered pixels and we requested a winit present (software or accel).
static FIRST_PROOF_OSR_PRESENT: AtomicBool = AtomicBool::new(false);

/// Shared render state for [`CefRenderHandler`]: every field is an `Arc` (or [`CefAttach`]) also held
/// on the Bevy side so ECS systems and CEF paint/view callbacks see one store.
#[derive(Clone)]
pub struct CefRenderInner {
    /// Same as [`ForeignOsrIndexResource`](crate::browser::cef::ForeignOsrIndexResource) / [`crate::runtime::ffi_osr_index`].
    pub osr_index: Arc<ForeignOsrIndex>,
    /// Same [`CefAttach`] as [`CefStartupState`](crate::browser::cef::CefStartupState) / OSR host.
    pub windows_attach: CefAttach,
    /// From [`SharedGpu`](crate::browser::cef::renderer::SharedGpu); same as [`GpuResource`](crate::browser::cef::GpuResource).
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub layout: Arc<wgpu::BindGroupLayout>,
    /// Same `Arc` as [`DeviceScaleFactorResource`](crate::browser::cef::DeviceScaleFactorResource).
    pub device_scale_factor: Arc<Mutex<f32>>,
    /// Same `Arc` as [`ForeignOsrIndex::paint_redraw_throttle`](ForeignOsrIndex::paint_redraw_throttle).
    /// Throttle `request_redraw` after each paint so we do not spin: begin_frame → paint → redraw.
    pub paint_redraw_throttle: Arc<Mutex<HashMap<i32, Instant>>>,
}

impl CefRenderInner {
    /// After a new texture/bind group is uploaded, schedule a winit present.
    ///
    /// Always call `request_redraw`: `replace_bind_group` already updated the texture; throttling
    /// redraws here used to skip `request_redraw` while still swapping bind groups, so the window
    /// kept presenting stale pixels (common after history navigation + several accelerated paints).
    fn request_redraw_after_new_texture(&self, browser_id: i32) {
        if let Ok(mut m) = self.paint_redraw_throttle.lock() {
            m.insert(browser_id, Instant::now());
        }
        let Some(wid) = window_id_for_browser(self.osr_index.as_ref(), browser_id) else {
            return;
        };
        let Ok(windows) = self.windows_attach.windows_store.lock() else {
            return;
        };
        if let Some(entry) = windows.get(&wid) {
            entry.surface.window.request_redraw();
        }
    }
}

wrap_render_handler! {
    pub struct CefRenderHandler {
        inner: CefRenderInner,
    }

    impl RenderHandler {
        fn view_rect(&self, browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            let Some(rect) = rect else { return };
            let Some(browser) = browser else { return };
            apply_view_rect(self.inner.osr_index.as_ref(), browser, rect);
        }

        fn screen_info(
            &self,
            browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> ::std::os::raw::c_int {
            if let Some(screen_info) = screen_info {
                let dsf = self
                    .inner
                    .device_scale_factor
                    .lock()
                    .map(|v| *v)
                    .unwrap_or(1.0);
                apply_screen_info(
                    self.inner.osr_index.as_ref(),
                    browser.map(|b| &*b),
                    dsf,
                    screen_info,
                );
                return true as _;
            }
            false as _
        }

        fn screen_point(
            &self,
            _browser: Option<&mut Browser>,
            _view_x: ::std::os::raw::c_int,
            _view_y: ::std::os::raw::c_int,
            _screen_x: Option<&mut ::std::os::raw::c_int>,
            _screen_y: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            false as _
        }

        // Bind group + texture must be committed before this callback returns; deferring only
        // `request_redraw` would still require synchronous GPU upload here (CEF/OSR contract).
        #[cfg(feature = "accelerated_osr")]
        fn on_accelerated_paint(
            &self,
            browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            let Some(info) = info else { return };
            let Some(browser) = browser else { return };
            if type_ != PaintElementType::default() {
                return;
            }
            let id = browser.identifier();
            use ::cef::osr_texture_import::shared_texture_handle::SharedTextureHandle;
            let shared_handle = SharedTextureHandle::new(info);
            if let SharedTextureHandle::Unsupported = shared_handle {
                return;
            }
            let src_texture = match shared_handle.import_texture(self.inner.device.as_ref()) {
                Ok(t) => t,
                Err(_) => return,
            };
            // Upstream `cef/` imports IOSurface as UNORM. For correct web colors, copy into a
            // staging UNORM texture that supports an sRGB *view*, and sample that view.
            let w = info.extra.coded_size.width.max(1) as u32;
            let h = info.extra.coded_size.height.max(1) as u32;
            let staging = self.inner.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vmux-osr accel staging"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: CEF_VIEW_TEXTURE_BASE_FORMAT_BGRA,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &CEF_VIEW_TEXTURE_FORMATS_BGRA,
            });
            let mut encoder = self
                .inner
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("vmux-osr accel copy"),
                });
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &src_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &staging,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            self.inner.queue.submit(std::iter::once(encoder.finish()));
            let sampler = self.inner.device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            });
            let bind_group = self.inner.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vmux-osr accel"),
                layout: self.inner.layout.as_ref(),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&staging.create_view(
                            &wgpu::TextureViewDescriptor {
                                format: Some(CEF_VIEW_TEXTURE_VIEW_FORMAT_BGRA),
                                ..Default::default()
                            },
                        )),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            replace_bind_group(self.inner.osr_index.as_ref(), id, bind_group);
            self.inner.request_redraw_after_new_texture(id);
            if !FIRST_PROOF_OSR_PRESENT.swap(true, Ordering::AcqRel) {
                bevy_log::info!(
                    target: "vmux",
                    pid = std::process::id(),
                    "proof: first_osr_frame_ready accelerated_osr browser_id={} {}x{} (texture → redraw requested)",
                    id,
                    w,
                    h
                );
            }
        }

        fn on_paint(
            &self,
            browser: Option<&mut Browser>,
            _type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            let Some(browser) = browser else { return };
            use wgpu::{Extent3d, TextureDescriptor, TextureDimension, TextureUsages};
            if buffer.is_null() || width <= 0 || height <= 0 {
                return;
            }
            let id = browser.identifier();
            let buffer_size = (width * height * 4) as usize;
            let buffer_slice = unsafe { std::slice::from_raw_parts(buffer, buffer_size) };

            if !FIRST_PROOF_OSR_PRESENT.swap(true, Ordering::AcqRel) {
                bevy_log::info!(
                    target: "vmux",
                    pid = std::process::id(),
                    "proof: first_osr_frame_ready software_osr browser_id={} pixels={}x{} (buffer → texture → redraw requested)",
                    id,
                    width,
                    height
                );
            }

            let (debug_paint, force_opaque) = if cfg!(debug_assertions) {
                (
                    std::env::var_os("VMUX_CEF_DEBUG_PAINT").is_some(),
                    std::env::var_os("VMUX_CEF_FORCE_OPAQUE").is_some(),
                )
            } else {
                (false, false)
            };
            let mut owned;
            let upload_bytes: &[u8] = if debug_paint || force_opaque {
                owned = buffer_slice.to_vec();
                if force_opaque {
                    for a in owned.iter_mut().skip(3).step_by(4) {
                        *a = 255;
                    }
                }
                &owned
            } else {
                buffer_slice
            };

            if debug_paint {
                // Throttle: log ~1 line per 120 paints per process.
                let tick = PAINT_DEBUG_TICK.fetch_add(1, Ordering::Relaxed);
                if tick % 120 == 0 {
                    let mut a_min: u8 = 255;
                    let mut a_max: u8 = 0;
                    // Sample every ~32px to keep this cheap.
                    let step_px = 32usize;
                    let w = width as usize;
                    let h = height as usize;
                    for y in (0..h).step_by(step_px) {
                        for x in (0..w).step_by(step_px) {
                            let i = (y * w + x) * 4 + 3;
                            let a = upload_bytes.get(i).copied().unwrap_or(255);
                            a_min = a_min.min(a);
                            a_max = a_max.max(a);
                        }
                    }
                    let cx = (w / 2).min(w.saturating_sub(1));
                    let cy = (h / 2).min(h.saturating_sub(1));
                    let ci = (cy * w + cx) * 4;
                    let b = upload_bytes.get(ci).copied().unwrap_or(0);
                    let g = upload_bytes.get(ci + 1).copied().unwrap_or(0);
                    let r = upload_bytes.get(ci + 2).copied().unwrap_or(0);
                    let a = upload_bytes.get(ci + 3).copied().unwrap_or(255);
                    println!(
                        "[vmux-osr paint pid={} id={}] {}x{} alpha[min={}, max={}] center[bgra]=[{},{},{},{}] force_opaque={}",
                        std::process::id(),
                        id,
                        width,
                        height,
                        a_min,
                        a_max,
                        b,
                        g,
                        r,
                        a,
                        force_opaque
                    );
                }
            }

            let texture_desc = TextureDescriptor {
                label: Some("vmux-osr paint"),
                size: Extent3d {
                    width: width as u32,
                    height: height as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                // Upload as UNORM; sample through sRGB view for correct web colors.
                format: CEF_VIEW_TEXTURE_BASE_FORMAT_BGRA,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &CEF_VIEW_TEXTURE_FORMATS_BGRA,
            };
            let texture = self.inner.device.create_texture(&texture_desc);
            self.inner.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                upload_bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * width as u32),
                    rows_per_image: Some(height as u32),
                },
                texture_desc.size,
            );
            let sampler = self.inner.device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            });
            let bind_group = self.inner.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vmux-osr paint bg"),
                layout: self.inner.layout.as_ref(),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture.create_view(
                            &wgpu::TextureViewDescriptor {
                                format: Some(CEF_VIEW_TEXTURE_VIEW_FORMAT_BGRA),
                                ..Default::default()
                            },
                        )),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            replace_bind_group(self.inner.osr_index.as_ref(), id, bind_group);
            self.inner.request_redraw_after_new_texture(id);
        }
    }
}

impl CefRenderHandler {
    pub fn build(inner: CefRenderInner) -> RenderHandler {
        Self::new(inner)
    }
}
