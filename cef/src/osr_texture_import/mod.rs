//! Unified texture import system for CEF hardware acceleration
//!
//! Copies CEF shared textures into application-owned wgpu textures and waits for
//! completion before CEF can reuse the source after `on_accelerated_paint`.
//!
//! Import failures are returned as errors. Software rendering uses `on_paint`.
//!
//! # Supported Platforms
//!
//! - **Linux**: DMA-BUF via Vulkan external memory
//! - **Windows**: D3D11 shared textures via D3D12 or Vulkan interop
//! - **macOS**: IOSurface via Metal native API
//!
//! # Usage
//!
//! ```no_run
//! use cef::{osr_texture_import::SharedTextureHandle, PaintElementType, AcceleratedPaintInfo};
//! fn on_accelerated_paint(device: &wgpu::Device, queue: &wgpu::Queue, type_: PaintElementType, info: Option<&AcceleratedPaintInfo>) {
//!     let Some(info) = info else { return };
//!     if type_ != PaintElementType::default() {
//!         return;
//!     }
//!     let shared_handle = SharedTextureHandle::new(info);
//!     // SAFETY: Called inside the CEF callback, with its current frame and
//!     // a device/queue using the same GPU (and driver for Vulkan) as CEF.
//!     let texture = match unsafe { shared_handle.import_texture(device, queue) } {
//!         Ok(texture) => texture,
//!         Err(error) => {
//!             eprintln!("Failed to copy shared texture: {error}");
//!             return;
//!         }
//!     };
//!     // The texture can be retained after the callback.
//! }
//! ```
//!
//! # Features
//!
//! Enable `accelerated_osr` for texture import on the current platform.
//! Vulkan devices must enable `wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF`
//! on Linux or `wgpu::Features::VULKAN_EXTERNAL_MEMORY_WIN32` on Windows.

pub(crate) mod common;

pub mod shared_texture_handle;
pub use shared_texture_handle::SharedTextureHandle;

#[cfg(target_os = "linux")]
pub(crate) mod dmabuf;

#[cfg(target_os = "windows")]
pub(crate) mod d3d11;

#[cfg(target_os = "macos")]
pub(crate) mod iosurface;

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) mod vulkan;

/// Result type for texture import operations
pub type TextureImportResult = Result<wgpu::Texture, TextureImportError>;

/// Errors that can occur during texture import
#[derive(Debug, thiserror::Error)]
pub enum TextureImportError {
    #[error("Invalid texture handle: {0}")]
    InvalidHandle(String),

    #[error("Unsupported texture format: {format:?}")]
    UnsupportedFormat {
        format: crate::sys::cef_color_type_t,
    },

    #[error("Hardware acceleration not available: {reason}")]
    HardwareUnavailable { reason: String },

    #[error("Vulkan operation failed: {operation}")]
    VulkanError { operation: String },

    #[error("Platform-specific error: {message}")]
    PlatformError { message: String },

    #[error("Unsupported platform for texture import")]
    UnsupportedPlatform,
}

impl From<wgpu::hal::DeviceError> for TextureImportError {
    fn from(e: wgpu::hal::DeviceError) -> Self {
        TextureImportError::PlatformError {
            message: format!("wgpu-hal DeviceError: {:?}", e),
        }
    }
}

/// Trait for platform-specific texture importers
pub trait TextureImporter {
    fn new(info: &crate::AcceleratedPaintInfo) -> Self;

    /// Copy the shared frame into an owned texture, waiting for GPU completion.
    ///
    /// # Safety
    ///
    /// Call within the originating `on_accelerated_paint` callback, while the
    /// source remains valid and available for reading. `queue` must belong to
    /// `device`, using the same physical GPU as CEF and the same driver for Vulkan.
    unsafe fn import_to_wgpu(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> TextureImportResult;

    /// Check if hardware acceleration is available for this texture
    fn supports_hardware_acceleration(&self, device: &wgpu::Device) -> bool;
}
