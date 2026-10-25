//! End-to-end check of the Vulkan layer without a window (AC-1.34b, Linux part).
//!
//! Plays a minimal Mumcord: listens on the overlay socket, answers the layer's hello with an
//! enabled config and a scene with one talking person. Then it renders frames to a
//! `VK_EXT_headless_surface` swapchain (cleared to blue), presents them through the loader (and
//! so through the layer, if the loader picks it up), reads a presented image back and checks that the overlay's green talking dot is in the top-left corner.
//!
//! Run with an isolated socket name and a data directory that holds the layer manifest, e.g.
//! `MUMCORD_OVERLAY_SOCKET=test XDG_DATA_HOME=… cargo run -p mumcord-overlay-layer --example headless_check`.
//! Exit code 0: the overlay was drawn.
//!
//! `headless_check serve <seconds>` only plays Mumcord for that long, to watch the overlay in a
//! real program such as `vkcube` started with the same environment. `headless_check render` only
//! renders, against a Mumcord started elsewhere.

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("serve") {
        let seconds = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
        if let Err(e) = linux::serve_one_game() {
            eprintln!("FAILED: {e}");
            std::process::exit(1);
        }
        std::thread::sleep(std::time::Duration::from_secs(seconds));
        return;
    }
    // `render` skips the built-in Mumcord, e.g. to check that a layer inside a container (Steam
    // Linux Runtime) reaches a `serve` running outside it.
    let serve = args.get(1).map(String::as_str) != Some("render");
    match linux::run(serve) {
        Ok(()) => println!("OK: the overlay was drawn into the presented image"),
        Err(e) => {
            eprintln!("FAILED: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Linux only");
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::Write;
    use std::time::{Duration, Instant};

    use ash::vk;
    use mumcord_overlay_ipc::wire::{self, Config, Corner, Message, Person, Scene};

    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 360;

    pub fn serve_one_game() -> Result<(), String> {
        let listener = mumcord_overlay_ipc::endpoint::bind().map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            match wire::read_message(&mut stream) {
                Ok(Message::Hello(h)) => {
                    println!("layer said hello: api {:?}, exe {:?}", h.api, h.exe)
                }
                other => {
                    eprintln!("unexpected first message: {other:?}");
                    return;
                }
            }
            let config = Config {
                enabled: true,
                corner: Corner::TopLeft,
                size_percent: 100,
                opacity_percent: 100,
                show_whole_channel: false,
                show_channel_name: false,
            };
            let scene = Scene {
                channel: String::new(),
                people: vec![Person {
                    name: "Bot".into(),
                    talking: true,
                    muted: false,
                    deafened: false,
                    is_self: false,
                }],
            };
            let _ = stream.write_all(&Message::Config(config).encode());
            let _ = stream.write_all(&Message::Scene(scene).encode());
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if stream.write_all(&Message::Heartbeat.encode()).is_err() {
                    return;
                }
            }
        });
        Ok(())
    }

    fn check(result: Result<impl Sized, vk::Result>, what: &str) -> Result<(), String> {
        result.map(drop).map_err(|e| format!("{what}: {e:?}"))
    }

    pub fn run(serve: bool) -> Result<(), String> {
        if serve {
            serve_one_game()?;
        }
        // Safety: plain Vulkan usage; every object lives until the process exits.
        unsafe {
            let entry = ash::Entry::load().map_err(|e| e.to_string())?;
            let extensions = [
                ash::khr::surface::NAME.as_ptr(),
                ash::ext::headless_surface::NAME.as_ptr(),
            ];
            let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
            let instance = entry
                .create_instance(
                    &vk::InstanceCreateInfo::default()
                        .application_info(&app)
                        .enabled_extension_names(&extensions),
                    None,
                )
                .map_err(|e| format!("create instance: {e:?}"))?;
            let physical = *instance
                .enumerate_physical_devices()
                .map_err(|e| e.to_string())?
                .first()
                .ok_or("no GPU")?;
            let family = instance
                .get_physical_device_queue_family_properties(physical)
                .iter()
                .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
                .ok_or("no graphics queue")? as u32;
            let priorities = [1.0];
            let queues = [vk::DeviceQueueCreateInfo::default()
                .queue_family_index(family)
                .queue_priorities(&priorities)];
            let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];
            let device = instance
                .create_device(
                    physical,
                    &vk::DeviceCreateInfo::default()
                        .queue_create_infos(&queues)
                        .enabled_extension_names(&device_extensions),
                    None,
                )
                .map_err(|e| format!("create device: {e:?}"))?;
            let queue = device.get_device_queue(family, 0);
            let headless = ash::ext::headless_surface::Instance::new(&entry, &instance);
            let surface = headless
                .create_headless_surface(&vk::HeadlessSurfaceCreateInfoEXT::default(), None)
                .map_err(|e| format!("headless surface: {e:?}"))?;
            let surface_fn = ash::khr::surface::Instance::new(&entry, &instance);
            let caps = surface_fn
                .get_physical_device_surface_capabilities(physical, surface)
                .map_err(|e| e.to_string())?;
            let format = surface_fn
                .get_physical_device_surface_formats(physical, surface)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|f| f.format == vk::Format::B8G8R8A8_UNORM)
                .ok_or("no BGRA8 UNORM surface format")?;
            let swapchain_fn = ash::khr::swapchain::Device::new(&instance, &device);
            let swapchain = swapchain_fn
                .create_swapchain(
                    &vk::SwapchainCreateInfoKHR::default()
                        .surface(surface)
                        .min_image_count(caps.min_image_count.max(2))
                        .image_format(format.format)
                        .image_color_space(format.color_space)
                        .image_extent(vk::Extent2D {
                            width: WIDTH,
                            height: HEIGHT,
                        })
                        .image_array_layers(1)
                        .image_usage(
                            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC,
                        )
                        .pre_transform(caps.current_transform)
                        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                        .present_mode(vk::PresentModeKHR::FIFO)
                        .clipped(true),
                    None,
                )
                .map_err(|e| format!("create swapchain: {e:?}"))?;
            let images = swapchain_fn
                .get_swapchain_images(swapchain)
                .map_err(|e| e.to_string())?;
            let pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(|e| e.to_string())?;
            let command = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(pool)
                        .command_buffer_count(1),
                )
                .map_err(|e| e.to_string())?[0];
            let acquired = device
                .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                .map_err(|e| e.to_string())?;
            let rendered = device
                .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                .map_err(|e| e.to_string())?;
            let fence = device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(|e| e.to_string())?;
            let range = vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            };
            let barrier = |image, old, new, src, dst| {
                vk::ImageMemoryBarrier::default()
                    .old_layout(old)
                    .new_layout(new)
                    .src_access_mask(src)
                    .dst_access_mask(dst)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range)
            };

            // Present blue frames for two seconds; the overlay appears once the layer's IPC
            // thread has the scene.
            let started = Instant::now();
            let mut frames = 0;
            while started.elapsed() < Duration::from_secs(2) {
                let (index, _) = swapchain_fn
                    .acquire_next_image(swapchain, u64::MAX, acquired, vk::Fence::null())
                    .map_err(|e| format!("acquire: {e:?}"))?;
                let image = images[index as usize];
                check(
                    device.reset_command_buffer(command, vk::CommandBufferResetFlags::empty()),
                    "reset",
                )?;
                check(
                    device.begin_command_buffer(command, &vk::CommandBufferBeginInfo::default()),
                    "begin",
                )?;
                device.cmd_pipeline_barrier(
                    command,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier(
                        image,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::empty(),
                        vk::AccessFlags::TRANSFER_WRITE,
                    )],
                );
                device.cmd_clear_color_image(
                    command,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearColorValue {
                        float32: [0.0, 0.0, 1.0, 1.0],
                    },
                    &[range],
                );
                device.cmd_pipeline_barrier(
                    command,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier(
                        image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::ImageLayout::PRESENT_SRC_KHR,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::AccessFlags::empty(),
                    )],
                );
                check(device.end_command_buffer(command), "end")?;
                let waits = [acquired];
                let stages = [vk::PipelineStageFlags::TRANSFER];
                let commands = [command];
                let signals = [rendered];
                check(
                    device.queue_submit(
                        queue,
                        &[vk::SubmitInfo::default()
                            .wait_semaphores(&waits)
                            .wait_dst_stage_mask(&stages)
                            .command_buffers(&commands)
                            .signal_semaphores(&signals)],
                        fence,
                    ),
                    "submit",
                )?;
                let swapchains = [swapchain];
                let indices = [index];
                check(
                    swapchain_fn.queue_present(
                        queue,
                        &vk::PresentInfoKHR::default()
                            .wait_semaphores(&signals)
                            .swapchains(&swapchains)
                            .image_indices(&indices),
                    ),
                    "present",
                )?;
                check(device.wait_for_fences(&[fence], true, u64::MAX), "wait")?;
                check(device.reset_fences(&[fence]), "reset fence")?;
                frames += 1;
                std::thread::sleep(Duration::from_millis(16));
            }
            check(device.device_wait_idle(), "idle")?;
            println!("presented {frames} frames");

            // Read back the image presented last (re-acquired; headless images keep their content).
            let (index, _) = swapchain_fn
                .acquire_next_image(swapchain, u64::MAX, vk::Semaphore::null(), fence)
                .map_err(|e| format!("acquire: {e:?}"))?;
            check(device.wait_for_fences(&[fence], true, u64::MAX), "wait")?;
            check(device.reset_fences(&[fence]), "reset fence")?;
            let image = images[index as usize];
            let size = u64::from(WIDTH * HEIGHT * 4);
            let buffer = device
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(size)
                        .usage(vk::BufferUsageFlags::TRANSFER_DST),
                    None,
                )
                .map_err(|e| e.to_string())?;
            let req = device.get_buffer_memory_requirements(buffer);
            let props = instance.get_physical_device_memory_properties(physical);
            let memory_index = (0..props.memory_type_count)
                .find(|&i| {
                    req.memory_type_bits & (1 << i) != 0
                        && props.memory_types[i as usize].property_flags.contains(
                            vk::MemoryPropertyFlags::HOST_VISIBLE
                                | vk::MemoryPropertyFlags::HOST_COHERENT,
                        )
                })
                .ok_or("no host-visible memory")?;
            let memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(memory_index),
                    None,
                )
                .map_err(|e| e.to_string())?;
            check(device.bind_buffer_memory(buffer, memory, 0), "bind")?;
            check(
                device.reset_command_buffer(command, vk::CommandBufferResetFlags::empty()),
                "reset",
            )?;
            check(
                device.begin_command_buffer(command, &vk::CommandBufferBeginInfo::default()),
                "begin",
            )?;
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier(
                    image,
                    vk::ImageLayout::PRESENT_SRC_KHR,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::TRANSFER_READ,
                )],
            );
            device.cmd_copy_image_to_buffer(
                command,
                image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer,
                &[vk::BufferImageCopy::default()
                    .image_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .image_extent(vk::Extent3D {
                        width: WIDTH,
                        height: HEIGHT,
                        depth: 1,
                    })],
            );
            check(device.end_command_buffer(command), "end")?;
            let commands = [command];
            check(
                device.queue_submit(
                    queue,
                    &[vk::SubmitInfo::default().command_buffers(&commands)],
                    fence,
                ),
                "submit",
            )?;
            check(device.wait_for_fences(&[fence], true, u64::MAX), "wait")?;
            let mapped = device
                .map_memory(memory, 0, size, vk::MemoryMapFlags::empty())
                .map_err(|e| e.to_string())?
                .cast::<u8>();
            let pixels = std::slice::from_raw_parts(mapped, size as usize);
            if let Ok(out) = std::env::var("MUMCORD_CHECK_PPM") {
                write_ppm(&out, pixels)?;
            }
            // BGRA. The talking dot sits 13/21 px into the panel, which starts 16 px in.
            let at = |x: u32, y: u32| {
                let i = ((y * WIDTH + x) * 4) as usize;
                (pixels[i + 2], pixels[i + 1], pixels[i])
            };
            let dot = at(16 + 13, 16 + 21);
            let far = at(WIDTH - 20, HEIGHT - 20);
            println!("pixel at the talking dot: rgb{dot:?}, far away: rgb{far:?}");
            if far != (0, 0, 255) {
                return Err("the frame was not cleared to blue".into());
            }
            if !(dot.1 > 150 && dot.2 < 160 && dot.0 < 120) {
                return Err("no overlay at the expected place (is the layer loaded?)".into());
            }
            Ok(())
        }
    }

    fn write_ppm(path: &str, bgra: &[u8]) -> Result<(), String> {
        let mut out = format!("P6 {WIDTH} {HEIGHT} 255\n").into_bytes();
        for px in bgra.as_chunks::<4>().0 {
            out.extend_from_slice(&[px[2], px[1], px[0]]);
        }
        std::fs::write(path, out).map_err(|e| e.to_string())
    }
}
