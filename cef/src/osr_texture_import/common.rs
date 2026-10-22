//! Common utilities and traits for texture import across platforms

use super::*;
use crate::sys::cef_color_type_t;
use wgpu::Device;

/// Common format conversion utilities
pub mod format {
    use super::*;

    /// Convert CEF color type to wgpu texture format
    pub fn cef_to_wgpu(
        format: cef_color_type_t,
    ) -> Result<wgpu::TextureFormat, TextureImportError> {
        match format {
            cef_color_type_t::CEF_COLOR_TYPE_BGRA_8888 => Ok(wgpu::TextureFormat::Bgra8Unorm),
            cef_color_type_t::CEF_COLOR_TYPE_RGBA_8888 => Ok(wgpu::TextureFormat::Rgba8Unorm),
            _ => Err(TextureImportError::UnsupportedFormat { format }),
        }
    }

    #[cfg(target_os = "linux")]
    /// Convert CEF color type to Vulkan format
    pub fn cef_to_vulkan(format: cef_color_type_t) -> Result<ash::vk::Format, TextureImportError> {
        match format {
            cef_color_type_t::CEF_COLOR_TYPE_BGRA_8888 => Ok(ash::vk::Format::B8G8R8A8_UNORM),
            cef_color_type_t::CEF_COLOR_TYPE_RGBA_8888 => Ok(ash::vk::Format::R8G8B8A8_UNORM),
            _ => Err(TextureImportError::UnsupportedFormat { format }),
        }
    }
}

/// Common texture creation utilities
pub mod texture {
    use super::*;

    /// Copy the source and wait before CEF can reuse it.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn copy_to_owned(
        device: &Device,
        queue: &wgpu::Queue,
        source: wgpu::Texture,
        release_state: Option<wgpu::TextureUses>,
    ) -> TextureImportResult {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("CEF owned frame"),
            size: source.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: source.format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("CEF frame copy"),
        });
        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            texture.as_image_copy(),
            source.size(),
        );
        if let Some(state) = release_state {
            encoder.transition_resources(
                std::iter::empty(),
                std::iter::once(wgpu::TextureTransition {
                    texture: &source,
                    selector: None,
                    state,
                }),
            );
        }
        let submission = queue.submit([encoder.finish()]);
        wait_for_copy(device, submission)?;
        Ok(texture)
    }

    pub fn wait_for_copy(
        device: &Device,
        submission: wgpu::SubmissionIndex,
    ) -> Result<(), TextureImportError> {
        loop {
            match device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission.clone()),
                timeout: None,
            }) {
                Ok(_) => return Ok(()),
                // CEF must not reuse the source while the copy is in flight.
                Err(wgpu::PollError::Timeout) => continue,
                Err(error) => {
                    return Err(TextureImportError::PlatformError {
                        message: format!("Failed to wait for CEF frame copy: {error}"),
                    });
                }
            }
        }
    }
}

/// Common Vulkan utilities
pub mod vulkan {
    #[cfg(not(target_os = "macos"))]
    use super::Device;

    #[cfg(target_os = "linux")]
    use ash::vk;

    /// Find a suitable memory type index for Vulkan allocation
    #[cfg(target_os = "linux")]
    pub fn find_memory_type_index(
        type_filter: u32,
        properties: vk::MemoryPropertyFlags,
        mem_properties: &vk::PhysicalDeviceMemoryProperties,
    ) -> Option<u32> {
        (0..mem_properties.memory_type_count).find(|&i| {
            (type_filter & (1 << i)) != 0
                && mem_properties.memory_types[i as usize]
                    .property_flags
                    .contains(properties)
        })
    }

    /// Check if the wgpu device is using Vulkan backend
    #[cfg(not(target_os = "macos"))]
    pub fn is_vulkan_backend(device: &Device) -> bool {
        use wgpu::hal::api;
        unsafe { device.as_hal::<api::Vulkan>().is_some() }
    }

    /// Check if the wgpu device is using D3D12 backend
    #[cfg(target_os = "windows")]
    pub fn is_d3d12_backend(device: &Device) -> bool {
        use wgpu::hal::api;
        unsafe { device.as_hal::<api::Dx12>().is_some() }
    }
}
