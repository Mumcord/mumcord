//! What the app steers on the own camera besides on and off: the device list and device, the
//! background effect and its quality, the preview picture, and the Settings preview that runs
//! without a server (FR-8.11, FR-8.29, FR-8.30, FR-8.40).

use std::path::PathBuf;
use std::sync::Arc;

use mumcord_camera::{CameraDevice, CameraError, DeviceId, DeviceList};
use mumcord_effects::{Background, EffectStats, EffectsConfig, EffectsError};
use mumcord_store::{CameraBackground, CameraBackgrounds, CameraEffectQuality, Settings};

use super::preview::PreviewSink;
use super::send::{CameraSend, SendCeiling, Stages};
use super::session::{CameraFailure, CameraState, Cameras, encoder_prefs};
use crate::screen_share::encoder::battery;
use crate::screen_share::encoder::natural_factory;
use crate::session::{Engine, lock};

/// The background when the default one can't be used (FR-8.15: a deleted own image falls back
/// to Blur).
fn fallback_background(blur_strength: u8) -> Background {
    Background::Blur {
        strength: blur_strength.min(100),
    }
}

/// The running pipeline: the preview dialog's or on-air camera, else the Settings preview.
fn running(cameras: &Cameras) -> Option<&CameraSend> {
    cameras
        .own
        .as_ref()
        .map(|own| &own.send)
        .or(cameras.local.as_ref())
}

impl Engine {
    /// Where the built-in background images are (`{dir}/{id}.jpg|png|webp`); the app passes its
    /// resource folder once at start.
    pub fn camera_set_builtin_backgrounds(&self, dir: PathBuf) {
        lock(self.camera_state()).builtin_backgrounds = Some(dir);
    }

    /// Sends the own camera's picture after the effect to `sink` (the preview dialog, the Settings
    /// preview and the self-view), or stops with `None`.
    pub fn camera_set_preview_sink(&self, sink: Option<PreviewSink>) {
        let tap = lock(self.camera_state()).preview.clone();
        tap.set_sink(sink);
    }

    /// The cameras of this computer, in OS order. Names are for the picker only (FR-8.28).
    pub fn camera_devices(&self) -> Result<Vec<CameraDevice>, CameraFailure> {
        let backend = lock(self.camera_state())
            .backend()
            .ok_or(CameraFailure::Camera(CameraError::NoCamera))?;
        backend.devices().map_err(CameraFailure::Camera)
    }

    /// Whether background effects can run on this PC; the error is the reason for "Background
    /// effects are not available on this PC: {reason}" (FR-8.33).
    pub fn camera_effects_available(&self) -> Result<(), String> {
        mumcord_effects::availability()
    }

    /// Resolves a background choice: built-in images from the app's resources, own images from
    /// the profile's background store (FR-8.14).
    pub fn camera_resolve_background(
        &self,
        setting: &CameraBackground,
        blur_strength: u8,
    ) -> Result<Background, EffectsError> {
        let builtin = lock(self.camera_state())
            .builtin_backgrounds
            .clone()
            .unwrap_or_default();
        let own = CameraBackgrounds::new(self.data_dir());
        mumcord_effects::resolve(setting, blur_strength, &builtin, &own)
    }

    /// The default background of the settings (FR-8.76); Blur when it can't be used.
    pub(super) fn camera_default_background(&self, settings: &Settings) -> Background {
        self.camera_resolve_background(
            &settings.camera_default_background,
            settings.camera_blur_strength,
        )
        .unwrap_or_else(|e| {
            tracing::info!(error = %e, "default camera background not usable, using Blur");
            fallback_background(settings.camera_blur_strength)
        })
    }

    /// Opens `device` (else the remembered camera, else the OS default, FR-8.27) with the
    /// effects and the preview.
    pub(super) fn camera_send_start(
        &self,
        cameras: &mut Cameras,
        settings: &Settings,
        device: Option<DeviceId>,
        background: Background,
    ) -> Result<CameraSend, CameraFailure> {
        let ceiling = SendCeiling::from_setting(settings.camera_send_quality);
        let backend = cameras
            .backend()
            .ok_or(CameraFailure::Camera(CameraError::NoCamera))?;
        let mut devices = DeviceList::new();
        devices.update(backend.devices().map_err(CameraFailure::Camera)?);
        let wanted =
            device.or_else(|| settings.camera_device_id.clone().map(DeviceId::from_stored));
        let device = devices
            .pick(wanted.as_ref())
            .ok_or(CameraFailure::Camera(CameraError::NoCamera))?
            .id
            .clone();
        let selection = self
            .screen_encoders()
            .select(settings, encoder_prefs(ceiling));
        let encoders = natural_factory(selection.candidates, Arc::default());
        let stages = Stages {
            effects: Some(EffectsConfig {
                fps: ceiling.fps,
                quality: settings.camera_effect_quality,
                background: background.clone(),
                on_battery: Arc::new(battery::on_battery),
            }),
            preview: Some(cameras.preview.clone()),
        };
        let send = CameraSend::start(backend.as_ref(), &device, ceiling, encoders, stages)
            .map_err(CameraFailure::Camera)?;
        cameras.background = Some(background);
        Ok(send)
    }

    /// Starts the Settings → Camera preview (FR-8.30): the camera runs on this device only, with
    /// the default background. `Busy` while the preview dialog or the camera runs (FR-8.29).
    pub fn camera_local_preview_start(&self) -> Result<(), CameraFailure> {
        let settings = self.encoder_settings();
        let background = self.camera_default_background(&settings);
        let mut cameras = lock(self.camera_state());
        if cameras.own.is_some() {
            return Err(CameraFailure::Busy);
        }
        if cameras.local.is_none() {
            let send = self.camera_send_start(&mut cameras, &settings, None, background)?;
            cameras.local = Some(send);
        }
        Ok(())
    }

    /// Stops the Settings preview (the page or the Settings window closed); the camera light
    /// goes off unless the preview dialog took the camera over.
    pub fn camera_local_preview_stop(&self) {
        let local = lock(self.camera_state()).local.take();
        drop(local);
    }

    /// Switches the previewed camera to `device` (the picker of the preview dialog and of
    /// Settings). The old camera keeps running when the new one can't be opened. `Busy` while
    /// the camera is on air or being turned on; nothing happens while no camera runs.
    pub fn camera_set_device(&self, device: DeviceId) -> Result<(), CameraFailure> {
        let settings = self.encoder_settings();
        let mut cameras = lock(self.camera_state());
        if cameras
            .own
            .as_ref()
            .is_some_and(|own| own.state != CameraState::Preview)
        {
            return Err(CameraFailure::Busy);
        }
        if cameras.own.is_none() && cameras.local.is_none() {
            return Ok(());
        }
        let background = cameras
            .background
            .clone()
            .unwrap_or_else(|| fallback_background(settings.camera_blur_strength));
        let send = self.camera_send_start(&mut cameras, &settings, Some(device), background)?;
        // The old pipeline stops (its camera light goes off) when it is replaced here.
        let old = match cameras.own.as_mut() {
            Some(own) => std::mem::replace(&mut own.send, send),
            None => match cameras.local.replace(send) {
                Some(old) => old,
                None => return Ok(()),
            },
        };
        drop(cameras);
        drop(old);
        Ok(())
    }

    /// Changes the background of the running camera, also on air (FR-8.40). Without a running
    /// camera the choice is not kept: the next preview starts with the default (FR-8.76).
    pub fn camera_set_background(&self, background: Background) {
        let mut cameras = lock(self.camera_state());
        if let Some(effects) = running(&cameras).and_then(CameraSend::effects) {
            effects.set_background(background.clone());
            cameras.background = Some(background);
        }
    }

    /// Changes the effect quality of the running camera; Auto measures again (FR-8.35).
    pub fn camera_set_effect_quality(&self, quality: CameraEffectQuality) {
        let cameras = lock(self.camera_state());
        if let Some(effects) = running(&cameras).and_then(CameraSend::effects) {
            effects.set_quality(quality);
        }
    }

    /// The load line of the preview (FR-8.36): level, load, "too busy" and why effects are off;
    /// `None` while no camera runs.
    pub fn camera_effect_stats(&self) -> Option<EffectStats> {
        let cameras = lock(self.camera_state());
        running(&cameras)
            .and_then(CameraSend::effects)
            .map(mumcord_effects::Effects::stats)
    }
}
