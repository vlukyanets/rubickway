//! EGL setup: a display, a GLES 3.0 context, window surfaces, and a GL function loader.
//!
//! Two kinds of display:
//! - [`Egl::gbm`] on the graphics card's GBM device, one window surface per display;
//! - [`Egl::software_device`] on Mesa's software rasteriser (llvmpipe), chosen explicitly
//!   through device enumeration so rendered pixels are identical on every machine.
#![allow(unsafe_code)]

use std::ffi::{CStr, c_char, c_void};

use khronos_egl as egl;
use thiserror::Error;

const PLATFORM_GBM_KHR: egl::Enum = 0x31D7;
const PLATFORM_DEVICE_EXT: egl::Enum = 0x313F;
const PLATFORM_SURFACELESS_MESA: egl::Enum = 0x31DD;

type QueryDevicesExt =
    unsafe extern "system" fn(max: egl::Int, devices: *mut *mut c_void, num: *mut egl::Int) -> u32;
type QueryDeviceStringExt =
    unsafe extern "system" fn(device: *mut c_void, name: egl::Int) -> *const c_char;

/// Errors while setting up EGL or GLES.
#[derive(Debug, Error)]
pub enum EglError {
    #[error("{call} failed: {source}")]
    Call {
        call: &'static str,
        source: egl::Error,
    },
    #[error("no EGL config with OpenGL ES 3 support")]
    NoConfig,
    #[error("EGL extension {0} is not available")]
    MissingExtension(&'static str),
    #[error("no software rendering device (EGL_MESA_device_software) found")]
    NoSoftwareDevice,
}

fn call<T>(name: &'static str, result: Result<T, egl::Error>) -> Result<T, EglError> {
    result.map_err(|source| EglError::Call { call: name, source })
}

/// An initialised EGL display with one GLES 3.0 context.
pub struct Egl {
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
}

impl Egl {
    /// EGL on Mesa's software rasteriser, for offscreen rendering in tests.
    pub fn software_device() -> Result<Egl, EglError> {
        let display = match software_device_display() {
            Ok(display) => display,
            Err(err) => {
                tracing::warn!("{err}; falling back to the surfaceless platform");
                // SAFETY: the surfaceless platform takes no native display (`DEFAULT_DISPLAY`,
                // a null pointer), and the attribute list is `NONE`-terminated.
                call("eglGetPlatformDisplay", unsafe {
                    egl::API.get_platform_display(
                        PLATFORM_SURFACELESS_MESA,
                        egl::DEFAULT_DISPLAY,
                        &[egl::ATTRIB_NONE],
                    )
                })?
            }
        };
        // Any surface type: rendering goes to a framebuffer object.
        Egl::with_display(display, 0)
    }

    fn with_display(display: egl::Display, surface_type: egl::Int) -> Result<Egl, EglError> {
        call("eglInitialize", egl::API.initialize(display))?;
        call("eglBindAPI", egl::API.bind_api(egl::OPENGL_ES_API))?;
        let attribs = [
            egl::SURFACE_TYPE,
            surface_type,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_ES3_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::NONE,
        ];
        let config = call(
            "eglChooseConfig",
            egl::API.choose_first_config(display, &attribs),
        )?
        .ok_or(EglError::NoConfig)?;
        let context = call(
            "eglCreateContext",
            egl::API.create_context(
                display,
                config,
                None,
                &[egl::CONTEXT_MAJOR_VERSION, 3, egl::NONE],
            ),
        )?;
        Ok(Egl {
            display,
            config,
            context,
        })
    }

    /// EGL on a GBM device, for the standalone backend (research S6). The config's native
    /// visual is `XRGB8888`, the format scanout buffers use. The caller keeps `device` alive
    /// for as long as this `Egl` and its surfaces exist.
    pub fn gbm<T: std::os::fd::AsFd>(device: &gbm::Device<T>) -> Result<Egl, EglError> {
        use gbm::AsRaw;
        let native = device.as_raw().cast_mut().cast::<c_void>();
        // SAFETY: `native` is a live `gbm_device`, which outlives every EGL object made here
        // (caller contract above); the attribute list is `NONE`-terminated.
        let display = call("eglGetPlatformDisplay", unsafe {
            egl::API.get_platform_display(PLATFORM_GBM_KHR, native, &[egl::ATTRIB_NONE])
        })?;
        call("eglInitialize", egl::API.initialize(display))?;
        call("eglBindAPI", egl::API.bind_api(egl::OPENGL_ES_API))?;
        let attribs = [
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_ES3_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::NONE,
        ];
        let mut configs = Vec::with_capacity(64);
        call(
            "eglChooseConfig",
            egl::API.choose_config(display, &attribs, &mut configs),
        )?;
        let xrgb = gbm::Format::Xrgb8888 as u32;
        let config = configs
            .into_iter()
            .find(|&c| {
                egl::API
                    .get_config_attrib(display, c, egl::NATIVE_VISUAL_ID)
                    .is_ok_and(|v| v as u32 == xrgb)
            })
            .ok_or(EglError::NoConfig)?;
        let context = call(
            "eglCreateContext",
            egl::API.create_context(
                display,
                config,
                None,
                &[egl::CONTEXT_MAJOR_VERSION, 3, egl::NONE],
            ),
        )?;
        Ok(Egl {
            display,
            config,
            context,
        })
    }

    /// Creates a window surface on a GBM surface (one per output).
    pub fn create_gbm_surface<U: 'static>(
        &self,
        surface: &gbm::Surface<U>,
    ) -> Result<egl::Surface, EglError> {
        use gbm::AsRaw;
        // SAFETY: `surface` is a live `gbm_surface` of the device this display was opened on;
        // the caller destroys the EGL surface (`destroy_surface`) before the GBM surface.
        call("eglCreatePlatformWindowSurface", unsafe {
            egl::API.create_platform_window_surface(
                self.display,
                self.config,
                surface.as_raw().cast_mut().cast::<c_void>(),
                &[egl::ATTRIB_NONE],
            )
        })
    }

    /// Destroys a surface made by [`Egl::create_gbm_surface`].
    pub fn destroy_surface(&self, surface: egl::Surface) {
        if let Err(err) = egl::API.destroy_surface(self.display, surface) {
            tracing::warn!("eglDestroySurface failed: {err}");
        }
    }

    /// Makes the context current on `surface`, or without a surface for offscreen work.
    pub fn make_current(&self, surface: Option<egl::Surface>) -> Result<(), EglError> {
        call(
            "eglMakeCurrent",
            egl::API.make_current(self.display, surface, surface, Some(self.context)),
        )
    }

    /// Presents the back buffer of `surface`.
    pub fn swap_buffers(&self, surface: egl::Surface) -> Result<(), EglError> {
        call(
            "eglSwapBuffers",
            egl::API.swap_buffers(self.display, surface),
        )
    }

    /// A `glow` context for the current EGL context.
    pub fn load_gl(&self) -> glow::Context {
        // SAFETY: the loader returns GL entry points from this EGL implementation (or null for
        // unknown names, which glow tolerates); the context is current on this thread when the
        // returned functions are called, as `GlesRenderer` requires.
        unsafe {
            glow::Context::from_loader_function(|name| {
                egl::API
                    .get_proc_address(name)
                    .map_or(std::ptr::null(), |f| f as *const c_void)
            })
        }
    }
}

impl Drop for Egl {
    fn drop(&mut self) {
        // The display is not terminated: EGL shares one display per native display, and other
        // `Egl` values (tests) may still use it. Process exit releases it.
        let _ = egl::API.make_current(self.display, None, None, None);
        if let Err(err) = egl::API.destroy_context(self.display, self.context) {
            tracing::warn!("eglDestroyContext failed: {err}");
        }
    }
}

fn client_extensions() -> &'static str {
    egl::API
        .query_string(None, egl::EXTENSIONS)
        .ok()
        .and_then(|s| s.to_str().ok())
        .unwrap_or("")
}

/// Finds the EGL device advertising `EGL_MESA_device_software` and opens a display on it.
fn software_device_display() -> Result<egl::Display, EglError> {
    let extensions = client_extensions();
    for ext in ["EGL_EXT_device_enumeration", "EGL_EXT_platform_device"] {
        if !extensions.split(' ').any(|e| e == ext) {
            return Err(EglError::MissingExtension(ext));
        }
    }
    let query_devices = egl::API
        .get_proc_address("eglQueryDevicesEXT")
        .ok_or(EglError::MissingExtension("eglQueryDevicesEXT"))?;
    let query_string = egl::API
        .get_proc_address("eglQueryDeviceStringEXT")
        .ok_or(EglError::MissingExtension("eglQueryDeviceStringEXT"))?;
    // SAFETY: both names come from EGL_EXT_device_enumeration / EGL_EXT_device_query, whose
    // specifications define exactly these signatures.
    let (query_devices, query_string) = unsafe {
        (
            std::mem::transmute::<extern "system" fn(), QueryDevicesExt>(query_devices),
            std::mem::transmute::<extern "system" fn(), QueryDeviceStringExt>(query_string),
        )
    };

    let mut devices = [std::ptr::null_mut::<c_void>(); 16];
    let mut count: egl::Int = 0;
    // SAFETY: `devices` has room for the 16 entries we announce, and `count` is a valid out
    // pointer.
    let ok = unsafe { query_devices(devices.len() as egl::Int, devices.as_mut_ptr(), &mut count) };
    if ok == 0 {
        return Err(EglError::NoSoftwareDevice);
    }
    let found = devices[..count.clamp(0, 16) as usize]
        .iter()
        .copied()
        .find(|&device| {
            // SAFETY: `device` was returned by eglQueryDevicesEXT above; the returned string is
            // owned by EGL and lives as long as the device.
            let ext = unsafe { query_string(device, egl::EXTENSIONS) };
            !ext.is_null()
                // SAFETY: non-null pointers from eglQueryDeviceStringEXT are NUL-terminated.
                && unsafe { CStr::from_ptr(ext) }
                    .to_str()
                    .is_ok_and(|s| s.split(' ').any(|e| e == "EGL_MESA_device_software"))
        })
        .ok_or(EglError::NoSoftwareDevice)?;

    // SAFETY: `found` is an EGLDeviceEXT from eglQueryDevicesEXT, the native display type for
    // EGL_PLATFORM_DEVICE_EXT; the attribute list is `NONE`-terminated.
    call("eglGetPlatformDisplay", unsafe {
        egl::API.get_platform_display(PLATFORM_DEVICE_EXT, found, &[egl::ATTRIB_NONE])
    })
}
