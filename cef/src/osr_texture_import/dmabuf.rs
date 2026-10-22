//! Linux DMA-BUF texture import implementation.

use super::common::{format, vulkan};
use super::{TextureImportError, TextureImportResult, TextureImporter};
use crate::{sys::cef_color_type_t, AcceleratedPaintInfo};
use ash::vk;
use std::os::fd::{AsRawFd, BorrowedFd, IntoRawFd, RawFd};
use wgpu::hal::api;

struct Plane {
    fd: RawFd,
    stride: u32,
    offset: u64,
}

pub struct DmaBufImporter {
    planes: Vec<Plane>,
    format: cef_color_type_t,
    modifier: u64,
    width: u32,
    height: u32,
}

impl TextureImporter for DmaBufImporter {
    fn new(info: &AcceleratedPaintInfo) -> Self {
        let planes = info
            .planes
            .get(..info.plane_count as usize)
            .unwrap_or_default()
            .iter()
            .map(|plane| Plane {
                fd: plane.fd,
                stride: plane.stride,
                offset: plane.offset,
            })
            .collect();

        Self {
            planes,
            format: *info.format.as_ref(),
            modifier: info.modifier,
            width: info.extra.coded_size.width as u32,
            height: info.extra.coded_size.height as u32,
        }
    }

    unsafe fn import_to_wgpu(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> TextureImportResult {
        self.validate_planes()?;
        let max_dimension = device.limits().max_texture_dimension_2d;
        if self.width > max_dimension || self.height > max_dimension {
            return Err(TextureImportError::InvalidHandle(format!(
                "DMA-BUF dimensions {}x{} exceed the device limit of {max_dimension}",
                self.width, self.height
            )));
        }
        if !device
            .features()
            .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF)
        {
            return Err(TextureImportError::HardwareUnavailable {
                reason: "The wgpu device requires VULKAN_EXTERNAL_MEMORY_DMA_BUF".into(),
            });
        }

        unsafe { self.import_via_vulkan(device, queue) }
    }

    fn supports_hardware_acceleration(&self, device: &wgpu::Device) -> bool {
        self.validate_planes().is_ok()
            && self.width <= device.limits().max_texture_dimension_2d
            && self.height <= device.limits().max_texture_dimension_2d
            && device
                .features()
                .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF)
            && vulkan::is_vulkan_backend(device)
    }
}

impl DmaBufImporter {
    fn validate_planes(&self) -> Result<(), TextureImportError> {
        if self.width == 0 || self.height == 0 || self.planes.is_empty() {
            return Err(TextureImportError::InvalidHandle(
                "Invalid DMA-BUF dimensions or plane count".into(),
            ));
        }

        // Auxiliary planes must share the allocation bound below.
        let mut allocation = None;
        for plane in &self.planes {
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            if plane.fd < 0 || plane.stride == 0 {
                return Err(TextureImportError::InvalidHandle(
                    "Invalid DMA-BUF descriptor or stride".into(),
                ));
            }
            if unsafe { libc::fstat(plane.fd, stat.as_mut_ptr()) } != 0 {
                return Err(TextureImportError::InvalidHandle(format!(
                    "Invalid DMA-BUF descriptor: {}",
                    std::io::Error::last_os_error()
                )));
            }
            let stat = unsafe { stat.assume_init() };
            let identity = (stat.st_dev, stat.st_ino);
            if allocation.is_some_and(|allocation| allocation != identity) {
                return Err(TextureImportError::HardwareUnavailable {
                    reason: "DMA-BUF planes backed by separate allocations require disjoint image bindings".into(),
                });
            }
            allocation = Some(identity);
        }
        Ok(())
    }

    unsafe fn import_via_vulkan(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> TextureImportResult {
        let wgpu_format = format::cef_to_wgpu(self.format)?;
        let size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };

        let (hal_texture, external_queue_family) = {
            let hal_device = unsafe { device.as_hal::<api::Vulkan>() }.ok_or_else(|| {
                TextureImportError::HardwareUnavailable {
                    reason: "Device is not using the Vulkan backend".into(),
                }
            })?;
            let external_queue_family = if hal_device
                .enabled_device_extensions()
                .contains(&ash::ext::queue_family_foreign::NAME)
            {
                vk::QUEUE_FAMILY_FOREIGN_EXT
            } else {
                vk::QUEUE_FAMILY_EXTERNAL
            };
            let mut imported = unsafe { self.create_vulkan_image_from_dmabuf(&hal_device) }?;
            let texture = unsafe {
                hal_device.texture_from_raw(
                    imported.image,
                    &wgpu::hal::TextureDescriptor {
                        label: Some("CEF DMA-BUF callback texture"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu_format,
                        usage: wgpu::TextureUses::COPY_SRC | wgpu::TextureUses::RESOURCE,
                        memory_flags: wgpu::hal::MemoryFlags::empty(),
                        view_formats: vec![],
                    },
                    None,
                    wgpu::hal::vulkan::TextureMemory::Dedicated(imported.memory),
                )
            };
            // wgpu-hal now owns both the VkImage and its imported allocation.
            imported.image = vk::Image::null();
            imported.memory = vk::DeviceMemory::null();
            (texture, external_queue_family)
        };

        let source = unsafe {
            device.create_texture_from_hal::<api::Vulkan>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("CEF DMA-BUF callback texture"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu_format,
                    usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                super::vulkan::EXTERNAL_TEXTURE_STATE,
            )
        };

        super::vulkan::copy_to_owned(device, queue, source, external_queue_family)
    }

    unsafe fn create_vulkan_image_from_dmabuf(
        &self,
        hal_device: &<api::Vulkan as wgpu::hal::Api>::Device,
    ) -> Result<ImportedImage, TextureImportError> {
        let device = hal_device.raw_device();
        let plane_layouts: Vec<_> = self
            .planes
            .iter()
            .map(|plane| vk::SubresourceLayout {
                offset: plane.offset,
                row_pitch: u64::from(plane.stride),
                ..Default::default()
            })
            .collect();
        let mut modifier = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
            .drm_format_modifier(self.modifier)
            .plane_layouts(&plane_layouts);
        let mut external_memory = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let image_info = vk::ImageCreateInfo::default()
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
            .usage(vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .push_next(&mut modifier)
            .push_next(&mut external_memory);

        let image = unsafe { device.create_image(&image_info, None) }
            .map_err(|error| vulkan_error("create DMA-BUF image", error))?;
        let mut imported = ImportedImage {
            device: device.clone(),
            image,
            memory: vk::DeviceMemory::null(),
        };
        let requirements = unsafe { device.get_image_memory_requirements(image) };

        // Vulkan takes ownership of the fd only if allocation succeeds.
        let fd = unsafe { BorrowedFd::borrow_raw(self.planes[0].fd) }
            .try_clone_to_owned()
            .map_err(|error| TextureImportError::PlatformError {
                message: format!("Failed to duplicate DMA-BUF descriptor: {error}"),
            })?;
        let external_fd = ash::khr::external_memory_fd::Device::new(
            hal_device.shared_instance().raw_instance(),
            device,
        );
        let mut fd_properties = vk::MemoryFdPropertiesKHR::default();
        unsafe {
            external_fd.get_memory_fd_properties(
                vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
                fd.as_raw_fd(),
                &mut fd_properties,
            )
        }
        .map_err(|error| vulkan_error("query DMA-BUF memory properties", error))?;
        let memory_properties = unsafe {
            hal_device
                .shared_instance()
                .raw_instance()
                .get_physical_device_memory_properties(hal_device.raw_physical_device())
        };
        let memory_type_index = vulkan::find_memory_type_index(
            requirements.memory_type_bits & fd_properties.memory_type_bits,
            vk::MemoryPropertyFlags::empty(),
            &memory_properties,
        )
        .ok_or_else(|| TextureImportError::VulkanError {
            operation: "No compatible memory type for DMA-BUF".into(),
        })?;

        let mut import_fd = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .fd(fd.as_raw_fd());
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let allocate_info = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type_index)
            .push_next(&mut import_fd)
            .push_next(&mut dedicated);
        imported.memory = unsafe { device.allocate_memory(&allocate_info, None) }
            .map_err(|error| vulkan_error("import DMA-BUF memory", error))?;
        let _ = fd.into_raw_fd();

        unsafe { device.bind_image_memory(image, imported.memory, 0) }
            .map_err(|error| vulkan_error("bind DMA-BUF memory", error))?;
        Ok(imported)
    }
}

/// Owns partial imports until both Vulkan objects are transferred to wgpu-hal.
struct ImportedImage {
    device: ash::Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
}

impl Drop for ImportedImage {
    fn drop(&mut self) {
        unsafe {
            if self.image != vk::Image::null() {
                self.device.destroy_image(self.image, None);
            }
            if self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
            }
        }
    }
}

fn vulkan_error(operation: &str, error: vk::Result) -> TextureImportError {
    TextureImportError::VulkanError {
        operation: format!("Failed to {operation}: {error:?}"),
    }
}
