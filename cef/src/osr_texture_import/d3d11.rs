//! Windows D3D11 shared texture import implementation

use super::common::{format, texture, vulkan};
use super::{TextureImportError, TextureImportResult, TextureImporter};
use crate::{sys::cef_color_type_t, AcceleratedPaintInfo};
use std::os::raw::c_void;

pub struct D3D11Importer {
    pub handle: *mut c_void,
    pub format: cef_color_type_t,
    pub width: u32,
    pub height: u32,
}

#[cfg(target_os = "windows")]
impl TextureImporter for D3D11Importer {
    fn new(info: &AcceleratedPaintInfo) -> Self {
        Self {
            handle: info.shared_texture_handle,
            format: *info.format.as_ref(),
            width: info.extra.coded_size.width as u32,
            height: info.extra.coded_size.height as u32,
        }
    }

    fn import_to_wgpu(&self, device: &wgpu::Device) -> TextureImportResult {
        // Try hardware acceleration first
        if self.supports_hardware_acceleration(device) {
            #[cfg(feature = "accelerated_osr_dawn")]
            match self.import_via_dawn(device) {
                Ok(texture) => {
                    tracing::info!("Successfully imported D3D11 shared texture via Dawn");
                    return Ok(texture);
                }
                Err(e) => {
                    tracing::debug!(
                        "Failed to import D3D11 via Dawn: {}, trying native fallback",
                        e
                    );
                }
            }
            // Try D3D12 first (most efficient on Windows)
            if vulkan::is_d3d12_backend(device) {
                match self.import_via_d3d12(device) {
                    Ok(texture) => {
                        tracing::info!("Successfully imported D3D11 shared texture via D3D12");
                        return Ok(texture);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to import D3D11 via D3D12: {}, trying Vulkan fallback",
                            e
                        );
                    }
                }
            }

            // Try Vulkan as fallback
            if vulkan::is_vulkan_backend(device) {
                match self.import_via_vulkan(device) {
                    Ok(texture) => {
                        tracing::info!("Successfully imported D3D11 shared texture via Vulkan");
                        return Ok(texture);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to import D3D11 via Vulkan: {}, falling back to CPU texture",
                            e
                        );
                    }
                }
            }
        }

        // Fallback to CPU texture
        texture::create_fallback(
            device,
            self.width,
            self.height,
            self.format,
            "CEF D3D11 Texture (fallback)",
        )
    }

    fn supports_hardware_acceleration(&self, device: &wgpu::Device) -> bool {
        // Check if handle is valid
        if self.handle.is_null() {
            return false;
        }

        #[cfg(feature = "accelerated_osr_dawn")]
        if dawn_wgpu::Compat::<dawn_rs::Device>::try_from(device).is_ok() {
            return true;
        }

        // Check if wgpu is using D3D12 or Vulkan backend
        vulkan::is_d3d12_backend(device) || vulkan::is_vulkan_backend(device)
    }
}

impl D3D11Importer {
    #[cfg(feature = "accelerated_osr_dawn")]
    fn import_via_dawn(&self, device: &wgpu::Device) -> TextureImportResult {
        use dawn_wgpu::{Compat, CompatTexture};

        let dawn_device = Compat::<dawn_rs::Device>::try_from(device)
            .map_err(|_| TextureImportError::HardwareUnavailable {
                reason: "wgpu device is not backed by dawn-wgpu".into(),
            })?
            .into_inner();
        if !dawn_device.has_feature(dawn_rs::FeatureName::SharedTextureMemoryDXGISharedHandle) {
            return Err(TextureImportError::HardwareUnavailable {
                reason: "SharedTextureMemoryDXGISharedHandle feature is unavailable".into(),
            });
        }

        let mut dxgi_desc = dawn_rs::SharedTextureMemoryDXGISharedHandleDescriptor::new();
        dxgi_desc.handle = Some(self.handle);
        dxgi_desc.use_keyed_mutex = Some(false);
        let shared_desc =
            dawn_rs::SharedTextureMemoryDescriptor::new().with_extension(dxgi_desc.into());
        let shared_memory = dawn_device.import_shared_texture_memory(&shared_desc);

        let mut properties = dawn_rs::SharedTextureMemoryProperties::new();
        let properties_status = shared_memory.get_properties(&mut properties);
        if properties_status != dawn_rs::Status::Success {
            return Err(TextureImportError::PlatformError {
                message: format!(
                    "Dawn DXGI get_properties failed with {:?}",
                    properties_status
                ),
            });
        }

        let mut dawn_texture_desc = dawn_rs::TextureDescriptor::new();
        dawn_texture_desc.dimension = Some(dawn_rs::TextureDimension::D2);
        dawn_texture_desc.format = properties
            .format
            .or(Some(format::cef_to_dawn(self.format)?));
        let usage = properties
            .usage
            .unwrap_or(dawn_rs::TextureUsage::TEXTURE_BINDING)
            | dawn_rs::TextureUsage::TEXTURE_BINDING;
        dawn_texture_desc.usage = Some(usage);
        dawn_texture_desc.size = properties.size.or(Some(dawn_rs::Extent3D {
            width: Some(self.width),
            height: Some(self.height),
            depth_or_array_layers: Some(1),
        }));

        let dawn_texture = shared_memory.create_texture(Some(&dawn_texture_desc));
        let mut begin_desc = dawn_rs::SharedTextureMemoryBeginAccessDescriptor::new();
        begin_desc.initialized = Some(true);
        begin_desc.concurrent_read = Some(false);
        let status = shared_memory.begin_access(dawn_texture.clone(), &begin_desc);
        if status != dawn_rs::Status::Success {
            return Err(TextureImportError::PlatformError {
                message: format!("Dawn DXGI begin_access failed with {:?}", status),
            });
        }

        let texture_desc = wgpu::TextureDescriptor {
            label: Some("CEF Dawn DXGI Texture"),
            size: wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: format::cef_to_wgpu(self.format)?,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let texture = wgpu::Texture::from(CompatTexture::new(dawn_texture.clone(), &texture_desc));

        let _ = shared_memory;

        Ok(texture)
    }

    fn import_via_d3d12(&self, device: &wgpu::Device) -> TextureImportResult {
        // Get wgpu's D3D12 device
        use wgpu::hal::api;
        let hal_texture = unsafe {
            let hal_device_guard = device.as_hal::<api::Dx12>();
            let Some(hal_device) = hal_device_guard else {
                return Err(TextureImportError::HardwareUnavailable {
                    reason: "Device is not using D3D12 backend".to_string(),
                });
            };

            // Import D3D11 shared handle directly into D3D12 resource
            let d3d12_resource = self.import_d3d11_handle_to_d3d12(&hal_device)?;

            // Wrap D3D12 resource in wgpu-hal texture
            let hal_texture = <api::Dx12 as wgpu::hal::Api>::Device::texture_from_raw(
                d3d12_resource,
                format::cef_to_wgpu(self.format)?,
                wgpu::TextureDimension::D2,
                wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
                1, // mip_level_count
                1, // sample_count
            );

            Ok::<_, TextureImportError>(hal_texture)
        }?;

        // Import hal texture into wgpu
        let texture = unsafe {
            device.create_texture_from_hal::<api::Dx12>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("CEF D3D11→D3D12 Shared Texture"),
                    size: wgpu::Extent3d {
                        width: self.width,
                        height: self.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: format::cef_to_wgpu(self.format)?,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::TextureUses::RESOURCE,
            )
        };

        Ok(texture)
    }

    fn import_via_vulkan(&self, device: &wgpu::Device) -> TextureImportResult {
        // Get wgpu's Vulkan instance and device
        use wgpu::{wgc::api::Vulkan, TextureUses};
        let hal_texture = unsafe {
            let hal_device_guard = device.as_hal::<Vulkan>();
            let Some(hal_device) = hal_device_guard else {
                return Err(TextureImportError::HardwareUnavailable {
                    reason: "Device is not using Vulkan backend".to_string(),
                });
            };

            // Import D3D11 shared handle into Vulkan
            let hal_texture = <Vulkan as wgpu::hal::Api>::Device::texture_from_d3d11_shared_handle(
                &hal_device, // <-- Pass the raw Vulkan device
                windows::Win32::Foundation::HANDLE(self.handle),
                &wgpu::hal::TextureDescriptor {
                    label: Some("CEF D3D11 Shared Texture"),
                    size: wgpu::Extent3d {
                        width: self.width,
                        height: self.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: format::cef_to_wgpu(self.format)?,
                    usage: TextureUses::COPY_DST | TextureUses::RESOURCE,
                    memory_flags: wgpu::hal::MemoryFlags::empty(),
                    view_formats: vec![],
                },
            )
            .map_err(|e| TextureImportError::PlatformError {
                message: format!("Failed to import D3D11 shared handle into Vulkan: {:?}", e),
            })?;

            Ok::<_, TextureImportError>(hal_texture)
        }?;

        // Import hal texture into wgpu
        let texture = unsafe {
            device.create_texture_from_hal::<Vulkan>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("CEF D3D11 Shared Texture"),
                    size: wgpu::Extent3d {
                        width: self.width,
                        height: self.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: format::cef_to_wgpu(self.format)?,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::TextureUses::RESOURCE,
            )
        };

        Ok(texture)
    }

    fn import_d3d11_handle_to_d3d12(
        &self,
        hal_device: &<wgpu::hal::api::Dx12 as wgpu::hal::Api>::Device,
    ) -> Result<windows::Win32::Graphics::Direct3D12::ID3D12Resource, TextureImportError> {
        use windows::Win32::Graphics::Direct3D12::*;

        // Get D3D12 device from wgpu-hal
        let d3d12_device = hal_device.raw_device();

        // Validate dimensions
        if self.width == 0 || self.height == 0 {
            return Err(TextureImportError::InvalidHandle(
                "Invalid D3D11 texture dimensions".to_string(),
            ));
        }

        // Open D3D11 shared handle on D3D12 device
        unsafe {
            let mut shared_resource: Option<ID3D12Resource> = None;
            d3d12_device
                .OpenSharedHandle(
                    windows::Win32::Foundation::HANDLE(self.handle),
                    &mut shared_resource,
                )
                .map_err(|e| TextureImportError::PlatformError {
                    message: format!("Failed to open D3D11 shared handle on D3D12: {:?}", e),
                })?;

            shared_resource.ok_or_else(|| {
                TextureImportError::InvalidHandle(
                    "Failed to get D3D12 resource from shared handle".to_string(),
                )
            })
        }
    }
}
