//! OpenGL fallback, loaded with `LD_PRELOAD` (Steam launch option, see `docs/engine-api.md`).
//!
//! Interposes `glXSwapBuffers` and `eglSwapBuffers` (and returns the same hooks from
//! `glXGetProcAddress[ARB]` / `eglGetProcAddress`), draws the overlay into the back buffer with a
//! tiny shader, restores every piece of GL state it touched, then calls the real swap. Programs
//! that look the swap function up with `dlsym` on their own library handle are not reached.
//!
//! As for Vulkan: panics are caught, any failure draws nothing, and the real swap always runs.

// The GL function pointers are transmuted from `dlsym`/`GetProcAddress` results into their typed
// signatures; the target type is given by the field being assigned, so a turbofish would only
// repeat it.
#![allow(clippy::missing_transmute_annotations)]

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_uint, c_ulong, c_void};
use std::sync::{Arc, Mutex, OnceLock};

use mumcord_overlay_ipc::Api;

use crate::shared::{self, Frame};

type GlEnum = c_uint;
type GlUint = c_uint;
type GlInt = c_int;

const GL_VERSION: GlEnum = 0x1F02;
const GL_MAJOR_VERSION: GlEnum = 0x821B;
const GL_MINOR_VERSION: GlEnum = 0x821C;
const GL_CURRENT_PROGRAM: GlEnum = 0x8B8D;
const GL_VERTEX_ARRAY_BINDING: GlEnum = 0x85B5;
const GL_ARRAY_BUFFER_BINDING: GlEnum = 0x8894;
const GL_ARRAY_BUFFER: GlEnum = 0x8892;
const GL_ACTIVE_TEXTURE: GlEnum = 0x84E0;
const GL_TEXTURE0: GlEnum = 0x84C0;
const GL_TEXTURE_2D: GlEnum = 0x0DE1;
const GL_TEXTURE_BINDING_2D: GlEnum = 0x8069;
const GL_SAMPLER_BINDING: GlEnum = 0x8919;
const GL_BLEND: GlEnum = 0x0BE2;
const GL_BLEND_SRC_RGB: GlEnum = 0x80C9;
const GL_BLEND_DST_RGB: GlEnum = 0x80C8;
const GL_BLEND_SRC_ALPHA: GlEnum = 0x80CB;
const GL_BLEND_DST_ALPHA: GlEnum = 0x80CA;
const GL_BLEND_EQUATION_RGB: GlEnum = 0x8009;
const GL_BLEND_EQUATION_ALPHA: GlEnum = 0x883D;
const GL_DEPTH_TEST: GlEnum = 0x0B71;
const GL_CULL_FACE: GlEnum = 0x0B44;
const GL_SCISSOR_TEST: GlEnum = 0x0C11;
const GL_STENCIL_TEST: GlEnum = 0x0B90;
const GL_FRAMEBUFFER_SRGB: GlEnum = 0x8DB9;
const GL_RASTERIZER_DISCARD: GlEnum = 0x8C89;
const GL_VIEWPORT: GlEnum = 0x0BA2;
const GL_COLOR_WRITEMASK: GlEnum = 0x0C23;
const GL_DRAW_FRAMEBUFFER: GlEnum = 0x8CA9;
const GL_DRAW_FRAMEBUFFER_BINDING: GlEnum = 0x8CA6;
const GL_UNPACK_ALIGNMENT: GlEnum = 0x0CF5;
const GL_UNPACK_ROW_LENGTH: GlEnum = 0x0CF2;
const GL_PIXEL_UNPACK_BUFFER: GlEnum = 0x88EC;
const GL_PIXEL_UNPACK_BUFFER_BINDING: GlEnum = 0x88EF;
const GL_RGBA: GlEnum = 0x1908;
const GL_UNSIGNED_BYTE: GlEnum = 0x1401;
const GL_TEXTURE_MIN_FILTER: GlEnum = 0x2801;
const GL_TEXTURE_MAG_FILTER: GlEnum = 0x2800;
const GL_TEXTURE_WRAP_S: GlEnum = 0x2802;
const GL_TEXTURE_WRAP_T: GlEnum = 0x2803;
const GL_NEAREST: GlInt = 0x2600;
const GL_CLAMP_TO_EDGE: GlInt = 0x812F;
const GL_ONE: GlEnum = 1;
const GL_ONE_MINUS_SRC_ALPHA: GlEnum = 0x0303;
const GL_FUNC_ADD: GlEnum = 0x8006;
const GL_TRIANGLES: GlEnum = 0x0004;
const GL_VERTEX_SHADER: GlEnum = 0x8B31;
const GL_FRAGMENT_SHADER: GlEnum = 0x8B30;
const GL_COMPILE_STATUS: GlEnum = 0x8B81;
const GL_LINK_STATUS: GlEnum = 0x8B82;
const GLX_WIDTH: c_int = 0x801D;
const GLX_HEIGHT: c_int = 0x801E;
const EGL_WIDTH: c_int = 0x3057;
const EGL_HEIGHT: c_int = 0x3056;

/// The GL functions the overlay uses, loaded once from the window system's proc-address
/// function. Missing optional ones (VAOs, samplers, sRGB framebuffers) are `None`.
#[allow(clippy::type_complexity)]
struct Gl {
    get_string: unsafe extern "C" fn(GlEnum) -> *const u8,
    get_integerv: unsafe extern "C" fn(GlEnum, *mut GlInt),
    get_booleanv: unsafe extern "C" fn(GlEnum, *mut u8),
    is_enabled: unsafe extern "C" fn(GlEnum) -> u8,
    enable: unsafe extern "C" fn(GlEnum),
    disable: unsafe extern "C" fn(GlEnum),
    viewport: unsafe extern "C" fn(GlInt, GlInt, GlInt, GlInt),
    color_mask: unsafe extern "C" fn(u8, u8, u8, u8),
    blend_func_separate: unsafe extern "C" fn(GlEnum, GlEnum, GlEnum, GlEnum),
    blend_equation_separate: unsafe extern "C" fn(GlEnum, GlEnum),
    active_texture: unsafe extern "C" fn(GlEnum),
    bind_texture: unsafe extern "C" fn(GlEnum, GlUint),
    gen_textures: unsafe extern "C" fn(GlInt, *mut GlUint),
    is_texture: unsafe extern "C" fn(GlUint) -> u8,
    tex_parameteri: unsafe extern "C" fn(GlEnum, GlEnum, GlInt),
    tex_image_2d: unsafe extern "C" fn(
        GlEnum,
        GlInt,
        GlInt,
        GlInt,
        GlInt,
        GlInt,
        GlEnum,
        GlEnum,
        *const c_void,
    ),
    pixel_storei: unsafe extern "C" fn(GlEnum, GlInt),
    bind_buffer: unsafe extern "C" fn(GlEnum, GlUint),
    bind_framebuffer: unsafe extern "C" fn(GlEnum, GlUint),
    create_shader: unsafe extern "C" fn(GlEnum) -> GlUint,
    shader_source: unsafe extern "C" fn(GlUint, GlInt, *const *const c_char, *const GlInt),
    compile_shader: unsafe extern "C" fn(GlUint),
    get_shaderiv: unsafe extern "C" fn(GlUint, GlEnum, *mut GlInt),
    delete_shader: unsafe extern "C" fn(GlUint),
    create_program: unsafe extern "C" fn() -> GlUint,
    attach_shader: unsafe extern "C" fn(GlUint, GlUint),
    link_program: unsafe extern "C" fn(GlUint),
    get_programiv: unsafe extern "C" fn(GlUint, GlEnum, *mut GlInt),
    is_program: unsafe extern "C" fn(GlUint) -> u8,
    use_program: unsafe extern "C" fn(GlUint),
    get_uniform_location: unsafe extern "C" fn(GlUint, *const c_char) -> GlInt,
    uniform1i: unsafe extern "C" fn(GlInt, GlInt),
    draw_arrays: unsafe extern "C" fn(GlEnum, GlInt, GlInt),
    gen_vertex_arrays: Option<unsafe extern "C" fn(GlInt, *mut GlUint)>,
    bind_vertex_array: Option<unsafe extern "C" fn(GlUint)>,
    bind_sampler: Option<unsafe extern "C" fn(GlUint, GlUint)>,
    /// OpenGL ES: no `GL_FRAMEBUFFER_SRGB` switch (touching it would raise a GL error the game
    /// might see).
    es: bool,
}

type ProcAddress = unsafe extern "C" fn(*const c_char) -> *const c_void;

impl Gl {
    unsafe fn load(get: ProcAddress) -> Option<Gl> {
        macro_rules! req {
            ($name:literal) => {{
                let p = unsafe { get(concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return None;
                }
                // The name and the field type match the GL specification.
                unsafe { std::mem::transmute::<*const c_void, _>(p) }
            }};
        }
        macro_rules! opt {
            ($name:literal) => {{
                let p = unsafe { get(concat!($name, "\0").as_ptr().cast()) };
                (!p.is_null()).then(|| unsafe { std::mem::transmute::<*const c_void, _>(p) })
            }};
        }
        let get_string: unsafe extern "C" fn(GlEnum) -> *const u8 = req!("glGetString");
        // Called from a swap, so the context is current; GL returns a static string.
        let version = unsafe { get_string(GL_VERSION) };
        let es = !version.is_null()
            && unsafe { CStr::from_ptr(version.cast()) }
                .to_bytes()
                .starts_with(b"OpenGL ES");
        Some(Gl {
            es,
            get_string,
            get_integerv: req!("glGetIntegerv"),
            get_booleanv: req!("glGetBooleanv"),
            is_enabled: req!("glIsEnabled"),
            enable: req!("glEnable"),
            disable: req!("glDisable"),
            viewport: req!("glViewport"),
            color_mask: req!("glColorMask"),
            blend_func_separate: req!("glBlendFuncSeparate"),
            blend_equation_separate: req!("glBlendEquationSeparate"),
            active_texture: req!("glActiveTexture"),
            bind_texture: req!("glBindTexture"),
            gen_textures: req!("glGenTextures"),
            is_texture: req!("glIsTexture"),
            tex_parameteri: req!("glTexParameteri"),
            tex_image_2d: req!("glTexImage2D"),
            pixel_storei: req!("glPixelStorei"),
            bind_buffer: req!("glBindBuffer"),
            bind_framebuffer: req!("glBindFramebuffer"),
            create_shader: req!("glCreateShader"),
            shader_source: req!("glShaderSource"),
            compile_shader: req!("glCompileShader"),
            get_shaderiv: req!("glGetShaderiv"),
            delete_shader: req!("glDeleteShader"),
            create_program: req!("glCreateProgram"),
            attach_shader: req!("glAttachShader"),
            link_program: req!("glLinkProgram"),
            get_programiv: req!("glGetProgramiv"),
            is_program: req!("glIsProgram"),
            use_program: req!("glUseProgram"),
            get_uniform_location: req!("glGetUniformLocation"),
            uniform1i: req!("glUniform1i"),
            draw_arrays: req!("glDrawArrays"),
            gen_vertex_arrays: opt!("glGenVertexArrays"),
            bind_vertex_array: opt!("glBindVertexArray"),
            bind_sampler: opt!("glBindSampler"),
        })
    }
}

/// GL objects of one context. Names are re-checked every frame, so a destroyed context whose
/// address is reused gets fresh objects.
struct Context {
    program: GlUint,
    vao: GlUint,
    texture: GlUint,
    seen: u64,
    frame: Option<Arc<Frame>>,
    needs_upload: bool,
    size: (u32, u32),
}

#[derive(Default)]
struct Hooks {
    glx: Option<Gl>,
    egl: Option<Gl>,
    contexts: HashMap<usize, Context>,
}

static HOOKS: Mutex<Option<Hooks>> = Mutex::new(None);

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// The next definition of `name` after this library (the real GL / EGL function).
fn next(name: &CStr) -> *mut c_void {
    // `RTLD_NEXT` lookups are thread-safe and the name is NUL-terminated.
    unsafe { libc::dlsym(libc::RTLD_NEXT, name.as_ptr()) }
}

fn real<T: Copy>(slot: &OnceLock<usize>, name: &CStr) -> Option<T> {
    let p = *slot.get_or_init(|| next(name) as usize);
    // The slot holds the address of the function `T` describes, or 0.
    (p != 0).then(|| unsafe { std::mem::transmute_copy::<usize, T>(&p) })
}

type GlxSwap = unsafe extern "C" fn(*mut c_void, c_ulong);
type EglSwap = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_uint;

static REAL_GLX_SWAP: OnceLock<usize> = OnceLock::new();
static REAL_EGL_SWAP: OnceLock<usize> = OnceLock::new();
static REAL_GLX_PROC: OnceLock<usize> = OnceLock::new();
static REAL_GLX_PROC_ARB: OnceLock<usize> = OnceLock::new();
static REAL_EGL_PROC: OnceLock<usize> = OnceLock::new();
static REAL_GLX_CONTEXT: OnceLock<usize> = OnceLock::new();
static REAL_EGL_CONTEXT: OnceLock<usize> = OnceLock::new();
static REAL_GLX_QUERY: OnceLock<usize> = OnceLock::new();
static REAL_EGL_QUERY: OnceLock<usize> = OnceLock::new();

/// # Safety
/// Same contract as the real `glXSwapBuffers`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glXSwapBuffers(display: *mut c_void, drawable: c_ulong) {
    guard((), || unsafe { draw_glx(display, drawable) });
    if let Some(swap) = real::<GlxSwap>(&REAL_GLX_SWAP, c"glXSwapBuffers") {
        unsafe { swap(display, drawable) };
    }
}

/// # Safety
/// Same contract as the real `eglSwapBuffers`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn eglSwapBuffers(display: *mut c_void, surface: *mut c_void) -> c_uint {
    guard((), || unsafe { draw_egl(display, surface) });
    match real::<EglSwap>(&REAL_EGL_SWAP, c"eglSwapBuffers") {
        Some(swap) => unsafe { swap(display, surface) },
        None => 0,
    }
}

fn hook_for(name: *const c_char) -> Option<*const c_void> {
    if name.is_null() {
        return None;
    }
    // A NUL-terminated name from the caller.
    match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"glXSwapBuffers" => Some(glXSwapBuffers as *const c_void),
        b"eglSwapBuffers" => Some(eglSwapBuffers as *const c_void),
        _ => None,
    }
}

/// # Safety
/// Same contract as the real `glXGetProcAddressARB`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glXGetProcAddressARB(name: *const c_char) -> *const c_void {
    if let Some(hook) = guard(None, || hook_for(name)) {
        return hook;
    }
    match real::<ProcAddress>(&REAL_GLX_PROC_ARB, c"glXGetProcAddressARB") {
        Some(get) => unsafe { get(name) },
        None => std::ptr::null(),
    }
}

/// # Safety
/// Same contract as the real `glXGetProcAddress`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glXGetProcAddress(name: *const c_char) -> *const c_void {
    if let Some(hook) = guard(None, || hook_for(name)) {
        return hook;
    }
    match real::<ProcAddress>(&REAL_GLX_PROC, c"glXGetProcAddress") {
        Some(get) => unsafe { get(name) },
        None => std::ptr::null(),
    }
}

/// # Safety
/// Same contract as the real `eglGetProcAddress`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn eglGetProcAddress(name: *const c_char) -> *const c_void {
    if let Some(hook) = guard(None, || hook_for(name)) {
        return hook;
    }
    match real::<ProcAddress>(&REAL_EGL_PROC, c"eglGetProcAddress") {
        Some(get) => unsafe { get(name) },
        None => std::ptr::null(),
    }
}

unsafe fn draw_glx(display: *mut c_void, drawable: c_ulong) {
    if shared::disabled_by_env() {
        return;
    }
    let Some(current) =
        real::<unsafe extern "C" fn() -> *mut c_void>(&REAL_GLX_CONTEXT, c"glXGetCurrentContext")
    else {
        return;
    };
    let context = unsafe { current() } as usize;
    let Some(query) = real::<unsafe extern "C" fn(*mut c_void, c_ulong, c_int, *mut c_uint)>(
        &REAL_GLX_QUERY,
        c"glXQueryDrawable",
    ) else {
        return;
    };
    let (mut w, mut h) = (0, 0);
    unsafe {
        query(display, drawable, GLX_WIDTH, &mut w);
        query(display, drawable, GLX_HEIGHT, &mut h);
    }
    unsafe { draw(false, context, (w, h)) };
}

unsafe fn draw_egl(display: *mut c_void, surface: *mut c_void) {
    if shared::disabled_by_env() {
        return;
    }
    let Some(current) =
        real::<unsafe extern "C" fn() -> *mut c_void>(&REAL_EGL_CONTEXT, c"eglGetCurrentContext")
    else {
        return;
    };
    let context = unsafe { current() } as usize;
    let Some(query) = real::<
        unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, *mut c_int) -> c_uint,
    >(&REAL_EGL_QUERY, c"eglQuerySurface") else {
        return;
    };
    let (mut w, mut h) = (0, 0);
    unsafe {
        query(display, surface, EGL_WIDTH, &mut w);
        query(display, surface, EGL_HEIGHT, &mut h);
    }
    let size = (u32::try_from(w).unwrap_or(0), u32::try_from(h).unwrap_or(0));
    unsafe { draw(true, context, size) };
}

unsafe fn draw(egl: bool, context: usize, size: (u32, u32)) {
    if context == 0 || size.0 == 0 || size.1 == 0 {
        return;
    }
    shared::start(Api::OpenGl);
    shared::set_display_height(size.1);
    // Only the thread swapping uses its context; contention here means another context of the
    // same process is drawing, and that frame simply goes without the overlay.
    let Ok(mut hooks) = HOOKS.try_lock() else {
        return;
    };
    let hooks = hooks.get_or_insert_with(Hooks::default);
    let gl = if egl { &mut hooks.egl } else { &mut hooks.glx };
    if gl.is_none() {
        let get = if egl {
            real::<ProcAddress>(&REAL_EGL_PROC, c"eglGetProcAddress")
        } else {
            real::<ProcAddress>(&REAL_GLX_PROC_ARB, c"glXGetProcAddressARB")
        };
        let Some(get) = get else {
            return;
        };
        *gl = unsafe { Gl::load(get) };
    }
    let Some(gl) = gl.as_ref() else {
        return;
    };
    let ctx = hooks.contexts.entry(context).or_insert(Context {
        program: 0,
        vao: 0,
        texture: 0,
        seen: 0,
        frame: None,
        needs_upload: false,
        size: (0, 0),
    });
    let generation = shared::generation();
    if generation != ctx.seen
        && let Ok(frame) = shared::try_frame()
    {
        ctx.seen = generation;
        ctx.needs_upload = frame.is_some();
        ctx.frame = frame;
    }
    let Some(frame) = ctx.frame.clone() else {
        return;
    };
    let (w, h) = (frame.image.width, frame.image.height);
    let Some((x, top)) = shared::place(frame.corner, frame.margin, (w, h), size) else {
        return;
    };
    let saved = unsafe { Saved::take(gl) };
    unsafe {
        if prepare(gl, ctx).is_some() {
            render(gl, ctx, &frame, (x, size.1 - top - h));
        }
        saved.restore(gl);
    }
}

/// Makes sure the program, VAO and texture exist in the current context.
unsafe fn prepare(gl: &Gl, ctx: &mut Context) -> Option<()> {
    unsafe {
        if ctx.program == 0 || (gl.is_program)(ctx.program) == 0 {
            ctx.program = program(gl)?;
            ctx.vao = 0;
            ctx.texture = 0;
        }
        if ctx.vao == 0
            && let Some(gen_vertex_arrays) = gl.gen_vertex_arrays
        {
            gen_vertex_arrays(1, &mut ctx.vao);
        }
        if ctx.texture == 0 || (gl.is_texture)(ctx.texture) == 0 {
            (gl.gen_textures)(1, &mut ctx.texture);
            ctx.needs_upload = true;
            ctx.size = (0, 0);
        }
    }
    Some(())
}

/// Desktop GL 3.3+ (core or compatibility), desktop GL 3.0–3.2, or OpenGL ES 3.
unsafe fn shader_header(gl: &Gl) -> Option<&'static str> {
    let version = unsafe { (gl.get_string)(GL_VERSION) };
    if version.is_null() {
        return None;
    }
    // GL returns a NUL-terminated static string.
    let version = unsafe { CStr::from_ptr(version.cast()) }.to_string_lossy();
    if version.starts_with("OpenGL ES") {
        let major = version
            .trim_start_matches("OpenGL ES ")
            .chars()
            .next()?
            .to_digit(10)?;
        return (major >= 3).then_some("#version 300 es\nprecision mediump float;\n");
    }
    let (mut major, mut minor) = (0, 0);
    unsafe {
        (gl.get_integerv)(GL_MAJOR_VERSION, &mut major);
        (gl.get_integerv)(GL_MINOR_VERSION, &mut minor);
    }
    match (major, minor) {
        (m, _) if m > 3 => Some("#version 330 core\n"),
        (3, n) if n >= 3 => Some("#version 330 core\n"),
        (3, _) => Some("#version 130\n"),
        _ => None,
    }
}

const VERTEX: &str = "out vec2 uv;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    uv = vec2(p.x, 1.0 - p.y);
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
";

const FRAGMENT: &str = "in vec2 uv;
uniform sampler2D overlay;
out vec4 color;
void main() {
    color = texture(overlay, uv);
}
";

unsafe fn shader(gl: &Gl, kind: GlEnum, header: &str, body: &str) -> Option<GlUint> {
    let source = std::ffi::CString::new(format!("{header}{body}")).ok()?;
    unsafe {
        let shader = (gl.create_shader)(kind);
        let ptr = source.as_ptr();
        (gl.shader_source)(shader, 1, &ptr, std::ptr::null());
        (gl.compile_shader)(shader);
        let mut ok = 0;
        (gl.get_shaderiv)(shader, GL_COMPILE_STATUS, &mut ok);
        if ok == 0 {
            (gl.delete_shader)(shader);
            return None;
        }
        Some(shader)
    }
}

unsafe fn program(gl: &Gl) -> Option<GlUint> {
    unsafe {
        let header = shader_header(gl)?;
        let vertex = shader(gl, GL_VERTEX_SHADER, header, VERTEX)?;
        let Some(fragment) = shader(gl, GL_FRAGMENT_SHADER, header, FRAGMENT) else {
            (gl.delete_shader)(vertex);
            return None;
        };
        let program = (gl.create_program)();
        (gl.attach_shader)(program, vertex);
        (gl.attach_shader)(program, fragment);
        (gl.link_program)(program);
        (gl.delete_shader)(vertex);
        (gl.delete_shader)(fragment);
        let mut ok = 0;
        (gl.get_programiv)(program, GL_LINK_STATUS, &mut ok);
        (ok != 0).then_some(program)
    }
}

unsafe fn render(gl: &Gl, ctx: &mut Context, frame: &Frame, (x, y): (u32, u32)) {
    let (w, h) = (frame.image.width, frame.image.height);
    let as_int = |v: u32| GlInt::try_from(v).unwrap_or(0);
    unsafe {
        (gl.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, 0);
        (gl.use_program)(ctx.program);
        if let Some(bind) = gl.bind_vertex_array {
            bind(ctx.vao);
        }
        (gl.bind_buffer)(GL_ARRAY_BUFFER, 0);
        (gl.active_texture)(GL_TEXTURE0);
        (gl.bind_texture)(GL_TEXTURE_2D, ctx.texture);
        if let Some(bind) = gl.bind_sampler {
            bind(0, 0);
        }
        if ctx.needs_upload {
            (gl.bind_buffer)(GL_PIXEL_UNPACK_BUFFER, 0);
            (gl.pixel_storei)(GL_UNPACK_ALIGNMENT, 4);
            (gl.pixel_storei)(GL_UNPACK_ROW_LENGTH, 0);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            // GL_RGBA8 as the internal format.
            (gl.tex_image_2d)(
                GL_TEXTURE_2D,
                0,
                0x8058,
                as_int(w),
                as_int(h),
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                frame.image.pixels.as_ptr().cast(),
            );
            ctx.needs_upload = false;
            ctx.size = (w, h);
        }
        let location = (gl.get_uniform_location)(ctx.program, c"overlay".as_ptr());
        (gl.uniform1i)(location, 0);
        for cap in [
            GL_DEPTH_TEST,
            GL_CULL_FACE,
            GL_SCISSOR_TEST,
            GL_STENCIL_TEST,
            GL_FRAMEBUFFER_SRGB,
            GL_RASTERIZER_DISCARD,
        ] {
            if !(gl.es && cap == GL_FRAMEBUFFER_SRGB) {
                (gl.disable)(cap);
            }
        }
        (gl.enable)(GL_BLEND);
        (gl.blend_func_separate)(
            GL_ONE,
            GL_ONE_MINUS_SRC_ALPHA,
            GL_ONE,
            GL_ONE_MINUS_SRC_ALPHA,
        );
        (gl.blend_equation_separate)(GL_FUNC_ADD, GL_FUNC_ADD);
        (gl.color_mask)(1, 1, 1, 1);
        (gl.viewport)(as_int(x), as_int(y), as_int(ctx.size.0), as_int(ctx.size.1));
        (gl.draw_arrays)(GL_TRIANGLES, 0, 3);
    }
}

/// The GL state the overlay changes, restored right after drawing.
struct Saved {
    program: GlInt,
    vao: GlInt,
    array_buffer: GlInt,
    unpack_buffer: GlInt,
    active_texture: GlInt,
    texture: GlInt,
    sampler: GlInt,
    framebuffer: GlInt,
    blend_src_rgb: GlInt,
    blend_dst_rgb: GlInt,
    blend_src_alpha: GlInt,
    blend_dst_alpha: GlInt,
    blend_eq_rgb: GlInt,
    blend_eq_alpha: GlInt,
    unpack_alignment: GlInt,
    unpack_row_length: GlInt,
    viewport: [GlInt; 4],
    color_mask: [u8; 4],
    caps: [(GlEnum, bool); 7],
}

impl Saved {
    unsafe fn take(gl: &Gl) -> Saved {
        let int = |name| {
            let mut v = 0;
            unsafe { (gl.get_integerv)(name, &mut v) };
            v
        };
        let mut viewport = [0; 4];
        let mut color_mask = [1; 4];
        unsafe {
            (gl.get_integerv)(GL_VIEWPORT, viewport.as_mut_ptr());
            (gl.get_booleanv)(GL_COLOR_WRITEMASK, color_mask.as_mut_ptr());
        }
        let active_texture = int(GL_ACTIVE_TEXTURE);
        unsafe { (gl.active_texture)(GL_TEXTURE0) };
        let texture = int(GL_TEXTURE_BINDING_2D);
        let sampler = if gl.bind_sampler.is_some() {
            int(GL_SAMPLER_BINDING)
        } else {
            0
        };
        let caps = [
            GL_BLEND,
            GL_DEPTH_TEST,
            GL_CULL_FACE,
            GL_SCISSOR_TEST,
            GL_STENCIL_TEST,
            GL_FRAMEBUFFER_SRGB,
            GL_RASTERIZER_DISCARD,
        ]
        .map(|cap| {
            (
                cap,
                !(gl.es && cap == GL_FRAMEBUFFER_SRGB) && unsafe { (gl.is_enabled)(cap) } != 0,
            )
        });
        Saved {
            program: int(GL_CURRENT_PROGRAM),
            vao: if gl.bind_vertex_array.is_some() {
                int(GL_VERTEX_ARRAY_BINDING)
            } else {
                0
            },
            array_buffer: int(GL_ARRAY_BUFFER_BINDING),
            unpack_buffer: int(GL_PIXEL_UNPACK_BUFFER_BINDING),
            active_texture,
            texture,
            sampler,
            framebuffer: int(GL_DRAW_FRAMEBUFFER_BINDING),
            blend_src_rgb: int(GL_BLEND_SRC_RGB),
            blend_dst_rgb: int(GL_BLEND_DST_RGB),
            blend_src_alpha: int(GL_BLEND_SRC_ALPHA),
            blend_dst_alpha: int(GL_BLEND_DST_ALPHA),
            blend_eq_rgb: int(GL_BLEND_EQUATION_RGB),
            blend_eq_alpha: int(GL_BLEND_EQUATION_ALPHA),
            unpack_alignment: int(GL_UNPACK_ALIGNMENT),
            unpack_row_length: int(GL_UNPACK_ROW_LENGTH),
            viewport,
            color_mask,
            caps,
        }
    }

    unsafe fn restore(&self, gl: &Gl) {
        // GL reports names and enums as non-negative integers.
        #[allow(clippy::cast_sign_loss)]
        let u = |v: GlInt| v as GlUint;
        unsafe {
            for (cap, on) in self.caps {
                if gl.es && cap == GL_FRAMEBUFFER_SRGB {
                    continue;
                }
                if on {
                    (gl.enable)(cap);
                } else {
                    (gl.disable)(cap);
                }
            }
            (gl.blend_func_separate)(
                u(self.blend_src_rgb),
                u(self.blend_dst_rgb),
                u(self.blend_src_alpha),
                u(self.blend_dst_alpha),
            );
            (gl.blend_equation_separate)(u(self.blend_eq_rgb), u(self.blend_eq_alpha));
            let [r, g, b, a] = self.color_mask;
            (gl.color_mask)(r, g, b, a);
            let [x, y, w, h] = self.viewport;
            (gl.viewport)(x, y, w, h);
            (gl.pixel_storei)(GL_UNPACK_ALIGNMENT, self.unpack_alignment);
            (gl.pixel_storei)(GL_UNPACK_ROW_LENGTH, self.unpack_row_length);
            (gl.active_texture)(GL_TEXTURE0);
            (gl.bind_texture)(GL_TEXTURE_2D, u(self.texture));
            if let Some(bind) = gl.bind_sampler {
                bind(0, u(self.sampler));
            }
            (gl.active_texture)(u(self.active_texture));
            (gl.bind_buffer)(GL_PIXEL_UNPACK_BUFFER, u(self.unpack_buffer));
            (gl.bind_buffer)(GL_ARRAY_BUFFER, u(self.array_buffer));
            if let Some(bind) = gl.bind_vertex_array {
                bind(u(self.vao));
            }
            (gl.use_program)(u(self.program));
            (gl.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, u(self.framebuffer));
        }
    }
}
