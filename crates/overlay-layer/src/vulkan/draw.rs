//! Per-swapchain drawing: a render pass that loads the presented image, one pipeline that
//! blends the overlay texture into a viewport rectangle, and per-image command buffers, fences
//! and semaphores.
//!
//! Nothing here waits for the GPU on the present path: a command buffer still in flight skips
//! the overlay for that frame. The overlay image lives in two texture slots: draws show the front
//! one while a new image goes into the back one as soon as no unfinished draw samples it. A game
//! always has earlier frames on the GPU when it presents (DXVK, vkd3d-proton and any GPU-bound
//! game), so waiting for all draws to finish would never upload anything. Allocations happen when
//! the swapchain is created or the overlay image changes size, never per frame.

use std::sync::Arc;

use ash::vk::{self, Handle};

use super::DeviceData;
use crate::log::{Outcome, debug};
use crate::shared::{self, Frame};

const VERTEX_SPV: &[u8] = include_bytes!("../../shaders/overlay.vert.spv");
const FRAGMENT_SPV: &[u8] = include_bytes!("../../shaders/overlay.frag.spv");

/// Whether the swapchain carries HDR10 (BT.2020 primaries, ST 2084 curve), where the overlay
/// needs a colour conversion in the shader.
fn is_hdr10(color_space: vk::ColorSpaceKHR) -> bool {
    color_space == vk::ColorSpaceKHR::HDR10_ST2084_EXT
}

struct PerImage {
    view: vk::ImageView,
    framebuffer: vk::Framebuffer,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    semaphore: vk::Semaphore,
    submitted: bool,
    /// Texture slot the last submitted draw sampled.
    slot: usize,
}

struct Texture {
    width: u32,
    height: u32,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    mapped: *mut u8,
}

struct Slot {
    texture: Option<Texture>,
    set: vk::DescriptorSet,
}

pub struct Renderer {
    extent: vk::Extent2D,
    texture_format: vk::Format,
    render_pass: vk::RenderPass,
    set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    sampler: vk::Sampler,
    hdr10: bool,
    descriptor_pool: vk::DescriptorPool,
    images: Vec<PerImage>,
    /// Command pool and the queue family it belongs to, made on the first present.
    pool: Option<(u32, vk::CommandPool)>,
    slots: [Slot; 2],
    /// The slot draws show.
    front: usize,
    /// Frame generation last taken from the IPC thread.
    seen: u64,
    frame: Option<Arc<Frame>>,
    /// `frame` still has to be copied into the texture.
    needs_upload: bool,
}

// The mapped staging pointer is only used by the thread holding the layer's state lock.
unsafe impl Send for Renderer {}

/// Whether the swapchain stores linear values, so the sRGB-encoded overlay must be sampled
/// through an sRGB view to look right: the `_SRGB` formats and the float format HDR games and
/// DXVK/vkd3d-proton use for scRGB output.
fn is_linear_target(format: vk::Format) -> bool {
    matches!(
        format,
        vk::Format::B8G8R8A8_SRGB
            | vk::Format::R8G8B8A8_SRGB
            | vk::Format::A8B8G8R8_SRGB_PACK32
            | vk::Format::R16G16B16A16_SFLOAT
    )
}

/// The texture slot a new overlay image goes into: the one not on screen, once no unfinished
/// draw (given by the slot it sampled) uses it. Draws of the current image never hold it up.
fn upload_slot(front: usize, unfinished: impl IntoIterator<Item = usize>) -> Option<usize> {
    let back = 1 - front;
    unfinished
        .into_iter()
        .all(|slot| slot != back)
        .then_some(back)
}

fn memory_type(
    props: &vk::PhysicalDeviceMemoryProperties,
    bits: u32,
    flags: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| {
        bits & (1 << i) != 0
            && props.memory_types[i as usize]
                .property_flags
                .contains(flags)
    })
}

fn spirv(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect()
}

impl Renderer {
    /// Builds everything that only depends on the swapchain. `None` (no overlay for this
    /// swapchain) on any failure; partial objects are released.
    pub fn new(
        d: &DeviceData,
        device: vk::Device,
        swapchain: vk::SwapchainKHR,
        info: &vk::SwapchainCreateInfoKHR<'_>,
    ) -> Option<Self> {
        if matches!(
            info.present_mode,
            vk::PresentModeKHR::SHARED_DEMAND_REFRESH
                | vk::PresentModeKHR::SHARED_CONTINUOUS_REFRESH
        ) {
            return None;
        }
        let mut renderer = Self {
            extent: info.image_extent,
            texture_format: if is_linear_target(info.image_format) {
                vk::Format::R8G8B8A8_SRGB
            } else {
                vk::Format::R8G8B8A8_UNORM
            },
            render_pass: vk::RenderPass::null(),
            set_layout: vk::DescriptorSetLayout::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            sampler: vk::Sampler::null(),
            hdr10: is_hdr10(info.image_color_space),
            descriptor_pool: vk::DescriptorPool::null(),
            images: Vec::new(),
            pool: None,
            slots: [
                Slot {
                    texture: None,
                    set: vk::DescriptorSet::null(),
                },
                Slot {
                    texture: None,
                    set: vk::DescriptorSet::null(),
                },
            ],
            front: 0,
            seen: 0,
            frame: None,
            needs_upload: false,
        };
        // Safety: all handles come from this device and are destroyed in `destroy`.
        match unsafe { renderer.build(d, device, swapchain, info.image_format) } {
            Ok(()) => Some(renderer),
            Err(e) => {
                debug!(
                    "no overlay for this swapchain: setting up drawing failed ({})",
                    e.as_raw()
                );
                renderer.destroy(d);
                None
            }
        }
    }

    unsafe fn build(
        &mut self,
        d: &DeviceData,
        device: vk::Device,
        swapchain: vk::SwapchainKHR,
        format: vk::Format,
    ) -> Result<(), vk::Result> {
        let fns = &d.fns;
        let attachment = [vk::AttachmentDescription::default()
            .format(format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];
        let color_ref = [vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let subpass = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color_ref)];
        let dependency = [vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            )];
        let pass_info = vk::RenderPassCreateInfo::default()
            .attachments(&attachment)
            .subpasses(&subpass)
            .dependencies(&dependency);
        self.render_pass = unsafe { fns.create_render_pass(&pass_info, None)? };

        let binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        self.set_layout = unsafe {
            fns.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding),
                None,
            )?
        };
        let layouts = [self.set_layout];
        self.pipeline_layout = unsafe {
            fns.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts),
                None,
            )?
        };
        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(2)];
        self.descriptor_pool = unsafe {
            fns.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(2)
                    .pool_sizes(&pool_sizes),
                None,
            )?
        };
        let layouts = [self.set_layout; 2];
        let sets = unsafe {
            fns.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.descriptor_pool)
                    .set_layouts(&layouts),
            )?
        };
        for (slot, set) in self.slots.iter_mut().zip(sets) {
            slot.set = set;
        }
        // Nearest filtering: the overlay is drawn 1:1, pixel for pixel.
        self.sampler = unsafe {
            fns.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::NEAREST)
                    .min_filter(vk::Filter::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                None,
            )?
        };
        self.pipeline = unsafe { self.create_pipeline(d)? };

        let mut count = 0;
        unsafe {
            (d.swapchain.get_swapchain_images_khr)(
                device,
                swapchain,
                &mut count,
                std::ptr::null_mut(),
            )
            .result()?;
        }
        let mut images = vec![vk::Image::null(); count as usize];
        unsafe {
            (d.swapchain.get_swapchain_images_khr)(
                device,
                swapchain,
                &mut count,
                images.as_mut_ptr(),
            )
            .result()?;
        }
        images.truncate(count as usize);
        for image in images {
            let view = unsafe {
                fns.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(color_range()),
                    None,
                )?
            };
            let mut per = PerImage {
                view,
                framebuffer: vk::Framebuffer::null(),
                command: vk::CommandBuffer::null(),
                fence: vk::Fence::null(),
                semaphore: vk::Semaphore::null(),
                submitted: false,
                slot: 0,
            };
            let attachments = [view];
            let result = unsafe {
                fns.create_framebuffer(
                    &vk::FramebufferCreateInfo::default()
                        .render_pass(self.render_pass)
                        .attachments(&attachments)
                        .width(self.extent.width)
                        .height(self.extent.height)
                        .layers(1),
                    None,
                )
                .map(|f| per.framebuffer = f)
                .and_then(|()| {
                    fns.create_fence(&vk::FenceCreateInfo::default(), None)
                        .map(|f| per.fence = f)
                })
                .and_then(|()| {
                    fns.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                        .map(|s| per.semaphore = s)
                })
            };
            self.images.push(per);
            result?;
        }
        Ok(())
    }

    unsafe fn create_pipeline(&self, d: &DeviceData) -> Result<vk::Pipeline, vk::Result> {
        let fns = &d.fns;
        let vertex_code = spirv(VERTEX_SPV);
        let fragment_code = spirv(FRAGMENT_SPV);
        let vertex = unsafe {
            fns.create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&vertex_code),
                None,
            )?
        };
        let fragment = match unsafe {
            fns.create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&fragment_code),
                None,
            )
        } {
            Ok(m) => m,
            Err(e) => {
                unsafe { fns.destroy_shader_module(vertex, None) };
                return Err(e);
            }
        };
        // Selects the HDR10 conversion in the fragment shader (`HDR10`, constant id 0).
        let hdr10 = u32::from(self.hdr10).to_ne_bytes();
        let spec_entry = [vk::SpecializationMapEntry::default()
            .constant_id(0)
            .offset(0)
            .size(hdr10.len())];
        let spec = vk::SpecializationInfo::default()
            .map_entries(&spec_entry)
            .data(&hdr10);
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vertex)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fragment)
                .name(c"main")
                .specialization_info(&spec),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachment);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.pipeline_layout)
            .render_pass(self.render_pass)
            .subpass(0)];
        let result =
            unsafe { fns.create_graphics_pipelines(vk::PipelineCache::null(), &info, None) };
        unsafe {
            fns.destroy_shader_module(vertex, None);
            fns.destroy_shader_module(fragment, None);
        }
        result.map(|p| p[0]).map_err(|(_, e)| e)
    }

    /// Releases everything; waits (bounded) for draws still in flight. Called when the
    /// swapchain is destroyed, not on the present path.
    pub fn destroy(mut self, d: &DeviceData) {
        let fns = &d.fns;
        let fences: Vec<vk::Fence> = self
            .images
            .iter()
            .filter(|p| p.submitted)
            .map(|p| p.fence)
            .collect();
        // Safety: every handle below was created from this device and is no longer used.
        unsafe {
            if !fences.is_empty() {
                // Bounded wait: one second is far beyond any overlay draw.
                let _ = fns.wait_for_fences(&fences, true, 1_000_000_000);
            }
            for slot in 0..self.slots.len() {
                self.destroy_texture(d, slot);
            }
            for per in self.images.drain(..) {
                fns.destroy_semaphore(per.semaphore, None);
                fns.destroy_fence(per.fence, None);
                fns.destroy_framebuffer(per.framebuffer, None);
                fns.destroy_image_view(per.view, None);
            }
            if let Some((_, pool)) = self.pool.take() {
                fns.destroy_command_pool(pool, None);
            }
            fns.destroy_pipeline(self.pipeline, None);
            fns.destroy_sampler(self.sampler, None);
            fns.destroy_descriptor_pool(self.descriptor_pool, None);
            fns.destroy_pipeline_layout(self.pipeline_layout, None);
            fns.destroy_descriptor_set_layout(self.set_layout, None);
            fns.destroy_render_pass(self.render_pass, None);
        }
    }

    unsafe fn destroy_texture(&mut self, d: &DeviceData, slot: usize) {
        if let Some(t) = self.slots[slot].texture.take() {
            let fns = &d.fns;
            unsafe {
                fns.unmap_memory(t.staging_memory);
                fns.destroy_buffer(t.staging, None);
                fns.free_memory(t.staging_memory, None);
                fns.destroy_image_view(t.view, None);
                fns.destroy_image(t.image, None);
                fns.free_memory(t.memory, None);
            }
        }
    }

    /// Whether every submitted draw has finished (non-blocking).
    fn all_idle(&self, d: &DeviceData) -> bool {
        self.images
            .iter()
            .filter(|p| p.submitted)
            .all(|p| unsafe { d.fns.get_fence_status(p.fence) } == Ok(true))
    }

    /// Takes a newer frame from the IPC thread if there is one (never waits).
    fn refresh_frame(&mut self) {
        let generation = shared::generation();
        if generation == self.seen {
            return;
        }
        if let Ok(frame) = shared::try_frame() {
            self.seen = generation;
            self.needs_upload = frame.is_some();
            self.frame = frame;
        }
    }

    unsafe fn ensure_pool(&mut self, d: &DeviceData, family: u32) -> Option<()> {
        if self.pool.is_some_and(|(f, _)| f == family) {
            return Some(());
        }
        if !self.all_idle(d) {
            return None;
        }
        let fns = &d.fns;
        if let Some((_, old)) = self.pool.take() {
            unsafe { fns.destroy_command_pool(old, None) };
            for per in &mut self.images {
                per.command = vk::CommandBuffer::null();
                per.submitted = false;
            }
        }
        let pool = unsafe {
            fns.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
            .ok()?
        };
        self.pool = Some((family, pool));
        let count = u32::try_from(self.images.len()).ok()?;
        let commands = unsafe {
            fns.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(count),
            )
            .ok()?
        };
        for (per, command) in self.images.iter_mut().zip(commands) {
            // Command buffers made by a layer need the loader's dispatch pointer.
            let ok = unsafe {
                (d.set_loader_data)(fns.handle(), command.as_raw() as *mut std::ffi::c_void)
            };
            if ok != vk::Result::SUCCESS {
                return None;
            }
            per.command = command;
        }
        Some(())
    }

    /// Makes the texture in `slot` fit `frame` and copies its pixels into the slot's staging
    /// buffer. Only called while no draw in flight uses that slot.
    unsafe fn stage(&mut self, d: &DeviceData, slot: usize, frame: &Frame) -> Option<()> {
        let (w, h) = (frame.image.width, frame.image.height);
        if self.slots[slot]
            .texture
            .as_ref()
            .is_none_or(|t| t.width != w || t.height != h)
        {
            unsafe {
                self.destroy_texture(d, slot);
                self.slots[slot].texture = Some(self.create_texture(d, w, h)?);
            }
        }
        let slot = &self.slots[slot];
        let texture = slot.texture.as_ref()?;
        let bytes = &frame.image.pixels;
        // The staging buffer is exactly width × height × 4 bytes, as is the image.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), texture.mapped, bytes.len()) };
        let image_info = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(texture.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let write = [vk::WriteDescriptorSet::default()
            .dst_set(slot.set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info)];
        unsafe { d.fns.update_descriptor_sets(&write, &[]) };
        Some(())
    }

    unsafe fn create_texture(&self, d: &DeviceData, width: u32, height: u32) -> Option<Texture> {
        let fns = &d.fns;
        let size = u64::from(width) * u64::from(height) * 4;
        // Created step by step; on failure the earlier objects are released below.
        let mut t = Texture {
            width,
            height,
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            staging: vk::Buffer::null(),
            staging_memory: vk::DeviceMemory::null(),
            mapped: std::ptr::null_mut(),
        };
        let result = (|| unsafe {
            t.image = fns
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(self.texture_format)
                        .extent(vk::Extent3D {
                            width,
                            height,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .ok()?;
            let req = fns.get_image_memory_requirements(t.image);
            let index = memory_type(
                &d.memory,
                req.memory_type_bits,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )
            .or_else(|| {
                memory_type(
                    &d.memory,
                    req.memory_type_bits,
                    vk::MemoryPropertyFlags::empty(),
                )
            })?;
            t.memory = fns
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(index),
                    None,
                )
                .ok()?;
            fns.bind_image_memory(t.image, t.memory, 0).ok()?;
            t.view = fns
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(t.image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(self.texture_format)
                        .subresource_range(color_range()),
                    None,
                )
                .ok()?;
            t.staging = fns
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(size)
                        .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )
                .ok()?;
            let req = fns.get_buffer_memory_requirements(t.staging);
            let index = memory_type(
                &d.memory,
                req.memory_type_bits,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )?;
            t.staging_memory = fns
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(index),
                    None,
                )
                .ok()?;
            fns.bind_buffer_memory(t.staging, t.staging_memory, 0)
                .ok()?;
            t.mapped = fns
                .map_memory(t.staging_memory, 0, size, vk::MemoryMapFlags::empty())
                .ok()?
                .cast();
            Some(())
        })();
        if result.is_some() {
            return Some(t);
        }
        unsafe {
            if !t.mapped.is_null() {
                fns.unmap_memory(t.staging_memory);
            }
            fns.destroy_buffer(t.staging, None);
            fns.free_memory(t.staging_memory, None);
            fns.destroy_image_view(t.view, None);
            fns.destroy_image(t.image, None);
            fns.free_memory(t.memory, None);
        }
        None
    }

    /// Records and submits the overlay draw for swapchain image `index`, waiting on the game's
    /// `waits`. Returns the semaphore the present must wait on, or why nothing was submitted
    /// (the game's semaphores are then untouched).
    pub fn draw(
        &mut self,
        d: &DeviceData,
        queue: vk::Queue,
        family: u32,
        index: u32,
        waits: &[vk::Semaphore],
    ) -> Result<vk::Semaphore, Outcome> {
        const MAX_WAITS: usize = 16;
        if waits.len() > MAX_WAITS {
            return Err(Outcome::TooManyWaits);
        }
        self.refresh_frame();
        let frame = self.frame.clone().ok_or(Outcome::NothingToShow)?;
        let per_index = index as usize;
        let per = self.images.get(per_index).ok_or(Outcome::NoRenderer)?;
        if per.submitted && unsafe { d.fns.get_fence_status(per.fence) } != Ok(true) {
            return Err(Outcome::ImageBusy);
        }
        unsafe { self.ensure_pool(d, family) }.ok_or(Outcome::CommandPoolFailed)?;
        let mut upload = false;
        if self.needs_upload {
            let unfinished = self
                .images
                .iter()
                .filter(|p| p.submitted && unsafe { d.fns.get_fence_status(p.fence) } != Ok(true))
                .map(|p| p.slot);
            if let Some(slot) = upload_slot(self.front, unfinished) {
                unsafe { self.stage(d, slot, &frame) }.ok_or(Outcome::UploadFailed)?;
                self.front = slot;
                self.needs_upload = false;
                upload = true;
            }
        }
        let slot = &self.slots[self.front];
        let texture = slot.texture.as_ref().ok_or(Outcome::WaitingForUpload)?;
        let (w, h) = (texture.width, texture.height);
        // While a newer image waits for upload, the texture still shows the previous frame.
        let (x, y) = shared::place(
            frame.corner,
            frame.margin,
            (w, h),
            (self.extent.width, self.extent.height),
        )
        .ok_or(Outcome::DoesNotFit)?;
        let x_offset = i32::try_from(x).map_err(|_| Outcome::DoesNotFit)?;
        let y_offset = i32::try_from(y).map_err(|_| Outcome::DoesNotFit)?;
        let per = &mut self.images[per_index];
        let fns = &d.fns;
        // Safety: the command buffer and fence are idle (checked above) and belong to `d`.
        unsafe {
            if per.submitted {
                fns.reset_fences(&[per.fence])
                    .map_err(|_| Outcome::RecordFailed)?;
                per.submitted = false;
            }
            fns.reset_command_buffer(per.command, vk::CommandBufferResetFlags::empty())
                .map_err(|_| Outcome::RecordFailed)?;
            fns.begin_command_buffer(
                per.command,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .map_err(|_| Outcome::RecordFailed)?;
            if upload {
                record_upload(fns, per.command, texture);
            }
            let clear = [];
            fns.cmd_begin_render_pass(
                per.command,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.render_pass)
                    .framebuffer(per.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: self.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
            fns.cmd_bind_pipeline(per.command, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            fns.cmd_bind_descriptor_sets(
                per.command,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[slot.set],
                &[],
            );
            // Screen coordinates are far below f32 precision limits.
            #[allow(clippy::cast_precision_loss)]
            let viewport = [vk::Viewport {
                x: x as f32,
                y: y as f32,
                width: w as f32,
                height: h as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }];
            fns.cmd_set_viewport(per.command, 0, &viewport);
            let scissor = [vk::Rect2D {
                offset: vk::Offset2D {
                    x: x_offset,
                    y: y_offset,
                },
                extent: vk::Extent2D {
                    width: w,
                    height: h,
                },
            }];
            fns.cmd_set_scissor(per.command, 0, &scissor);
            fns.cmd_draw(per.command, 3, 1, 0, 0);
            fns.cmd_end_render_pass(per.command);
            fns.end_command_buffer(per.command)
                .map_err(|_| Outcome::RecordFailed)?;

            let stages = [vk::PipelineStageFlags::ALL_COMMANDS; MAX_WAITS];
            let commands = [per.command];
            let signals = [per.semaphore];
            let submit = [vk::SubmitInfo::default()
                .wait_semaphores(waits)
                .wait_dst_stage_mask(&stages[..waits.len()])
                .command_buffers(&commands)
                .signal_semaphores(&signals)];
            fns.queue_submit(queue, &submit, per.fence)
                .map_err(|_| Outcome::SubmitFailed)?;
            per.submitted = true;
            per.slot = self.front;
        }
        Ok(per.semaphore)
    }
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

unsafe fn record_upload(fns: &ash::Device, command: vk::CommandBuffer, texture: &Texture) {
    let to_transfer = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(texture.image)
        .subresource_range(color_range())];
    let region = [vk::BufferImageCopy::default()
        .image_subresource(vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        })
        .image_extent(vk::Extent3D {
            width: texture.width,
            height: texture.height,
            depth: 1,
        })];
    let to_shader = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(texture.image)
        .subresource_range(color_range())];
    // Safety: the caller records into a command buffer in the recording state.
    unsafe {
        fns.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_transfer,
        );
        fns.cmd_copy_buffer_to_image(
            command,
            texture.staging,
            texture.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &region,
        );
        fns.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_shader,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_images_go_to_the_back_slot_while_the_front_one_is_on_the_gpu() {
        // A game always has earlier frames in flight; those show the front slot.
        assert_eq!(upload_slot(0, [0, 0, 0]), Some(1));
        assert_eq!(upload_slot(1, [1]), Some(0));
        assert_eq!(upload_slot(0, []), Some(1));
        // The back slot is only reused once nothing on the GPU samples it any more.
        assert_eq!(upload_slot(0, [0, 1]), None);
    }

    #[test]
    fn only_the_st2084_colour_space_is_hdr10() {
        assert!(is_hdr10(vk::ColorSpaceKHR::HDR10_ST2084_EXT));
        assert!(!is_hdr10(vk::ColorSpaceKHR::SRGB_NONLINEAR));
        assert!(!is_hdr10(vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT));
        assert!(!is_hdr10(vk::ColorSpaceKHR::HDR10_HLG_EXT));
    }

    #[test]
    fn linear_swapchains_sample_the_overlay_as_srgb() {
        assert!(is_linear_target(vk::Format::B8G8R8A8_SRGB));
        assert!(is_linear_target(vk::Format::R16G16B16A16_SFLOAT));
        assert!(!is_linear_target(vk::Format::B8G8R8A8_UNORM));
        assert!(!is_linear_target(vk::Format::A2B10G10R10_UNORM_PACK32));
    }
}
