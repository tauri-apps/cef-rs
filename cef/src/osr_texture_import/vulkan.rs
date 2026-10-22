//! Copy imported Vulkan images while transferring ownership to and from CEF.

use super::common::texture;
use super::{TextureImportError, TextureImportResult};
use ash::vk;
use wgpu::hal::api::Vulkan;

// The Vulkan backend maps this combination of read usages to GENERAL.
pub const EXTERNAL_TEXTURE_STATE: wgpu::TextureUses =
    wgpu::TextureUses::COPY_SRC.union(wgpu::TextureUses::RESOURCE);

/// The source must be an initialized, single-layer, single-mip color image in
/// GENERAL, owned by `external_queue_family`. Its wgpu tracker must start in
/// EXTERNAL_TEXTURE_STATE. The producer must not access it until this returns.
pub fn copy_to_owned(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: wgpu::Texture,
    external_queue_family: u32,
) -> TextureImportResult {
    let (raw_device, local_queue_family, image) = unsafe {
        let hal_device = device.as_hal::<Vulkan>().unwrap();
        let hal_texture = source.as_hal::<Vulkan>().unwrap();
        (
            hal_device.raw_device().clone(),
            hal_device.queue_family_index(),
            hal_texture.raw_handle(),
        )
    };

    let destination = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("CEF owned Vulkan frame"),
        size: source.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: source.format(),
        usage: wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    // wgpu requires separate encoders for raw and tracked commands.
    let acquire = ownership_transfer(
        device,
        &raw_device,
        image,
        external_queue_family,
        local_queue_family,
        true,
    )?;

    let mut copy = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("CEF Vulkan frame copy"),
    });
    copy.copy_texture_to_texture(
        source.as_image_copy(),
        destination.as_image_copy(),
        source.size(),
    );
    copy.transition_resources(
        std::iter::empty(),
        std::iter::once(wgpu::TextureTransition {
            texture: &source,
            selector: None,
            state: EXTERNAL_TEXTURE_STATE,
        }),
    );

    let release = ownership_transfer(
        device,
        &raw_device,
        image,
        local_queue_family,
        external_queue_family,
        false,
    )?;
    let submission = queue.submit([acquire, copy.finish(), release]);
    texture::wait_for_copy(device, submission)?;
    Ok(destination)
}

fn ownership_transfer(
    device: &wgpu::Device,
    raw_device: &ash::Device,
    image: vk::Image,
    from_queue_family: u32,
    to_queue_family: u32,
    acquire: bool,
) -> Result<wgpu::CommandBuffer, TextureImportError> {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some(if acquire {
            "CEF Vulkan ownership acquire"
        } else {
            "CEF Vulkan ownership release"
        }),
    });
    let barrier = vk::ImageMemoryBarrier::default()
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        })
        .src_queue_family_index(from_queue_family)
        .dst_queue_family_index(to_queue_family)
        .old_layout(vk::ImageLayout::GENERAL)
        .new_layout(vk::ImageLayout::GENERAL)
        .src_access_mask(if acquire {
            vk::AccessFlags::empty()
        } else {
            vk::AccessFlags::MEMORY_READ
        })
        .dst_access_mask(if acquire {
            vk::AccessFlags::MEMORY_READ
        } else {
            vk::AccessFlags::empty()
        });

    // Leave GENERAL unchanged to preserve wgpu's layout tracking.
    unsafe {
        encoder.as_hal_mut::<Vulkan, _, _>(|hal_encoder| {
            let hal_encoder = hal_encoder.ok_or_else(|| TextureImportError::PlatformError {
                message: "Failed to record Vulkan external ownership transfer".to_string(),
            })?;
            raw_device.cmd_pipeline_barrier(
                hal_encoder.raw_handle(),
                if acquire {
                    vk::PipelineStageFlags::TOP_OF_PIPE
                } else {
                    vk::PipelineStageFlags::ALL_COMMANDS
                },
                if acquire {
                    vk::PipelineStageFlags::ALL_COMMANDS
                } else {
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE
                },
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
            Ok::<_, TextureImportError>(())
        })?;
    }
    Ok(encoder.finish())
}
