//! macOS IOSurface texture import implementation

use super::common::texture;
use super::{TextureImportError, TextureImportResult, TextureImporter};
use crate::osr_texture_import::common::format;
use crate::{sys::cef_color_type_t, AcceleratedPaintInfo};
use objc2::rc::Retained;
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};
use wgpu::TextureDescriptor;

use std::os::raw::c_void;

pub struct IOSurfaceImporter {
    pub handle: *mut c_void,
    pub format: cef_color_type_t,
    pub width: u32,
    pub height: u32,
}

impl TextureImporter for IOSurfaceImporter {
    fn new(info: &AcceleratedPaintInfo) -> Self {
        Self {
            handle: info.shared_texture_io_surface,
            format: *info.format.as_ref(),
            width: info.extra.coded_size.width as u32,
            height: info.extra.coded_size.height as u32,
        }
    }

    fn import_to_wgpu(&self, device: &wgpu::Device) -> TextureImportResult {
        // Try hardware acceleration first
        if self.supports_hardware_acceleration(device) {
            match self.import_via_metal(device) {
                Ok(texture) => {
                    tracing::trace!("Successfully imported IOSurface texture via Metal");
                    return Ok(texture);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to import IOSurface via Metal: {}, falling back to CPU texture",
                        e
                    );
                }
            }
        }

        // Fallback to CPU texture
        texture::create_fallback(
            device,
            self.width,
            self.height,
            self.format,
            "CEF IOSurface Texture (fallback)",
        )
    }

    fn supports_hardware_acceleration(&self, device: &wgpu::Device) -> bool {
        // Check if handle is valid
        if self.handle.is_null() {
            return false;
        }

        // Check if wgpu is using Metal backend
        self.is_metal_backend(device)
    }
}

impl IOSurfaceImporter {
    fn get_texture_desc(&self) -> TextureDescriptor<'_> {
        use wgpu::{Extent3d, TextureDimension, TextureUsages};

        TextureDescriptor {
            label: Some("Cef Texture"),
            size: Extent3d {
                width: self.width as _,
                height: self.height as _,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: format::cef_to_wgpu(self.format).expect("Unsupported CEF color format"),
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC,
            view_formats: &[],
        }
    }
    fn get_metal_desc(
        &self,
        texture_desc: &TextureDescriptor,
    ) -> Result<Retained<MTLTextureDescriptor>, TextureImportError> {
        if self.width == 0 || self.height == 0 {
            return Err(TextureImportError::InvalidHandle(
                "Invalid IOSurface texture dimensions".to_string(),
            ));
        }

        let metal_desc = MTLTextureDescriptor::new();
        unsafe {
            metal_desc.setWidth(texture_desc.size.width as _);
            metal_desc.setHeight(texture_desc.size.height as _);
            metal_desc.setArrayLength(texture_desc.array_layer_count() as _);
            metal_desc.setMipmapLevelCount(texture_desc.mip_level_count as _);
            metal_desc.setSampleCount(texture_desc.sample_count as _);
            metal_desc.setTextureType(MTLTextureType::Type2D);
            metal_desc.setPixelFormat(match texture_desc.format {
                wgpu::TextureFormat::Rgba8Unorm => MTLPixelFormat::RGBA8Unorm,
                wgpu::TextureFormat::Bgra8Unorm => MTLPixelFormat::BGRA8Unorm,
                _ => unimplemented!(),
            });
            metal_desc.setUsage(MTLTextureUsage::ShaderRead);
            metal_desc.setStorageMode(MTLStorageMode::Managed);
        }

        Ok(metal_desc)
    }

    fn import_via_metal(&self, device: &wgpu::Device) -> TextureImportResult {
        // Convert handle to IOSurface
        let io_surface = std::ptr::NonNull::new(self.handle.cast::<IOSurfaceRef>()).ok_or(
            TextureImportError::InvalidHandle("Invalid IOSurface handle".to_string()),
        )?;

        let texture_desc = self.get_texture_desc();
        let hal_tex = {
            let metal_desc = self.get_metal_desc(&texture_desc)?;

            // Get Metal device from wgpu and create texture
            let texture = unsafe {
                let hal_device_guard = device.as_hal::<wgpu::wgc::api::Metal>();
                let Some(hal_device) = hal_device_guard else {
                    return Err(TextureImportError::InvalidHandle(
                        "Failed to get Metal device from wgpu".to_string(),
                    ));
                };

                let texture = hal_device
                    .raw_device()
                    .newTextureWithDescriptor_iosurface_plane(
                        metal_desc.as_ref(),
                        io_surface.as_ref(),
                        0,
                    )
                    .ok_or(TextureImportError::InvalidHandle(
                        "Invalid IOSurface handle".to_string(),
                    ))?;

                let hal_tex = <wgpu::wgc::api::Metal as wgpu::hal::Api>::Device::texture_from_raw(
                    texture,
                    texture_desc.format,
                    MTLTextureType::Type2D,
                    texture_desc.array_layer_count(),
                    texture_desc.mip_level_count,
                    wgpu::hal::CopyExtent {
                        width: texture_desc.size.width,
                        height: texture_desc.size.height,
                        depth: texture_desc.array_layer_count(),
                    },
                    None,
                );

                Ok::<_, TextureImportError>(hal_tex)
            }?;
            texture
        };

        Ok(unsafe {
            device.create_texture_from_hal::<wgpu::wgc::api::Metal>(
                hal_tex,
                &texture_desc,
                wgpu::TextureUses::RESOURCE,
            )
        })
    }

    fn is_metal_backend(&self, device: &wgpu::Device) -> bool {
        use wgpu::hal::api;
        unsafe { device.as_hal::<api::Metal>().is_some() }
    }
}
