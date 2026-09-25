//! Unified texture import system for CEF hardware acceleration
//!
//! This module provides a platform-agnostic interface for importing shared textures
//! from CEF into wgpu, with automatic fallback to CPU textures when hardware
//! acceleration is not available.
//!
//! # Supported Platforms
//!
//! - **Linux**: DMA-BUF via Vulkan external memory
//! - **Windows**: D3D11 shared textures via Vulkan interop
//! - **macOS**: IOSurface via Metal native API
//!
//! # Usage
//!
//! ```no_run
//! use cef::{PaintElementType, AcceleratedPaintInfo};
//! use wgpu::Device;
//! fn on_accelerated_paint(device: &wgpu::Device, type_: PaintElementType, info: Option<&AcceleratedPaintInfo>) {
//!     let Some(info) = info else { return };
//!
//!     let src_texture = {
//!         use cef::osr_texture_import::shared_texture_handle::SharedTextureHandle;
//!
//!         if type_ != PaintElementType::default() {
//!             return;
//!         }
//!
//!         // Import texture with automatic platform detection
//!         let shared_handle = SharedTextureHandle::new(info);
//!         if let SharedTextureHandle::Unsupported = shared_handle {
//!             eprintln!("Platform does not support accelerated painting");
//!             return;
//!         }
//!
//!         match shared_handle.import_texture(device) {
//!             Ok(texture) => texture,
//!                 Err(e) => {
//!                     eprintln!("Failed to import shared texture: {:?}", e);
//!                     return;
//!                 }
//!         }
//!         // Use `src_texture` in rendering pipeline...
//!     };
//!     // Use `src_texture` in rendering pipeline...
//! };
//!```
//! # Features
//!
//! - `accelerated_paint` - Base feature for texture import
//! - `accelerated_paint_dmabuf` - Linux DMA-BUF support
//! - `accelerated_paint_d3d11` - Windows D3D11 support
//! - `accelerated_paint_iosurface` - macOS IOSurface support
//! - `accelerated_osr_dawn` - Dawn-based shared texture import on macOS

pub(crate) mod common;

pub mod shared_texture_handle;
pub use shared_texture_handle::SharedTextureHandle;

#[cfg(target_os = "linux")]
pub(crate) mod dmabuf;

#[cfg(target_os = "windows")]
pub(crate) mod d3d11;

#[cfg(target_os = "macos")]
pub(crate) mod iosurface;

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

    /// Import the texture into wgpu, with automatic fallback to CPU texture
    fn import_to_wgpu(&self, device: &wgpu::Device) -> TextureImportResult;

    /// Check if hardware acceleration is available for this texture
    fn supports_hardware_acceleration(&self, device: &wgpu::Device) -> bool;
}
