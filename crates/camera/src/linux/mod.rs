//! Linux camera backend (FR-8.25): the xdg-desktop-portal Camera with PipeWire, or V4L2
//! directly when there is no camera portal.
//!
//! The path is chosen once per process, on first use: the portal when it reports a camera (or
//! always inside Flatpak, where `/dev/video*` is not reachable), otherwise V4L2. Both deliver
//! through the same mode choice, crop and conversion as the other backends ([`crate::mode`],
//! [`crate::convert`], [`crate::pump`]).

mod portal;
mod ranges;
mod v4l2;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use crate::{
    CameraBackend, CameraDevice, CameraError, CaptureRequest, CaptureSession, DeviceId, FrameSink,
    HotplugCallback, HotplugWatch,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Portal,
    V4l2,
}

enum PortalState {
    /// Access not asked yet, or the last connection ended.
    Idle,
    /// The user refused. Listing does not ask again; opening does ("Try again").
    Denied,
    Connected(portal::Connection),
}

/// The Linux [`CameraBackend`].
pub struct LinuxCamera {
    route: OnceLock<Route>,
    portal: Mutex<PortalState>,
    v4l2: v4l2::V4l2,
    hotplug: portal::HotplugSlot,
}

impl Default for LinuxCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxCamera {
    pub fn new() -> Self {
        Self {
            route: OnceLock::new(),
            portal: Mutex::new(PortalState::Idle),
            v4l2: v4l2::V4l2::default(),
            hotplug: Arc::new(Mutex::new(None)),
        }
    }

    fn route(&self) -> Route {
        *self.route.get_or_init(|| {
            let in_flatpak = Path::new("/.flatpak-info").exists();
            let route = match portal::probe() {
                Some(true) => Route::Portal,
                _ if in_flatpak => Route::Portal,
                _ => Route::V4l2,
            };
            tracing::info!(?route, in_flatpak, "camera backend");
            route
        })
    }

    fn portal(&self) -> MutexGuard<'_, PortalState> {
        self.portal.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Connects through the portal; this asks for access (the portal may show its dialog).
    fn connect(&self, state: &mut PortalState) -> Result<(), CameraError> {
        match portal::Connection::open(Arc::clone(&self.hotplug)) {
            Ok(connection) => {
                *state = PortalState::Connected(connection);
                Ok(())
            }
            Err(CameraError::PermissionDenied) => {
                *state = PortalState::Denied;
                Err(CameraError::PermissionDenied)
            }
            Err(err) => {
                *state = PortalState::Idle;
                Err(err)
            }
        }
    }

    fn portal_devices(&self) -> Result<Vec<CameraDevice>, CameraError> {
        let mut state = self.portal();
        match &*state {
            PortalState::Connected(connection) if connection.alive() => {
                return Ok(connection.devices());
            }
            PortalState::Denied => return Err(CameraError::PermissionDenied),
            PortalState::Connected(_) | PortalState::Idle => {}
        }
        // No dialog just to show an empty list.
        if portal::probe() != Some(true) {
            *state = PortalState::Idle;
            return Ok(Vec::new());
        }
        self.connect(&mut state)?;
        match &*state {
            PortalState::Connected(connection) => Ok(connection.devices()),
            PortalState::Idle | PortalState::Denied => Ok(Vec::new()),
        }
    }

    fn portal_open(
        &self,
        device: &DeviceId,
        request: CaptureRequest,
        sink: Box<dyn FrameSink>,
    ) -> Result<Box<dyn CaptureSession>, CameraError> {
        let mut state = self.portal();
        let connected = matches!(&*state, PortalState::Connected(connection) if connection.alive());
        if !connected {
            self.connect(&mut state)?;
        }
        match &*state {
            PortalState::Connected(connection) => connection.capture(device, request, sink),
            PortalState::Idle | PortalState::Denied => Err(CameraError::NoCamera),
        }
    }
}

/// Clears the hot-plug slot when the watcher goes away.
struct Watch(portal::HotplugSlot);

impl Drop for Watch {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

impl CameraBackend for LinuxCamera {
    fn devices(&self) -> Result<Vec<CameraDevice>, CameraError> {
        match self.route() {
            Route::Portal => self.portal_devices(),
            Route::V4l2 => Ok(self.v4l2.devices()),
        }
    }

    /// PipeWire reports added and removed camera nodes, so the portal path notifies once it is
    /// connected. Before that, and on the V4L2 path, the caller polls.
    fn watch(&self, changed: HotplugCallback) -> Result<Option<HotplugWatch>, CameraError> {
        if self.route() != Route::Portal {
            return Ok(None);
        }
        let connected = matches!(
            &*self.portal(),
            PortalState::Connected(connection) if connection.alive()
        );
        if !connected {
            return Ok(None);
        }
        *self.hotplug.lock().unwrap_or_else(PoisonError::into_inner) = Some(changed);
        Ok(Some(Box::new(Watch(Arc::clone(&self.hotplug)))))
    }

    fn open(
        &self,
        device: &DeviceId,
        request: CaptureRequest,
        sink: Box<dyn FrameSink>,
    ) -> Result<Box<dyn CaptureSession>, CameraError> {
        match self.route() {
            Route::Portal => self.portal_open(device, request, sink),
            Route::V4l2 => self.v4l2.open(device, request, sink),
        }
    }
}
