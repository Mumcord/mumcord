//! The Vulkan layer `VK_LAYER_MUMCORD_overlay` (loader interface version 2).
//!
//! It intercepts only what it needs: instance and device creation (to find the next layer's
//! functions), device queues (to know a queue's family), swapchain creation (to add the
//! colour-attachment usage and build the per-swapchain renderer) and `vkQueuePresentKHR`, where
//! it records one small draw on top of the image before handing it on. Everything else goes
//! straight to the next layer.
//!
//! Safety rules: every entry point catches panics; any failure means "draw nothing" and the
//! call still goes through, so the game never notices the layer.

mod draw;

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::sync::{Arc, Mutex, MutexGuard};

use ash::vk::{self, Handle};

use crate::log::{self, Outcome, debug};
use crate::shared;
use draw::Renderer;

/// Layer name; must match the manifest Mumcord installs.
pub const LAYER_NAME: &str = "VK_LAYER_MUMCORD_overlay";

const LOADER_INSTANCE_CREATE_INFO: i32 = 47;
const LOADER_DEVICE_CREATE_INFO: i32 = 48;
const LAYER_LINK_INFO: i32 = 0;
const LOADER_DATA_CALLBACK: i32 = 1;
const LAYER_NEGOTIATE_INTERFACE_STRUCT: i32 = 1;

#[repr(C)]
struct LayerInstanceLink {
    next: *mut LayerInstanceLink,
    next_get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    next_get_physical_device_proc_addr: *const c_void,
}

/// `VkLayerInstanceCreateInfo`; only the first union member is ever read.
#[repr(C)]
struct LayerInstanceCreateInfo {
    s_type: i32,
    p_next: *const c_void,
    function: i32,
    u: *mut c_void,
}

#[repr(C)]
struct LayerDeviceLink {
    next: *mut LayerDeviceLink,
    next_get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    next_get_device_proc_addr: vk::PFN_vkGetDeviceProcAddr,
}

#[repr(C)]
struct LayerDeviceCreateInfo {
    s_type: i32,
    p_next: *const c_void,
    function: i32,
    u: *mut c_void,
}

pub type SetDeviceLoaderData = unsafe extern "system" fn(vk::Device, *mut c_void) -> vk::Result;

#[repr(C)]
pub struct NegotiateLayerInterface {
    s_type: i32,
    p_next: *mut c_void,
    loader_layer_interface_version: u32,
    pfn_get_instance_proc_addr: Option<vk::PFN_vkGetInstanceProcAddr>,
    pfn_get_device_proc_addr: Option<vk::PFN_vkGetDeviceProcAddr>,
    pfn_get_physical_device_proc_addr: *const c_void,
}

struct InstanceData {
    handle: vk::Instance,
    gipa: vk::PFN_vkGetInstanceProcAddr,
    fns: ash::InstanceFnV1_0,
}

pub struct DeviceData {
    pub fns: ash::Device,
    pub swapchain: ash::khr::swapchain::DeviceFn,
    pub set_loader_data: SetDeviceLoaderData,
    pub memory: vk::PhysicalDeviceMemoryProperties,
    families: Vec<vk::QueueFamilyProperties>,
    gdpa: vk::PFN_vkGetDeviceProcAddr,
    /// Queue handle → queue family index.
    queues: HashMap<u64, u32>,
}

#[derive(Default)]
struct State {
    instances: HashMap<usize, Arc<InstanceData>>,
    devices: HashMap<usize, Arc<Mutex<DeviceData>>>,
    /// Swapchain handle → renderer (`None`: drawing unsupported for it).
    swapchains: HashMap<u64, Option<Renderer>>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state() -> MutexGuard<'static, Option<State>> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = state();
    f(guard.get_or_insert_with(State::default))
}

/// Runs `f`, turning a panic into `fallback`.
fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// The loader's dispatch table pointer at the start of every dispatchable handle; equal for an
/// instance and its physical devices, and for a device and its queues and command buffers.
unsafe fn key<H: Handle>(handle: H) -> usize {
    // Every dispatchable Vulkan handle points at its dispatch table pointer.
    unsafe { *(handle.as_raw() as *const usize) }
}

fn device_of(dispatch_key: usize) -> Option<Arc<Mutex<DeviceData>>> {
    with_state(|s| s.devices.get(&dispatch_key).cloned())
}

/// Loader entry point (manifest `functions.vkNegotiateLoaderLayerInterfaceVersion`).
///
/// # Safety
/// Called by the Vulkan loader with a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn mumcord_vkNegotiateLoaderLayerInterfaceVersion(
    info: *mut NegotiateLayerInterface,
) -> vk::Result {
    guard(vk::Result::ERROR_INITIALIZATION_FAILED, || {
        // The loader passes a valid struct.
        let Some(info) = (unsafe { info.as_mut() }) else {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        };
        if info.s_type != LAYER_NEGOTIATE_INTERFACE_STRUCT
            || info.loader_layer_interface_version < 2
        {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        }
        info.loader_layer_interface_version = 2;
        info.pfn_get_instance_proc_addr = Some(get_instance_proc_addr);
        info.pfn_get_device_proc_addr = Some(get_device_proc_addr);
        info.pfn_get_physical_device_proc_addr = std::ptr::null();
        vk::Result::SUCCESS
    })
}

/// Our implementation of a device-level function, if we intercept it.
fn device_hook(name: &CStr) -> vk::PFN_vkVoidFunction {
    // Each cast turns a typed function pointer into the generic PFN the loader expects.
    unsafe {
        Some(match name.to_bytes() {
            b"vkGetDeviceProcAddr" => std::mem::transmute::<
                vk::PFN_vkGetDeviceProcAddr,
                unsafe extern "system" fn(),
            >(get_device_proc_addr),
            b"vkDestroyDevice" => std::mem::transmute::<
                vk::PFN_vkDestroyDevice,
                unsafe extern "system" fn(),
            >(destroy_device),
            b"vkGetDeviceQueue" => std::mem::transmute::<
                vk::PFN_vkGetDeviceQueue,
                unsafe extern "system" fn(),
            >(get_device_queue),
            b"vkGetDeviceQueue2" => std::mem::transmute::<
                vk::PFN_vkGetDeviceQueue2,
                unsafe extern "system" fn(),
            >(get_device_queue2),
            b"vkCreateSwapchainKHR" => std::mem::transmute::<
                vk::PFN_vkCreateSwapchainKHR,
                unsafe extern "system" fn(),
            >(create_swapchain),
            b"vkDestroySwapchainKHR" => std::mem::transmute::<
                vk::PFN_vkDestroySwapchainKHR,
                unsafe extern "system" fn(),
            >(destroy_swapchain),
            b"vkQueuePresentKHR" => std::mem::transmute::<
                vk::PFN_vkQueuePresentKHR,
                unsafe extern "system" fn(),
            >(queue_present),
            _ => return None,
        })
    }
}

unsafe extern "system" fn get_instance_proc_addr(
    instance: vk::Instance,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    guard(None, || {
        // The loader and apps pass a valid, NUL-terminated name.
        let name = unsafe { CStr::from_ptr(name) };
        let own = unsafe {
            match name.to_bytes() {
                b"vkGetInstanceProcAddr" => Some(std::mem::transmute::<
                    vk::PFN_vkGetInstanceProcAddr,
                    unsafe extern "system" fn(),
                >(get_instance_proc_addr)),
                b"vkCreateInstance" => Some(std::mem::transmute::<
                    vk::PFN_vkCreateInstance,
                    unsafe extern "system" fn(),
                >(create_instance)),
                b"vkDestroyInstance" => Some(std::mem::transmute::<
                    vk::PFN_vkDestroyInstance,
                    unsafe extern "system" fn(),
                >(destroy_instance)),
                b"vkCreateDevice" => Some(std::mem::transmute::<
                    vk::PFN_vkCreateDevice,
                    unsafe extern "system" fn(),
                >(create_device)),
                _ => None,
            }
        };
        if own.is_some() {
            return own;
        }
        if let Some(hook) = device_hook(name) {
            return Some(hook);
        }
        if instance == vk::Instance::null() {
            return None;
        }
        let data = with_state(|s| s.instances.get(&unsafe { key(instance) }).cloned())?;
        unsafe { (data.gipa)(instance, name.as_ptr()) }
    })
}

unsafe extern "system" fn get_device_proc_addr(
    device: vk::Device,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    guard(None, || {
        let name = unsafe { CStr::from_ptr(name) };
        if let Some(hook) = device_hook(name) {
            return Some(hook);
        }
        let data = device_of(unsafe { key(device) })?;
        let gdpa = data.lock().ok()?.gdpa;
        unsafe { gdpa(device, name.as_ptr()) }
    })
}

/// The loader's link entry in a create-info chain.
unsafe fn find_chain<T>(mut next: *const c_void, s_type: i32, function: i32) -> Option<*mut T> {
    while !next.is_null() {
        // Every chained struct starts with `sType` and `pNext`; the loader structs follow it.
        let info = next.cast::<LayerInstanceCreateInfo>().cast_mut();
        unsafe {
            if (*info).s_type == s_type && (*info).function == function {
                return Some(info.cast());
            }
            next = (*info).p_next;
        }
    }
    None
}

unsafe extern "system" fn create_instance(
    create_info: *const vk::InstanceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Instance,
) -> vk::Result {
    guard(vk::Result::ERROR_INITIALIZATION_FAILED, || unsafe {
        let Some(chain) = find_chain::<LayerInstanceCreateInfo>(
            (*create_info).p_next,
            LOADER_INSTANCE_CREATE_INFO,
            LAYER_LINK_INFO,
        ) else {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        };
        let link = (*chain).u.cast::<LayerInstanceLink>();
        let gipa = (*link).next_get_instance_proc_addr;
        // The next layer reads its own link entry.
        (*chain).u = (*link).next.cast();
        let Some(next_create) = gipa(vk::Instance::null(), c"vkCreateInstance".as_ptr()) else {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        };
        let next_create: vk::PFN_vkCreateInstance = std::mem::transmute(next_create);
        let result = next_create(create_info, allocator, out);
        if result != vk::Result::SUCCESS {
            return result;
        }
        let instance = *out;
        let fns = ash::InstanceFnV1_0::load(|n| std::mem::transmute(gipa(instance, n.as_ptr())));
        with_state(|s| {
            s.instances.insert(
                key(instance),
                Arc::new(InstanceData {
                    handle: instance,
                    gipa,
                    fns,
                }),
            );
        });
        vk::Result::SUCCESS
    })
}

unsafe extern "system" fn destroy_instance(
    instance: vk::Instance,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    if instance == vk::Instance::null() {
        return;
    }
    let data = guard(None, || {
        with_state(|s| s.instances.remove(&unsafe { key(instance) }))
    });
    if let Some(data) = data {
        unsafe { (data.fns.destroy_instance)(instance, allocator) };
    }
}

unsafe extern "system" fn create_device(
    physical: vk::PhysicalDevice,
    create_info: *const vk::DeviceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Device,
) -> vk::Result {
    guard(vk::Result::ERROR_INITIALIZATION_FAILED, || unsafe {
        let Some(instance) = with_state(|s| s.instances.get(&key(physical)).cloned()) else {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        };
        let p_next = (*create_info).p_next;
        let Some(chain) =
            find_chain::<LayerDeviceCreateInfo>(p_next, LOADER_DEVICE_CREATE_INFO, LAYER_LINK_INFO)
        else {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        };
        let loader_data = find_chain::<LayerDeviceCreateInfo>(
            p_next,
            LOADER_DEVICE_CREATE_INFO,
            LOADER_DATA_CALLBACK,
        );
        let link = (*chain).u.cast::<LayerDeviceLink>();
        let gipa = (*link).next_get_instance_proc_addr;
        let gdpa = (*link).next_get_device_proc_addr;
        (*chain).u = (*link).next.cast();
        let Some(next_create) = gipa(instance.handle, c"vkCreateDevice".as_ptr()) else {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        };
        let next_create: vk::PFN_vkCreateDevice = std::mem::transmute(next_create);
        let result = next_create(physical, create_info, allocator, out);
        if result != vk::Result::SUCCESS {
            return result;
        }
        let device = *out;
        // Without the loader callback the layer cannot create command buffers; it then stays
        // out of the way for this device.
        let Some(loader_data) = loader_data else {
            debug!("no overlay for a device: the loader passed no dispatch callback");
            return vk::Result::SUCCESS;
        };
        let set_loader_data: SetDeviceLoaderData = std::mem::transmute((*loader_data).u);
        let load = |n: &CStr| std::mem::transmute(gdpa(device, n.as_ptr()));
        let fns = ash::Device::load_with(load, device);
        let swapchain = ash::khr::swapchain::DeviceFn::load(load);
        let mut count = 0;
        (instance.fns.get_physical_device_queue_family_properties)(
            physical,
            &mut count,
            std::ptr::null_mut(),
        );
        let mut families = vec![vk::QueueFamilyProperties::default(); count as usize];
        (instance.fns.get_physical_device_queue_family_properties)(
            physical,
            &mut count,
            families.as_mut_ptr(),
        );
        families.truncate(count as usize);
        let mut memory = vk::PhysicalDeviceMemoryProperties::default();
        (instance.fns.get_physical_device_memory_properties)(physical, &mut memory);
        let data = DeviceData {
            fns,
            swapchain,
            set_loader_data,
            memory,
            families,
            gdpa,
            queues: HashMap::new(),
        };
        with_state(|s| {
            s.devices.insert(key(device), Arc::new(Mutex::new(data)));
        });
        vk::Result::SUCCESS
    })
}

unsafe extern "system" fn destroy_device(
    device: vk::Device,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    if device == vk::Device::null() {
        return;
    }
    let data = guard(None, || {
        with_state(|s| s.devices.remove(&unsafe { key(device) }))
    });
    let Some(data) = data else {
        return;
    };
    let destroy = data.lock().map(|d| d.fns.fp_v1_0().destroy_device).ok();
    if let Some(destroy) = destroy {
        unsafe { destroy(device, allocator) };
    }
}

fn remember_queue(device: vk::Device, family: u32, queue: vk::Queue) {
    if queue == vk::Queue::null() {
        return;
    }
    if let Some(data) = device_of(unsafe { key(device) })
        && let Ok(mut d) = data.lock()
    {
        d.queues.insert(queue.as_raw(), family);
    }
}

unsafe extern "system" fn get_device_queue(
    device: vk::Device,
    family: u32,
    index: u32,
    out: *mut vk::Queue,
) {
    let Some(data) = guard(None, || device_of(unsafe { key(device) })) else {
        return;
    };
    let Ok(next) = data.lock().map(|d| d.fns.fp_v1_0().get_device_queue) else {
        return;
    };
    unsafe { next(device, family, index, out) };
    guard((), || remember_queue(device, family, unsafe { *out }));
}

unsafe extern "system" fn get_device_queue2(
    device: vk::Device,
    info: *const vk::DeviceQueueInfo2<'_>,
    out: *mut vk::Queue,
) {
    let Some(data) = guard(None, || device_of(unsafe { key(device) })) else {
        return;
    };
    let Ok(next) = data.lock().map(|d| d.fns.fp_v1_1().get_device_queue2) else {
        return;
    };
    unsafe { next(device, info, out) };
    guard((), || {
        remember_queue(device, unsafe { (*info).queue_family_index }, unsafe {
            *out
        })
    });
}

unsafe extern "system" fn create_swapchain(
    device: vk::Device,
    create_info: *const vk::SwapchainCreateInfoKHR<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::SwapchainKHR,
) -> vk::Result {
    let Some(data) = guard(None, || device_of(unsafe { key(device) })) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };
    let Ok(next) = data.lock().map(|d| d.swapchain.create_swapchain_khr) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };
    let active = guard(false, || !shared::disabled_by_env());
    // The overlay is drawn as a colour attachment; every surface supports that usage.
    let mut info = unsafe { *create_info };
    if active {
        info.image_usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
    }
    let result = unsafe { next(device, &info, allocator, out) };
    if result != vk::Result::SUCCESS || !active {
        if active {
            debug!("the game's swapchain creation failed ({})", result.as_raw());
        }
        return result;
    }
    guard((), || {
        let swapchain = unsafe { *out };
        debug!(
            "swapchain {:#x}: {}x{}, format {}, colour space {}, usage {:#x}, flags {:#x}, \
             present mode {}, at least {} images",
            swapchain.as_raw(),
            info.image_extent.width,
            info.image_extent.height,
            info.image_format.as_raw(),
            info.image_color_space.as_raw(),
            info.image_usage.as_raw(),
            info.flags.as_raw(),
            info.present_mode.as_raw(),
            info.min_image_count,
        );
        shared::set_display_height(info.image_extent.height);
        shared::start(mumcord_overlay_ipc::Api::Vulkan);
        let renderer = data
            .lock()
            .ok()
            .and_then(|d| Renderer::new(&d, device, swapchain, &info));
        with_state(|s| {
            s.swapchains.insert(swapchain.as_raw(), renderer);
        });
    });
    result
}

unsafe extern "system" fn destroy_swapchain(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    let Some(data) = guard(None, || device_of(unsafe { key(device) })) else {
        return;
    };
    guard((), || {
        let renderer = with_state(|s| s.swapchains.remove(&swapchain.as_raw())).flatten();
        if let Some(renderer) = renderer
            && let Ok(d) = data.lock()
        {
            renderer.destroy(&d);
        }
    });
    if let Ok(next) = data.lock().map(|d| d.swapchain.destroy_swapchain_khr) {
        unsafe { next(device, swapchain, allocator) };
    }
}

unsafe extern "system" fn queue_present(
    queue: vk::Queue,
    present: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    let Some(data) = guard(None, || device_of(unsafe { key(queue) })) else {
        return vk::Result::ERROR_DEVICE_LOST;
    };
    let Ok(next) = data.lock().map(|d| d.swapchain.queue_present_khr) else {
        return vk::Result::ERROR_DEVICE_LOST;
    };
    let drawn = guard(Err(Outcome::NoRenderer), || unsafe {
        draw_before_present(&data, queue, &*present)
    });
    guard((), || {
        log::present(match drawn {
            Ok(_) => Outcome::Drawn,
            Err(outcome) => outcome,
        });
    });
    match drawn.ok() {
        Some(semaphore) => {
            let mut info = unsafe { *present };
            let waits = [semaphore];
            info.wait_semaphore_count = 1;
            info.p_wait_semaphores = waits.as_ptr();
            unsafe { next(queue, &info) }
        }
        None => unsafe { next(queue, present) },
    }
}

/// Draws the overlay into the image being presented. Returns the semaphore the present must
/// wait on instead of the game's, or why nothing was drawn (the present goes through
/// unchanged).
unsafe fn draw_before_present(
    data: &Mutex<DeviceData>,
    queue: vk::Queue,
    present: &vk::PresentInfoKHR<'_>,
) -> Result<vk::Semaphore, Outcome> {
    // Several swapchains in one present are rare (multi-window); they are left alone.
    if present.swapchain_count != 1 {
        return Err(Outcome::SeveralSwapchains);
    }
    let (swapchain, index) = unsafe { (*present.p_swapchains, *present.p_image_indices) };
    let waits = if present.wait_semaphore_count == 0 {
        &[][..]
    } else {
        unsafe {
            std::slice::from_raw_parts(
                present.p_wait_semaphores,
                present.wait_semaphore_count as usize,
            )
        }
    };
    let device = data.lock().map_err(|_| Outcome::NoRenderer)?;
    let family = *device
        .queues
        .get(&queue.as_raw())
        .ok_or(Outcome::UnknownQueue)?;
    let graphics = device
        .families
        .get(family as usize)
        .is_some_and(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS));
    if !graphics {
        return Err(Outcome::NotGraphicsQueue);
    }
    let mut state = state();
    let renderer = state
        .as_mut()
        .and_then(|s| s.swapchains.get_mut(&swapchain.as_raw()))
        .and_then(Option::as_mut)
        .ok_or(Outcome::NoRenderer)?;
    renderer.draw(&device, queue, family, index, waits)
}
