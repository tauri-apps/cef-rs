//! GPU surfaces, OSR→swapchain compositing, and related presentation (OSR host state lives in `cef::shell`).

//! Windowless rendering: wgpu surfaces and presentation (hub + OSR handler live in `cef::osr`).
//!
//! ## Bevy `bevy_render` and `SharedGpu` (spike)
//!
//! - **Branch A (single `wgpu::Device` shared with Bevy):** `bevy_render` 0.15 is built on `wgpu` 23;
//!   vmux and CEF use workspace `wgpu` 28 (`SharedGpu`). Types do not unify,
//!   so `RenderDevice::from(Device)` cannot wrap vmux's device without aligning the entire workspace on one
//!   `wgpu` major (or upgrading Bevy).
//! - **Branch B (current):** One GPU context for CEF/OSR (`SharedGpu`, `wgpu` 28). Composite encode/present runs
//!   on that device. A minimal `wgpu` 23 stack exists only so `bevy_render::render_graph::Node::run` receives a
//!   valid `RenderContext`; the OSR node does not use it for the CEF→swapchain path.

//! Centralized CEF view texture color/alpha policy.

/// CEF view buffers represent sRGB web content in BGRA bytes.
///
/// We keep the underlying texture as UNORM and sample through an sRGB view.
/// This avoids touching upstream `cef/` while still getting correct colors.
pub const CEF_VIEW_TEXTURE_BASE_FORMAT_BGRA: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
pub const CEF_VIEW_TEXTURE_VIEW_FORMAT_BGRA: wgpu::TextureFormat =
    wgpu::TextureFormat::Bgra8UnormSrgb;
pub const CEF_VIEW_TEXTURE_FORMATS_BGRA: [wgpu::TextureFormat; 1] =
    [CEF_VIEW_TEXTURE_VIEW_FORMAT_BGRA];

/// vmux renders embedded CEF content into an opaque window.
pub const OUTPUT_IS_OPAQUE: bool = true;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ::cef::ImplBrowser as _;
use bevy_ecs::prelude::{Mut, Resource};
use bevy_ecs::world::World;
use bevy_render::render_graph::{
    Node, NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, SlotValue,
};
use bevy_render::renderer::RenderContext;
use wgpu::Backends;
use winit::window::WindowId;

use crate::browser::cef::entity::{BrowserId, BrowserWindowId};
use crate::browser::cef::osr::WindowEntry;
use crate::browser::cef::{ForeignOsrIndexResource, GpuResource};
use crate::runtime::RuntimeState;
use wgpu::util::DeviceExt;
use winit::dpi::PhysicalSize;
use winit::window::Window;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

pub struct Geometry {
    pub vertex_buffer: wgpu::Buffer,
    pub vertex_count: u32,
}

fn fullscreen_quad_vertices(offset_x_ndc: f32) -> [Vertex; 4] {
    let x = -1.0 + offset_x_ndc;
    let y = 1.0;
    let width = 2.0;
    let height = 2.0;
    let z = 1.0;
    [
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
    ]
}

impl Geometry {
    pub fn new(device: &wgpu::Device) -> Self {
        let vertices = fullscreen_quad_vertices(0.0);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vmux-osr quad"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            vertex_buffer,
            vertex_count: vertices.len() as u32,
        }
    }
}

pub struct SharedGpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub texture_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    pub surface_format: wgpu::TextureFormat,
}

impl SharedGpu {
    pub async fn new_headless() -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: Backends::from_comma_list("metal"),
            ..Default::default()
        });
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
        {
            Ok(a) => a,
            Err(e) => {
                eprintln!("vmux FATAL: wgpu request_adapter failed: {e}");
                std::process::exit(78);
            }
        };
        let (device, queue) = match adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_non_sampler_bindings: 2048,
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
        {
            Ok(x) => x,
            Err(e) => {
                eprintln!("vmux FATAL: wgpu request_device failed: {e}");
                std::process::exit(78);
            }
        };
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vmux-osr cef texture"),
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
        Self {
            instance,
            adapter,
            device,
            queue,
            texture_bind_group_layout: Arc::new(texture_bind_group_layout),
            surface_format: wgpu::TextureFormat::Bgra8Unorm,
        }
    }

    /// Match **`examples/osr`**: pick the Metal adapter using a real window surface **before** CEF creates
    /// a browser. Headless `request_adapter` can differ and correlated with macOS + CEF startup traps.
    pub async fn new_with_window(window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: Backends::from_comma_list("metal"),
            ..Default::default()
        });
        let surface = instance
            .create_surface(window)
            .expect("vmux FATAL: wgpu create_surface (new_with_window)");
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
        {
            Ok(a) => a,
            Err(e) => {
                eprintln!("vmux FATAL: wgpu request_adapter failed after surface: {e}");
                std::process::exit(78);
            }
        };
        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .or_else(|| caps.formats.first().copied())
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        drop(surface);
        let (device, queue) = match adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_non_sampler_bindings: 2048,
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
        {
            Ok(x) => x,
            Err(e) => {
                eprintln!("vmux FATAL: wgpu request_device failed: {e}");
                std::process::exit(78);
            }
        };
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vmux-osr cef texture"),
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
        Self {
            instance,
            adapter,
            device,
            queue,
            texture_bind_group_layout: Arc::new(texture_bind_group_layout),
            surface_format,
        }
    }

    pub fn create_pipeline(&self, surface_format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("vmux-osr shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
            });
        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("vmux-osr pipeline layout"),
                bind_group_layouts: &[&self.texture_bind_group_layout],
                immediate_size: 0,
            });
        self.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("vmux-osr pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Vertex::desc()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        // We always render the quad as opaque (shader outputs alpha=1).
                        blend: None,
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
            })
    }
}

pub struct WindowSurface {
    pub window: Arc<Window>,
    pub surface: wgpu::Surface<'static>,
    pub pipeline: wgpu::RenderPipeline,
    pub quad: Geometry,
    pub configured_size: PhysicalSize<u32>,
    pub surface_format: wgpu::TextureFormat,
    pub alpha_mode: wgpu::CompositeAlphaMode,
}

impl WindowSurface {
    pub fn new(gpu: &SharedGpu, window: Arc<Window>) -> Result<Self, String> {
        let surface = gpu
            .instance
            .create_surface(window.clone())
            .map_err(|e| format!("create_surface: {e}"))?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let surface_format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| {
                format!(
                    "surface has no supported formats (adapter formats={:?})",
                    caps.formats
                )
            })?;
        // Prefer an opaque swapchain if supported. If the swapchain is translucent and the
        // sampled content carries alpha < 1 (common with accelerated OSR textures), the whole
        // window will look like a grey/dim overlay due to compositor blending.
        let alpha_mode = caps
            .alpha_modes
            .iter()
            .copied()
            .find(|m| *m == wgpu::CompositeAlphaMode::Opaque)
            .unwrap_or(wgpu::CompositeAlphaMode::Auto);

        if cfg!(debug_assertions) && std::env::var_os("VMUX_CEF_DEBUG_GPU").is_some() {
            println!(
                "[vmux-osr gpu pid={}] surface_format={:?} (srgb={}) alpha_mode={:?} caps_formats={:?} caps_alpha_modes={:?}",
                std::process::id(),
                surface_format,
                surface_format.is_srgb(),
                alpha_mode,
                caps.formats,
                caps.alpha_modes
            );
        }
        let pipeline = gpu.create_pipeline(surface_format);
        let quad = Geometry::new(&gpu.device);
        let size = window.inner_size();
        let mut s = Self {
            window,
            surface,
            pipeline,
            quad,
            configured_size: size,
            surface_format,
            alpha_mode,
        };
        s.configure(gpu);
        Ok(s)
    }

    pub fn configure(&mut self, gpu: &SharedGpu) {
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.surface_format,
            view_formats: vec![self.surface_format],
            alpha_mode: self.alpha_mode,
            width: self.configured_size.width.max(1),
            height: self.configured_size.height.max(1),
            desired_maximum_frame_latency: 2,
            present_mode: wgpu::PresentMode::AutoVsync,
        };
        self.surface.configure(&gpu.device, &config);
    }

    pub fn resize(&mut self, gpu: &SharedGpu, new_size: PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.configured_size = new_size;
            self.configure(gpu);
        }
    }

    /// Composite CEF OSR content to this window’s swapchain (clear → optional textured quad → present).
    ///
    /// CEF still replaces bind groups on the UI thread; this path only **reads** the current bind group.
    /// Encoder / pass / submit are grouped here so a `bevy_render` graph node can mirror the same stages.
    #[inline]
    pub fn composite_osr_frame(&mut self, gpu: &SharedGpu, bind_group: Option<&wgpu::BindGroup>) {
        self.write_fullscreen_quad_vertices(gpu);
        let Some((frame, encoder)) = self.encode_osr_pass(gpu, bind_group) else {
            return;
        };
        self.submit_encoder_and_present(gpu, frame, encoder);
    }

    /// Back-compat name for [`Self::composite_osr_frame`].
    #[inline]
    pub fn render(&mut self, gpu: &SharedGpu, bind_group: Option<&wgpu::BindGroup>) {
        self.composite_osr_frame(gpu, bind_group);
    }

    fn write_fullscreen_quad_vertices(&mut self, gpu: &SharedGpu) {
        let vertices = fullscreen_quad_vertices(0.0);
        gpu.queue
            .write_buffer(&self.quad.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
    }

    /// Acquire swapchain texture and record clear (+ optional draw) into a command encoder.
    fn encode_osr_pass(
        &mut self,
        gpu: &SharedGpu,
        bind_group: Option<&wgpu::BindGroup>,
    ) -> Option<(wgpu::SurfaceTexture, wgpu::CommandEncoder)> {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => return None,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("vmux-osr surface"),
            ..Default::default()
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("vmux-osr encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vmux-osr pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            // Only draw the textured quad once we have a texture bind-group from CEF.
            // Otherwise we just clear the surface. The pipeline expects bind group 0,
            // so drawing without it is a wgpu validation error.
            if let Some(bg) = bind_group {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, Some(bg), &[]);
                pass.set_vertex_buffer(0, self.quad.vertex_buffer.slice(..));
                pass.draw(0..self.quad.vertex_count, 0..1);
            }
        }
        Some((frame, encoder))
    }

    fn submit_encoder_and_present(
        &mut self,
        gpu: &SharedGpu,
        frame: wgpu::SurfaceTexture,
        encoder: wgpu::CommandEncoder,
    ) {
        gpu.queue.submit(std::iter::once(encoder.finish()));
        self.window.pre_present_notify();
        frame.present();
    }
}

// `bevy_render`-shaped OSR composite: `RenderGraph` + extract + flush on redraw (same module as `SharedGpu` / wgpu surfaces above).

/// ECS mirror of `WindowId` → CEF `browser_id` for render-side reads (same info as the OSR host map, kept in sync each frame before redraw flush).
#[derive(Resource, Default, Clone)]
pub struct ExtractedOsrBrowserIds(pub HashMap<WindowId, i32>);

pub fn extract_osr_browser_ids(world: &mut bevy_ecs::world::World) {
    let mut map = HashMap::new();
    let mut q = world.query::<(&BrowserWindowId, &BrowserId)>();
    for (wid, bid) in q.iter(world) {
        map.insert(wid.0, bid.0);
    }
    world.insert_resource(ExtractedOsrBrowserIds(map));
}

/// Label for the OSR→swapchain composite node in [`VmuxOsrRenderGraphState::graph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, RenderLabel)]
pub struct OsrCompositeGraphLabel;

pub struct OsrCompositeNode;

impl Node for OsrCompositeNode {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        _render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let Some(token) = world.get_resource::<VmuxOsrRedrawToken>() else {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "OSR composite: missing VmuxOsrRedrawToken (skip)"
            );
            return Ok(());
        };
        let window_id = token.0;

        let gpu = world.resource::<GpuResource>().0.clone();
        let index = world.resource::<ForeignOsrIndexResource>().0.clone();
        let store = world.resource::<VmuxWindowsStoreResource>().0.clone();

        let mut guard = store.lock().unwrap_or_else(|e| {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "OSR composite: windows_store mutex poisoned, using inner map"
            );
            e.into_inner()
        });
        let Some(entry) = guard.get_mut(&window_id) else {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "OSR composite: WindowId {:?} not in windows_store (race or stale redraw; skip)",
                window_id
            );
            return Ok(());
        };
        let browser_id = entry.browser.identifier();

        let painted = crate::browser::cef::osr::with_bind_group(index.as_ref(), browser_id, |bg| {
            entry.surface.composite_osr_frame(&*gpu, Some(bg));
        })
        .is_some();

        if !painted {
            entry.surface.composite_osr_frame(&*gpu, None);
        }
        entry.surface.window.request_redraw();
        Ok(())
    }
}

/// Cloned from [`crate::browser::cef::osr::CefAttach::windows_store`] at startup so `bevy_render` graph nodes can lock the same map as dispatch (under `&World`).
#[derive(Resource, Clone)]
pub struct VmuxWindowsStoreResource(
    pub Arc<Mutex<std::collections::HashMap<WindowId, WindowEntry>>>,
);

/// Per-frame token: which window is being composited (inserted immediately before running the graph).
#[derive(Resource, Clone, Copy)]
pub struct VmuxOsrRedrawToken(pub WindowId);

/// Holds the retained [`RenderGraph`] and lazily initialized wgpu 23 handles for a minimal [`bevy_render::renderer::RenderContext`].
#[derive(Resource)]
pub struct VmuxOsrRenderGraphState {
    pub graph: RenderGraph,
    pub(crate) dummy_wgpu: std::sync::OnceLock<DummyWgpuBevy>,
}

pub(crate) struct DummyWgpuBevy {
    pub device: bevy_render::renderer::RenderDevice,
    pub queue: bevy_render::renderer::RenderQueue,
    /// Plain `wgpu` 23 info for [`bevy_render::renderer::RenderContext::new`].
    pub adapter_info_plain: wgpu_bevy::AdapterInfo,
}

impl Default for VmuxOsrRenderGraphState {
    fn default() -> Self {
        let mut graph = RenderGraph::default();
        graph.add_node(OsrCompositeGraphLabel, OsrCompositeNode);
        Self {
            graph,
            dummy_wgpu: std::sync::OnceLock::new(),
        }
    }
}

/// `bevy_render::RenderContext::finish` expects Bevy’s global task pools (normally from `DefaultPlugins`).
fn ensure_bevy_render_task_pools() {
    use bevy_tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| {
        TaskPoolBuilder::new()
            .thread_name("vmux Compute".to_string())
            .build()
    });
    AsyncComputeTaskPool::get_or_init(|| {
        TaskPoolBuilder::new()
            .thread_name("vmux AsyncCompute".to_string())
            .build()
    });
    IoTaskPool::get_or_init(|| {
        TaskPoolBuilder::new()
            .thread_name("vmux Io".to_string())
            .build()
    });
}

/// Run extract → insert redraw token → execute the single OSR composite [`bevy_render::render_graph::Node`], then submit the **dummy** wgpu 23 encoder (OSR work uses [`GpuResource`] / vmux wgpu 28 inside the node).
pub fn run_osr_composite_for_window(world: &mut World, window_id: WindowId) {
    ensure_bevy_render_task_pools();
    extract_osr_browser_ids(world);
    world.insert_resource(VmuxOsrRedrawToken(window_id));
    let (device, queue, adapter_info_plain) =
        world.resource_scope(|world, mut state: Mut<VmuxOsrRenderGraphState>| {
            state.graph.update(world);
            let d = state.ensure_dummy_wgpu_bevy();
            (
                d.device.clone(),
                d.queue.clone(),
                d.adapter_info_plain.clone(),
            )
        });
    let mut render_context = RenderContext::new(device, adapter_info_plain, None);
    if let Err(e) = run_single_osr_node(&*world, &mut render_context) {
        bevy_log::error!(
            target: "vmux",
            pid = std::process::id(),
            "OSR composite node failed (skipping frame): {e:?}"
        );
        drop(render_context);
        world.remove_resource::<VmuxOsrRedrawToken>();
        return;
    }
    let (command_buffers, _, _diag) = render_context.finish();
    queue.submit(command_buffers);
    world.remove_resource::<VmuxOsrRedrawToken>();
}

fn run_single_osr_node<'a>(
    world: &'a World,
    render_context: &mut RenderContext<'a>,
) -> Result<(), bevy_render::render_graph::NodeRunError> {
    let graph = &world.resource::<VmuxOsrRenderGraphState>().graph;
    let node_state = graph
        .get_node_state(OsrCompositeGraphLabel)
        .expect("vmux: OsrCompositeGraphLabel node");
    let inputs: &[SlotValue] = &[];
    let mut outputs: Vec<Option<SlotValue>> = vec![None; node_state.output_slots.len()];
    let mut ctx = RenderGraphContext::new(graph, node_state, inputs, &mut outputs);
    node_state.node.run(&mut ctx, render_context, world)?;
    Ok(())
}

impl VmuxOsrRenderGraphState {
    pub(crate) fn ensure_dummy_wgpu_bevy(&mut self) -> &DummyWgpuBevy {
        self.dummy_wgpu.get_or_init(|| {
            pollster::block_on(async {
                init_dummy_wgpu_bevy_async().await.unwrap_or_else(|e| {
                    bevy_log::error!(
                        target: "vmux",
                        pid = std::process::id(),
                        "FATAL: bevy_render dummy wgpu 23 init failed: {e}"
                    );
                    eprintln!("vmux: FATAL dummy wgpu (bevy_render): {e}");
                    std::process::exit(1);
                })
            })
        })
    }
}

async fn init_dummy_wgpu_bevy_async() -> Result<DummyWgpuBevy, String> {
    use bevy_render::renderer::{RenderDevice, RenderQueue, WgpuWrapper};
    use wgpu_bevy::Backends;

    // Match [`SharedGpu::new_headless`] — `Backends::all()` can pick a bad backend on macOS and
    // `request_adapter` then fails or misbehaves next to the Metal OSR instance.
    let backends = Backends::METAL;

    let instance = wgpu_bevy::Instance::new(wgpu_bevy::InstanceDescriptor {
        backends,
        ..Default::default()
    });
    let adapter = instance
        .request_adapter(&wgpu_bevy::RequestAdapterOptions::default())
        .await
        .ok_or_else(|| {
            "dummy wgpu 23: request_adapter returned None (try GPU / Metal availability)"
                .to_string()
        })?;
    let adapter_info_plain = adapter.get_info();
    let (device, queue) = adapter
        .request_device(
            &wgpu_bevy::DeviceDescriptor {
                label: Some("vmux bevy_render dummy"),
                required_features: wgpu_bevy::Features::empty(),
                required_limits: wgpu_bevy::Limits::default(),
                memory_hints: wgpu_bevy::MemoryHints::default(),
            },
            None,
        )
        .await
        .map_err(|e| format!("dummy wgpu 23: request_device failed: {e}"))?;
    Ok(DummyWgpuBevy {
        device: RenderDevice::from(device),
        queue: RenderQueue(Arc::new(WgpuWrapper::new(queue))),
        adapter_info_plain,
    })
}

/// Pending shell: same as inline path in [`crate::window::dispatch`] before CEF attaches.
pub fn composite_pending_surface(world: &mut World, window_id: WindowId) {
    use crate::browser::cef::GpuResource;

    let gpu = world.resource::<GpuResource>().0.clone();
    let mut rt = world.resource_mut::<RuntimeState>();
    if let Some(p) = rt
        .pending_browser_hosts
        .iter_mut()
        .find(|p| p.surface.window.id() == window_id)
    {
        p.surface.composite_osr_frame(&*gpu, None);
        p.surface.window.request_redraw();
    }
}

/// Drains [`RuntimeState::vmux_osr_redraw_queue`] after OSR window dispatches (see `WindowsPlugin` + `BrowserPlugin` ordering).
pub fn vmux_osr_flush_redraw_queue_system(world: &mut World) {
    use crate::browser::cef::GpuResource;

    // macOS: Bevy `GpuResource` is inserted in `winit_runner_resumed` after the first window; an
    // early `RedrawRequested` before that would otherwise panic in `composite_pending_surface`.
    let qsize = world
        .get_resource::<crate::runtime::RuntimeState>()
        .map(|rt| rt.vmux_osr_redraw_queue.len())
        .unwrap_or(0);
    if world.get_resource::<GpuResource>().is_none() {
        if qsize > 0 {
            crate::log::record_runtime_event(&format!(
                "vmux_osr_flush_skipped_no_gpu redraw_queue_len={qsize}"
            ));
        }
        return;
    }

    let pending: Vec<WindowId> = {
        let mut rt = world.resource_mut::<RuntimeState>();
        rt.vmux_osr_redraw_queue.drain(..).collect()
    };
    for wid in pending {
        flush_osr_redraw_for_window(world, wid);
    }
}

/// Full redraw flush: attached window uses the render graph; pending host uses [`composite_pending_surface`].
pub fn flush_osr_redraw_for_window(world: &mut World, window_id: WindowId) {
    let attached = world
        .resource::<VmuxWindowsStoreResource>()
        .0
        .lock()
        .map(|g| g.contains_key(&window_id))
        .unwrap_or(false);
    if attached {
        run_osr_composite_for_window(world, window_id);
    } else {
        composite_pending_surface(world, window_id);
    }
}
