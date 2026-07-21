mod webrender;

use cef::{args::Args, *};
use std::{cell::RefCell, process::ExitCode, sync::Arc, thread::sleep, time::Duration};
use wgpu::{Backends, CurrentSurfaceTexture, util::DeviceExt};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::pump_events::{EventLoopExtPumpEvents, PumpStatus},
    window::{Window, WindowAttributes, WindowId},
};

use crate::webrender::{
    ClientBuilder, OsrApp, OsrRenderHandler, OsrRequestContextHandler,
    RequestContextHandlerBuilder, TEXTURE,
};

struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    pipeline: wgpu::RenderPipeline,
    queue: wgpu::Queue,
    size: winit::dpi::PhysicalSize<u32>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    quad: Geometry,
}

impl State {
    async fn new(window: Arc<Window>) -> State {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            #[cfg(target_os = "windows")]
            backends: Backends::from_comma_list("dx12"),
            #[cfg(target_os = "macos")]
            backends: Backends::from_comma_list("metal"),
            #[cfg(target_os = "linux")]
            backends: Backends::from_comma_list("vulkan"),
            //flags: wgpu::InstanceFlags::debugging(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                ..Default::default()
            })
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_non_sampler_bindings: 2048,
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();

        let size = window.inner_size();

        let surface = instance.create_surface(window.clone()).unwrap();
        let surface_format = wgpu::TextureFormat::Bgra8Unorm;
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Cef Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Cef Pipeline Layout"),
                bind_group_layouts: &[Some(&texture_bind_group_layout)],
                immediate_size: 0,
            });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Cef Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(Vertex::desc())],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent::OVER,
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });
        let quad = Geometry::new(&device);

        let state = State {
            window,
            pipeline,
            device,
            queue,
            size,
            surface,
            surface_format,
            quad,
        };

        state.configure_surface();

        state
    }

    fn get_window(&self) -> &Window {
        &self.window
    }

    fn configure_surface(&self) {
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.surface_format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![self.surface_format],
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            width: self.size.width,
            height: self.size.height,
            desired_maximum_frame_latency: 2,
            present_mode: wgpu::PresentMode::AutoVsync,
        };
        self.surface.configure(&self.device, &surface_config);
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.configure_surface();
        }
    }

    fn render(&mut self) {
        let surface_texture = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(success) => success,
            CurrentSurfaceTexture::Suboptimal(suboptimal) => {
                self.configure_surface();
                suboptimal
            }
            _ => return,
        };

        let frame = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                label: Some("Surface"),
                format: Some(wgpu::TextureFormat::Bgra8Unorm),
                ..Default::default()
            });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });
        TEXTURE.with_borrow_mut(|textures| {
            let Some(bind_group) = textures.as_ref() else {
                return;
            };
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Cef Render Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &frame,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    ..Default::default()
                });
                render_pass.set_pipeline(&self.pipeline);
                render_pass.set_bind_group(0, bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.quad.vertex_buffer.slice(..));
                render_pass.draw(0..self.quad.vertex_count, 0..1);
            }
            self.queue.submit(std::iter::once(encoder.finish()));
        });

        self.window.pre_present_notify();
        self.queue.present(surface_texture);
    }
}

struct App {
    state: Option<State>,
    browser: Option<Browser>,
}

struct Browser {
    browser: cef::Browser,
    size: std::rc::Rc<RefCell<winit::dpi::LogicalSize<f32>>>,
}

impl App {
    fn new() -> Self {
        App {
            state: None,
            browser: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(WindowAttributes::default())
                .unwrap(),
        );

        let state = pollster::block_on(State::new(window.clone()));
        self.state = Some(state);
        let accelerated_osr = cfg!(all(
            any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux"
            ),
            feature = "accelerated_osr"
        ));
        let window_info = WindowInfo {
            windowless_rendering_enabled: true as _,
            shared_texture_enabled: accelerated_osr as _,
            external_begin_frame_enabled: accelerated_osr as _,
            ..Default::default()
        };

        let device_scale_factor = window.scale_factor();
        let (render_handler, browser_size) = OsrRenderHandler::new(
            self.state.as_ref().unwrap().device.clone(),
            self.state.as_ref().unwrap().queue.clone(),
            device_scale_factor as _,
            window.inner_size().to_logical(device_scale_factor),
        );

        let browser_settings = BrowserSettings {
            windowless_frame_rate: 60,
            ..Default::default()
        };
        let mut context = cef::request_context_create_context(
            Some(&RequestContextSettings::default()),
            Some(&mut RequestContextHandlerBuilder::build(
                OsrRequestContextHandler {},
            )),
        );

        let browser = cef::browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut ClientBuilder::build(render_handler)),
            Some(&"https:://github.com".into()),
            Some(&browser_settings),
            None,
            context.as_mut(),
        );
        assert!(browser.is_some());

        self.browser.replace(Browser {
            browser: browser.unwrap(),
            size: browser_size,
        });

        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let state = self.state.as_mut().unwrap();
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                if let Some(host) = self.browser.as_mut().and_then(|b| b.browser.host()) {
                    host.send_external_begin_frame();
                }
                state.render();
                state.get_window().request_redraw();
            }
            WindowEvent::Resized(size) => {
                state.resize(size);
                if let Some(browser) = self.browser.as_mut() {
                    *browser.size.borrow_mut() =
                        size.to_logical(self.state.as_ref().unwrap().get_window().scale_factor());
                    if let Some(host) = self.browser.as_mut().and_then(|b| b.browser.host()) {
                        host.was_resized();
                    }
                }
            }
            _ => (),
        }
    }
}

fn main() -> std::process::ExitCode {
    #[cfg(all(target_os = "windows", debug_assertions))]
    pix::load_winpix_gpu_capturer().unwrap();

    #[cfg(target_os = "macos")]
    let _loader = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), false);
        assert!(loader.load());
        loader
    };

    env_logger::init();

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let cmd = args.as_cmd_line().unwrap();

    let switch = CefString::from("type");
    let is_browser_process = cmd.has_switch(Some(&switch)) != 1;
    let mut app = webrender::AppBuilder::build(OsrApp::new());
    let ret = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );

    if is_browser_process {
        assert!(ret == -1, "cannot execute browser process");
    } else {
        let process_type = CefString::from(&cmd.switch_value(Some(&switch)));
        println!("launch process {process_type}");
        assert!(ret >= 0, "cannot execute non-browser process");
        // non-browser process does not initialize cef
        return 0.into();
    }
    let settings = Settings {
        windowless_rendering_enabled: true as _,
        external_message_pump: true as _,
        ..Default::default()
    };
    assert_eq!(
        initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        ),
        1
    );

    let mut event_loop = EventLoop::new().unwrap();

    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();
    let ret = loop {
        do_message_loop_work();
        let timeout = Some(Duration::ZERO);
        let status = event_loop.pump_app_events(timeout, &mut app);

        if let PumpStatus::Exit(exit_code) = status {
            break ExitCode::from(exit_code as u8);
        }

        sleep(Duration::from_millis(1000 / 17));
    };
    cef::shutdown();
    ret
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];

    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

struct Geometry {
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
}

impl Geometry {
    fn new(device: &wgpu::Device) -> Self {
        let x = -1.0;
        let y = 1.0;
        let width = 2.0;
        let height = 2.0;
        let z = 1.0; // Z value for 2D quad

        let vertices = [
            Vertex {
                position: [x, y, z],
                tex_coords: [0.0, 0.0],
            },
            Vertex {
                position: [x + width, y, z],
                tex_coords: [1.0, 0.0],
            },
            Vertex {
                position: [x, y - height, z],
                tex_coords: [0.0, 1.0],
            },
            Vertex {
                position: [x + width, y - height, z],
                tex_coords: [1.0, 1.0],
            },
        ];

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Quad Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            vertex_buffer,
            vertex_count: vertices.len() as u32,
        }
    }
}

#[cfg(all(target_os = "windows", debug_assertions))]
mod pix {
    use libloading::Library;
    use std::io::{Error, ErrorKind, Result};
    use std::path::PathBuf;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::core::{HSTRING, PCWSTR};

    fn get_latest_winpix_gpu_capturer_path() -> PathBuf {
        PathBuf::from(r"C:\Program Files")
            .join("Microsoft PIX")
            .join("2505.30")
            .join("WinPixGpuCapturer.dll")
    }

    pub fn load_winpix_gpu_capturer() -> Result<()> {
        let module_name = HSTRING::from("WinPixGpuCapturer.dll");

        unsafe {
            let module_pcwstr = PCWSTR::from_raw(module_name.as_ptr());
            let is_loaded = GetModuleHandleW(module_pcwstr).is_ok();

            if !is_loaded {
                let path = get_latest_winpix_gpu_capturer_path();

                if !path.exists() {
                    return Err(Error::new(
                        ErrorKind::NotFound,
                        format!("WinPixGpuCapturer.dll not found at {}", path.display()),
                    ));
                }

                match Library::new(&path) {
                    Ok(lib) => {
                        use std::sync::Once;
                        static INIT: Once = Once::new();
                        static mut LIBRARY: Option<Library> = None;

                        INIT.call_once(|| {
                            LIBRARY = Some(lib);
                        });

                        Ok(())
                    }
                    Err(e) => Err(Error::other(format!(
                        "Failed to load WinPixGpuCapturer.dll: {e}"
                    ))),
                }
            } else {
                Ok(())
            }
        }
    }
}
