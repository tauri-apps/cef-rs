use cef::{
    self, BrowserProcessHandler, ImplBrowserProcessHandler, WrapBrowserProcessHandler, rc::Rc, *,
};
use cef::{ImplRequestContextHandler, RequestContextHandler, WrapRequestContextHandler};
use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};

fn update_texture_bind_group(device: &wgpu::Device, texture: &wgpu::Texture) {
    static BINDGROUP_UPDATED: std::sync::Once = std::sync::Once::new();
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });

    let texture_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Cef Texture Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Cef Texture Bind Group"),
        layout: &texture_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture.create_view(
                    &wgpu::TextureViewDescriptor {
                        label: Some("Cef Texture View"),
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

    if let Ok(mut slot) = texture_slot().lock() {
        slot.replace(bind_group);
        BINDGROUP_UPDATED.call_once(|| {
            eprintln!("cef-osr-dawn texture bind group updated");
        });
    }
}


#[derive(Clone)]
pub struct OsrApp {}

impl OsrApp {
    pub fn new() -> Self {
        Self {}
    }
}

wrap_app! {
    pub(crate) struct AppBuilder {
        app: OsrApp,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&cef::CefStringUtf16>,
            command_line: Option<&mut cef::CommandLine>,
        ) {
            let Some(command_line) = command_line else {
                return;
            };

            command_line.append_switch(Some(&"no-startup-window".into()));
            command_line.append_switch(Some(&"noerrdialogs".into()));
            command_line.append_switch(Some(&"hide-crash-restore-bubble".into()));
            command_line.append_switch(Some(&"use-mock-keychain".into()));
            command_line.append_switch(Some(&"enable-logging=stderr".into()));
            command_line.append_switch_with_value(
                Some(&"remote-debugging-port".into()),
                Some(&"9229".into()),
            );
        }

        fn browser_process_handler(&self) -> Option<cef::BrowserProcessHandler> {
            Some(BrowserProcessHandlerBuilder::build(
                OsrBrowserProcessHandler::new(),
            ))
        }
    }
}

impl AppBuilder {
    pub(crate) fn build(app: OsrApp) -> cef::App {
        Self::new(app)
    }
}

#[derive(Clone)]
pub struct OsrBrowserProcessHandler {
    is_cef_ready: RefCell<bool>,
}

impl OsrBrowserProcessHandler {
    pub fn new() -> Self {
        Self {
            is_cef_ready: RefCell::new(false),
        }
    }
}

wrap_browser_process_handler! {
    pub(crate) struct BrowserProcessHandlerBuilder {
        handler: OsrBrowserProcessHandler,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            *self.handler.is_cef_ready.borrow_mut() = true;
        }

        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            let Some(command_line) = command_line else {
                return;
            };

            command_line.append_switch(Some(&"disable-web-security".into()));
            command_line.append_switch(Some(&"allow-running-insecure-content".into()));
            command_line.append_switch(Some(&"disable-session-crashed-bubble".into()));
            command_line.append_switch(Some(&"ignore-certificate-errors".into()));
            command_line.append_switch(Some(&"ignore-ssl-errors".into()));
            command_line.append_switch(Some(&"enable-logging=stderr".into()));
        }
    }
}

impl BrowserProcessHandlerBuilder {
    pub(crate) fn build(handler: OsrBrowserProcessHandler) -> BrowserProcessHandler {
        Self::new(handler)
    }
}

#[derive(Clone)]
pub struct OsrRenderHandler {
    device_scale_factor: f32,
    size: std::rc::Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
    _device: wgpu::Device,
    _queue: wgpu::Queue,
}

impl OsrRenderHandler {
    pub fn new(
        _device: wgpu::Device,
        _queue: wgpu::Queue,
        device_scale_factor: f32,
        size: winit::dpi::LogicalSize<f32>,
    ) -> (Self, std::rc::Rc<RefCell<winit::dpi::LogicalSize<f32>>>) {
        let size = std::rc::Rc::new(RefCell::new(size));
        (
            Self {
                size: size.clone(),
                device_scale_factor,
                _device,
                _queue,
            },
            size,
        )
    }
}

wrap_render_handler! {
    pub struct RenderHandlerBuilder {
        handler: OsrRenderHandler,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let size = self.handler.size.borrow();
                // size must be non-zero
                if size.width > 0.0 && size.height > 0.0 {
                    rect.width = size.width as _;
                    rect.height = size.height as _;
                }
            }
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> ::std::os::raw::c_int {
            if let Some(screen_info) = screen_info {
                screen_info.device_scale_factor = self.handler.device_scale_factor;
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

        #[cfg(all(
            any(target_os = "macos", target_os = "windows", target_os = "linux"),
            feature = "accelerated_osr"
        ))]
        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            static ON_ACCEL_LOGGED: std::sync::Once = std::sync::Once::new();
            let Some(info) = info else { return };
            ON_ACCEL_LOGGED.call_once(|| {
                eprintln!(
                    "cef-osr-dawn on_accelerated_paint called: type={:?}, coded_size={}x{}, thread={:?}",
                    type_,
                    info.extra.coded_size.width,
                    info.extra.coded_size.height,
                    std::thread::current().id()
                );
            });
            if type_ != PaintElementType::default() {
                return;
            }

            let src_texture = {
                use cef::osr_texture_import::shared_texture_handle::SharedTextureHandle;
                let shared_handle = SharedTextureHandle::new(info);
                if let SharedTextureHandle::Unsupported = shared_handle {
                    eprintln!("Platform does not support accelerated painting");
                    return;
                }
                match shared_handle.import_texture(&self.handler._device) {
                    Ok(texture) => texture,
                    Err(e) => {
                        eprintln!("Failed to import shared texture: {:?}", e);
                        return;
                    }
                }
            };
            update_texture_bind_group(&self.handler._device, &src_texture);
        }

        #[cfg(all(
            any(target_os = "macos", target_os = "windows", target_os = "linux"),
            feature = "accelerated_osr"
        ))]
        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            _type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            use wgpu::{Extent3d, TextureDescriptor, TextureDimension, TextureUsages};
            static ON_PAINT_LOGGED: std::sync::Once = std::sync::Once::new();

            if buffer.is_null() || width <= 0 || height <= 0 {
                return;
            }
            ON_PAINT_LOGGED.call_once(|| {
                eprintln!(
                    "cef-osr-dawn on_paint active: {}x{}, thread={:?}",
                    width,
                    height,
                    std::thread::current().id()
                );
            });

            let buffer_size = (width * height * 4) as usize; // BGRA format
            let buffer_slice = unsafe { std::slice::from_raw_parts(buffer, buffer_size) };

            // Create texture from CEF paint buffer
            let texture_desc = TextureDescriptor {
                label: Some("CEF Paint Texture"),
                size: Extent3d {
                    width: width as u32,
                    height: height as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            };

            let texture = self.handler._device.create_texture(&texture_desc);

            // Upload the CEF buffer data to the texture
            self.handler._queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                buffer_slice,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * width as u32),
                    rows_per_image: Some(height as u32),
                },
                texture_desc.size,
            );

            update_texture_bind_group(&self.handler._device, &texture);
        }
    }
}

impl RenderHandlerBuilder {
    pub fn build(handler: OsrRenderHandler) -> RenderHandler {
        Self::new(handler)
    }
}

static TEXTURE: OnceLock<Mutex<Option<wgpu::BindGroup>>> = OnceLock::new();

pub(crate) fn texture_slot() -> &'static Mutex<Option<wgpu::BindGroup>> {
    TEXTURE.get_or_init(|| Mutex::new(None))
}

wrap_client! {
    pub(crate) struct ClientBuilder {
        render_handler: RenderHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<cef::RenderHandler> {
            Some(self.render_handler.clone())
        }
    }
}

impl ClientBuilder {
    pub(crate) fn build(render_handler: OsrRenderHandler) -> Client {
        Self::new(RenderHandlerBuilder::build(render_handler))
    }
}

#[derive(Clone)]
pub struct OsrRequestContextHandler {}

wrap_request_context_handler! {
    pub(crate) struct RequestContextHandlerBuilder {
        handler: OsrRequestContextHandler,
    }

    impl RequestContextHandler {}
}

impl RequestContextHandlerBuilder {
    pub(crate) fn build(handler: OsrRequestContextHandler) -> RequestContextHandler {
        Self::new(handler)
    }
}
