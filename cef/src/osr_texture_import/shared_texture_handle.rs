use crate::AcceleratedPaintInfo;

use super::{TextureImportError, TextureImportResult, TextureImporter};

/// Borrowed CEF frame metadata; does not retain the shared resource.
pub enum SharedTextureHandle {
    #[cfg(target_os = "linux")]
    DmaBuf(super::dmabuf::DmaBufImporter),
    #[cfg(target_os = "windows")]
    D3D11(super::d3d11::D3D11Importer),
    #[cfg(target_os = "macos")]
    IOSurface(super::iosurface::IOSurfaceImporter),
    Unsupported,
}

impl SharedTextureHandle {
    pub fn new(info: &AcceleratedPaintInfo) -> Self {
        // Extract DMA-BUF information
        #[cfg(target_os = "linux")]
        return Self::DmaBuf(super::dmabuf::DmaBufImporter::new(info));

        // Extract D3D11 shared handle with texture metadata
        #[cfg(target_os = "windows")]
        return Self::D3D11(super::d3d11::D3D11Importer::new(info));

        // Extract IOSurface handle with texture metadata
        #[cfg(target_os = "macos")]
        return Self::IOSurface(super::iosurface::IOSurfaceImporter::new(info));

        #[allow(unreachable_code)]
        Self::Unsupported
    }

    /// Copy the current shared frame into an application-owned texture.
    ///
    /// Waits for the GPU copy so the texture can outlive the CEF callback.
    ///
    /// # Safety
    ///
    /// Call within the originating `on_accelerated_paint` callback, while the
    /// source remains valid and available for reading. `queue` must belong to
    /// `device`, using the same physical GPU as CEF and the same driver for Vulkan.
    pub unsafe fn import_texture(
        self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> TextureImportResult {
        match self {
            #[cfg(target_os = "linux")]
            SharedTextureHandle::DmaBuf(importer) => unsafe {
                importer.import_to_wgpu(device, queue)
            },
            #[cfg(target_os = "windows")]
            SharedTextureHandle::D3D11(importer) => unsafe {
                importer.import_to_wgpu(device, queue)
            },
            #[cfg(target_os = "macos")]
            SharedTextureHandle::IOSurface(importer) => unsafe {
                importer.import_to_wgpu(device, queue)
            },
            SharedTextureHandle::Unsupported => Err(TextureImportError::UnsupportedPlatform),
        }
    }
}
