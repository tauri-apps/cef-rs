//! Copy Windows shared textures into application-owned wgpu textures.

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

impl TextureImporter for D3D11Importer {
    fn new(info: &AcceleratedPaintInfo) -> Self {
        Self {
            handle: info.shared_texture_handle,
            format: *info.format.as_ref(),
            width: info.extra.coded_size.width as u32,
            height: info.extra.coded_size.height as u32,
        }
    }

    unsafe fn import_to_wgpu(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> TextureImportResult {
        if self.handle.is_null() {
            return Err(TextureImportError::InvalidHandle(
                "Null Windows shared texture handle".to_string(),
            ));
        }
        let limit = device.limits().max_texture_dimension_2d;
        if self.width == 0 || self.height == 0 || self.width > limit || self.height > limit {
            return Err(TextureImportError::InvalidHandle(
                "Invalid Windows shared texture dimensions".to_string(),
            ));
        }

        if vulkan::is_d3d12_backend(device) {
            self.import_via_d3d12(device, queue)
        } else if vulkan::is_vulkan_backend(device) {
            self.import_via_vulkan(device, queue)
        } else {
            Err(TextureImportError::HardwareUnavailable {
                reason: "Windows texture import requires a D3D12 or Vulkan device".to_string(),
            })
        }
    }

    fn supports_hardware_acceleration(&self, device: &wgpu::Device) -> bool {
        !self.handle.is_null()
            && (vulkan::is_d3d12_backend(device)
                || (vulkan::is_vulkan_backend(device)
                    && device
                        .features()
                        .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_WIN32)))
    }
}

impl D3D11Importer {
    fn descriptor(&self, format: wgpu::TextureFormat) -> wgpu::TextureDescriptor<'static> {
        wgpu::TextureDescriptor {
            label: Some("CEF shared Windows texture"),
            size: wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        }
    }

    fn import_via_d3d12(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> TextureImportResult {
        use wgpu::hal::api::Dx12;
        let mut descriptor = self.descriptor(format::cef_to_wgpu(self.format)?);
        descriptor.usage = wgpu::TextureUsages::COPY_SRC;
        let hal_texture = unsafe {
            let hal_device = device.as_hal::<Dx12>().unwrap();
            let resource = self.open_d3d12_resource(&hal_device)?;
            <Dx12 as wgpu::hal::Api>::Device::texture_from_raw(
                resource,
                descriptor.format,
                descriptor.dimension,
                descriptor.size,
                descriptor.mip_level_count,
                descriptor.sample_count,
            )
        };

        // D3D11 shared resources enter and leave in COMMON, which wgpu's
        // D3D12 backend represents as PRESENT.
        let source = unsafe {
            device.create_texture_from_hal::<Dx12>(
                hal_texture,
                &descriptor,
                wgpu::TextureUses::PRESENT,
            )
        };
        texture::copy_to_owned(device, queue, source, Some(wgpu::TextureUses::PRESENT))
    }

    fn import_via_vulkan(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> TextureImportResult {
        use wgpu::hal::api::Vulkan;
        if !device
            .features()
            .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_WIN32)
        {
            return Err(TextureImportError::HardwareUnavailable {
                reason: "Vulkan device requires VULKAN_EXTERNAL_MEMORY_WIN32".to_string(),
            });
        }
        let descriptor = self.descriptor(format::cef_to_wgpu(self.format)?);
        let hal_texture = unsafe {
            let hal_device = device.as_hal::<Vulkan>().unwrap();
            hal_device.texture_from_d3d11_shared_handle(
                windows::Win32::Foundation::HANDLE(self.handle),
                &wgpu::hal::TextureDescriptor {
                    label: descriptor.label,
                    size: descriptor.size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: descriptor.dimension,
                    format: descriptor.format,
                    usage: wgpu::TextureUses::COPY_SRC | wgpu::TextureUses::RESOURCE,
                    memory_flags: wgpu::hal::MemoryFlags::empty(),
                    view_formats: vec![],
                },
            )?
        };

        // D3D11_TEXTURE external memory has an implied GENERAL layout.
        let source = unsafe {
            device.create_texture_from_hal::<Vulkan>(
                hal_texture,
                &descriptor,
                super::vulkan::EXTERNAL_TEXTURE_STATE,
            )
        };
        super::vulkan::copy_to_owned(device, queue, source, ash::vk::QUEUE_FAMILY_EXTERNAL)
    }

    fn open_d3d12_resource(
        &self,
        hal_device: &<wgpu::hal::api::Dx12 as wgpu::hal::Api>::Device,
    ) -> Result<windows::Win32::Graphics::Direct3D12::ID3D12Resource, TextureImportError> {
        use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};

        let expected_format = match self.format {
            cef_color_type_t::CEF_COLOR_TYPE_BGRA_8888 => DXGI_FORMAT_B8G8R8A8_UNORM,
            cef_color_type_t::CEF_COLOR_TYPE_RGBA_8888 => DXGI_FORMAT_R8G8B8A8_UNORM,
            _ => {
                return Err(TextureImportError::UnsupportedFormat {
                    format: self.format,
                })
            }
        };

        // CEF retains ownership of the NT handle.
        let resource = unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            hal_device
                .raw_device()
                .OpenSharedHandle(
                    windows::Win32::Foundation::HANDLE(self.handle),
                    &mut resource,
                )
                .map_err(|error| TextureImportError::PlatformError {
                    message: format!("Failed to open Windows shared texture on D3D12: {error}"),
                })?;
            resource.ok_or_else(|| {
                TextureImportError::InvalidHandle(
                    "Shared handle did not yield a D3D12 resource".to_string(),
                )
            })?
        };
        let actual = unsafe { resource.GetDesc() };
        if actual.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
            || actual.Width != u64::from(self.width)
            || actual.Height != self.height
            || actual.DepthOrArraySize != 1
            || actual.MipLevels != 1
            || actual.SampleDesc.Count != 1
            || actual.SampleDesc.Quality != 0
            || actual.Format != expected_format
        {
            return Err(TextureImportError::InvalidHandle(
                "D3D12 resource descriptor does not match the CEF frame".to_string(),
            ));
        }
        Ok(resource)
    }
}
