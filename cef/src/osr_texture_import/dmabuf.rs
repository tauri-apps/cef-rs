//! Linux DMA-BUF texture import implementation

use super::common::{format, texture, vulkan};
use super::{TextureImportError, TextureImportResult, TextureImporter};
use crate::{sys::cef_color_type_t, AcceleratedPaintInfo};
use ash::vk;
use wgpu::hal::api;

pub struct DmaBufImporter {
    fds: Vec<std::os::fd::RawFd>,
    format: cef_color_type_t,
    modifier: u64,
    width: u32,
    height: u32,
    strides: Vec<u32>,
    offsets: Vec<u32>,
}

impl TextureImporter for DmaBufImporter {
    fn new(info: &AcceleratedPaintInfo) -> Self {
        Self {
            fds: extract_fds_from_info(info),
            format: *info.format.as_ref(),
            modifier: info.modifier,
            width: info.extra.coded_size.width as u32,
            height: info.extra.coded_size.height as u32,
            strides: extract_strides_from_info(info),
            offsets: extract_offsets_from_info(info),
        }
    }

    fn import_to_wgpu(&self, device: &wgpu::Device) -> TextureImportResult {
        // Try hardware acceleration first
        if self.supports_hardware_acceleration(device) {
            #[cfg(feature = "accelerated_osr_dawn")]
            match self.import_via_dawn(device) {
                Ok(texture) => {
                    tracing::info!("Successfully imported DMA-BUF texture via Dawn");
                    return Ok(texture);
                }
                Err(e) => {
                    tracing::debug!(
                        "Failed to import DMA-BUF via Dawn: {}, trying Vulkan fallback",
                        e
                    );
                }
            }
            match self.import_via_vulkan(device) {
                Ok(texture) => {
                    tracing::info!("Successfully imported DMA-BUF texture via Vulkan");
                    return Ok(texture);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to import DMA-BUF via Vulkan: {}, falling back to CPU texture",
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
            "CEF DMA-BUF Texture (fallback)",
        )
    }

    #[cfg(target_os = "linux")]
    fn supports_hardware_acceleration(&self, device: &wgpu::Device) -> bool {
        // Check if we have valid file descriptors
        if self.fds.is_empty() {
            return false;
        }

        for &fd in &self.fds {
            if fd < 0 {
                return false;
            }
            // Check if file descriptor is valid
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if flags == -1 {
                return false;
            }
        }

        #[cfg(feature = "accelerated_osr_dawn")]
        if dawn_wgpu::Compat::<dawn_rs::Device>::try_from(device).is_ok() {
            return true;
        }

        // Check if wgpu is using Vulkan backend
        vulkan::is_vulkan_backend(device)
    }
}

impl DmaBufImporter {
    #[cfg(feature = "accelerated_osr_dawn")]
    fn drm_fourcc_from_cef(&self) -> Result<u32, TextureImportError> {
        const fn fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
            (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
        }
        match self.format {
            // BGRA in memory usually maps to DRM AR24
            cef_color_type_t::CEF_COLOR_TYPE_BGRA_8888 => Ok(fourcc(b'A', b'R', b'2', b'4')),
            // RGBA in memory usually maps to DRM AB24
            cef_color_type_t::CEF_COLOR_TYPE_RGBA_8888 => Ok(fourcc(b'A', b'B', b'2', b'4')),
            _ => Err(TextureImportError::UnsupportedFormat {
                format: self.format,
            }),
        }
    }

    #[cfg(feature = "accelerated_osr_dawn")]
    fn import_via_dawn(&self, device: &wgpu::Device) -> TextureImportResult {
        use dawn_wgpu::{Compat, CompatTexture};

        let dawn_device = Compat::<dawn_rs::Device>::try_from(device)
            .map_err(|_| TextureImportError::HardwareUnavailable {
                reason: "wgpu device is not backed by dawn-wgpu".into(),
            })?
            .into_inner();
        if !dawn_device.has_feature(dawn_rs::FeatureName::SharedTextureMemoryDmaBuf) {
            return Err(TextureImportError::HardwareUnavailable {
                reason: "SharedTextureMemoryDmaBuf feature is unavailable".into(),
            });
        }

        let mut planes = Vec::with_capacity(self.fds.len());
        for i in 0..self.fds.len() {
            let dup_fd = unsafe { libc::dup(self.fds[i]) };
            if dup_fd < 0 {
                return Err(TextureImportError::PlatformError {
                    message: "Failed to dup DMA-BUF file descriptor".into(),
                });
            }
            let mut plane = dawn_rs::SharedTextureMemoryDmaBufPlane::new();
            plane.fd = Some(dup_fd);
            plane.offset = Some(self.offsets.get(i).copied().unwrap_or(0) as u64);
            plane.stride = Some(self.strides.get(i).copied().unwrap_or(0));
            planes.push(plane);
        }

        let mut desc = dawn_rs::SharedTextureMemoryDmaBufDescriptor::new();
        desc.size = Some(dawn_rs::Extent3D {
            width: Some(self.width),
            height: Some(self.height),
            depth_or_array_layers: Some(1),
        });
        desc.drm_format = Some(self.drm_fourcc_from_cef()?);
        desc.drm_modifier = Some(self.modifier);
        desc.planes = Some(planes);
        let shared_desc = dawn_rs::SharedTextureMemoryDescriptor::new().with_extension(desc.into());
        let shared_memory = dawn_device.import_shared_texture_memory(&shared_desc);

        let mut properties = dawn_rs::SharedTextureMemoryProperties::new();
        let properties_status = shared_memory.get_properties(&mut properties);
        if properties_status != dawn_rs::Status::Success {
            return Err(TextureImportError::PlatformError {
                message: format!(
                    "Dawn DMA-BUF get_properties failed with {:?}",
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
                message: format!("Dawn DMA-BUF begin_access failed with {:?}", status),
            });
        }

        let texture_desc = wgpu::TextureDescriptor {
            label: Some("CEF Dawn DMA-BUF Texture"),
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

    fn import_via_vulkan(&self, device: &wgpu::Device) -> TextureImportResult {
        // Get wgpu's Vulkan instance and device
        use wgpu::{wgc::api::Vulkan, TextureUses};
        let hal_texture = unsafe {
            let hal_device_guard = device.as_hal::<api::Vulkan>();
            let Some(hal_device) = hal_device_guard else {
                return Err(TextureImportError::HardwareUnavailable {
                    reason: "Device is not using Vulkan backend".to_string(),
                });
            };

            // Create VkImage from DMA-BUF using external memory
            let vk_image = self.create_vulkan_image_from_dmabuf(&hal_device)?;

            // Wrap VkImage in wgpu-hal texture
            let hal_texture = <api::Vulkan as wgpu::hal::Api>::Device::texture_from_raw(
                &hal_device,
                vk_image,
                &wgpu::hal::TextureDescriptor {
                    label: Some("CEF DMA-BUF Texture"),
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
                None, // drop_callback,
                wgpu::hal::vulkan::TextureMemory::External,
            );

            Ok::<_, TextureImportError>(hal_texture)
        }?;

        // Import hal texture into wgpu
        let texture = unsafe {
            device.create_texture_from_hal::<Vulkan>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("CEF DMA-BUF Texture"),
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
                TextureUses::RESOURCE,
            )
        };

        Ok(texture)
    }

    fn create_vulkan_image_from_dmabuf(
        &self,
        hal_device: &<api::Vulkan as wgpu::hal::Api>::Device,
    ) -> Result<vk::Image, TextureImportError> {
        // Get raw Vulkan handles
        let device = hal_device.raw_device();
        let _instance = hal_device.shared_instance().raw_instance();

        // Validate dimensions
        if self.width == 0 || self.height == 0 {
            return Err(TextureImportError::InvalidHandle(
                "Invalid DMA-BUF dimensions".to_string(),
            ));
        }

        // Create external memory image
        let image_create_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format::cef_to_vulkan(self.format)?)
            .extent(vk::Extent3D {
                width: self.width,
                height: self.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        // Set up DRM format modifier
        let plane_layouts = self.create_subresource_layouts()?;
        let mut drm_format_modifier = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
            .drm_format_modifier(self.modifier)
            .plane_layouts(&plane_layouts);

        let image_create_info = image_create_info.push_next(&mut drm_format_modifier);

        // Create the image
        let image = unsafe {
            device.create_image(&image_create_info, None).map_err(|e| {
                TextureImportError::VulkanError {
                    operation: format!("Failed to create Vulkan image: {e:?}"),
                }
            })?
        };

        // Import memory from DMA-BUF
        let memory_requirements = unsafe { device.get_image_memory_requirements(image) };

        // Duplicate the file descriptor to avoid ownership issues
        let dup_fd = unsafe { libc::dup(self.fds[0]) };
        if dup_fd == -1 {
            return Err(TextureImportError::PlatformError {
                message: "Failed to duplicate DMA-BUF file descriptor".to_string(),
            });
        }

        let mut import_memory_fd = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .fd(dup_fd);

        // Find a suitable memory type
        let memory_properties = unsafe {
            hal_device
                .shared_instance()
                .raw_instance()
                .get_physical_device_memory_properties(hal_device.raw_physical_device())
        };

        let memory_type_index = vulkan::find_memory_type_index(
            memory_requirements.memory_type_bits,
            vk::MemoryPropertyFlags::empty(),
            &memory_properties,
        )
        .ok_or_else(|| TextureImportError::VulkanError {
            operation: "Failed to find suitable memory type for DMA-BUF".to_string(),
        })?;

        let allocate_info = vk::MemoryAllocateInfo::default()
            .allocation_size(memory_requirements.size)
            .memory_type_index(memory_type_index)
            .push_next(&mut import_memory_fd);

        let device_memory = unsafe {
            device.allocate_memory(&allocate_info, None).map_err(|e| {
                TextureImportError::VulkanError {
                    operation: format!("Failed to allocate memory for DMA-BUF: {e:?}"),
                }
            })?
        };

        // Bind memory to image
        unsafe {
            device
                .bind_image_memory(image, device_memory, 0)
                .map_err(|e| TextureImportError::VulkanError {
                    operation: format!("Failed to bind memory to image: {e:?}"),
                })?;
        }

        Ok(image)
    }

    fn create_subresource_layouts(&self) -> Result<Vec<vk::SubresourceLayout>, TextureImportError> {
        let mut layouts = Vec::new();

        for i in 0..self.fds.len() {
            layouts.push(vk::SubresourceLayout {
                offset: self.offsets.get(i).copied().unwrap_or(0) as u64,
                size: 0, // Will be calculated by driver
                row_pitch: self.strides.get(i).copied().unwrap_or(0) as u64,
                array_pitch: 0,
                depth_pitch: 0,
            });
        }

        Ok(layouts)
    }
}

fn extract_fds_from_info(info: &crate::AcceleratedPaintInfo) -> Vec<std::os::fd::RawFd> {
    let plane_count = info.plane_count as usize;
    let mut fds = Vec::with_capacity(plane_count);

    for i in 0..plane_count {
        if let Some(plane) = info.planes.get(i) {
            fds.push(plane.fd);
        }
    }

    fds
}

fn extract_strides_from_info(info: &crate::AcceleratedPaintInfo) -> Vec<u32> {
    let plane_count = info.plane_count as usize;
    let mut strides = Vec::with_capacity(plane_count);

    for i in 0..plane_count {
        if let Some(plane) = info.planes.get(i) {
            strides.push(plane.stride);
        }
    }

    strides
}

fn extract_offsets_from_info(info: &crate::AcceleratedPaintInfo) -> Vec<u32> {
    let plane_count = info.plane_count as usize;
    let mut offsets = Vec::with_capacity(plane_count);

    for i in 0..plane_count {
        if let Some(plane) = info.planes.get(i) {
            offsets.push(plane.offset as u32);
        }
    }

    offsets
}
