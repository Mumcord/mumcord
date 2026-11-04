//! Camera devices: stable ids, the current list, and hot-plug events (FR-8.27, FR-8.28).

use std::fmt;

/// Stable, opaque camera id. It is what Settings remember as the last used camera (FR-8.27).
///
/// It is derived from a key the OS keeps stable for the same physical camera across replugs and
/// restarts (Windows symbolic link, macOS `uniqueID`, Linux portal node serial or
/// `/dev/v4l/by-id` path), hashed so the raw OS path never reaches the settings file. Device ids
/// and names are shown only in the UI and never logged or sent (FR-8.28), so `Debug` hides them.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    /// Builds the id for a camera. `backend` names the capture API (for example `"mf"`,
    /// `"avf"`, `"portal"`, `"v4l2"`) so two APIs that describe the same camera differently
    /// cannot collide; `os_key` is the OS's stable key for the camera.
    pub fn from_os_key(backend: &str, os_key: &str) -> Self {
        Self(format!("{backend}-{:016x}", fnv1a(backend, os_key)))
    }

    /// Restores an id read back from the settings.
    pub fn from_stored(stored: impl Into<String>) -> Self {
        Self(stored.into())
    }

    /// The text to store in the settings.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceId(..)")
    }
}

/// 64-bit FNV-1a: a fixed, documented hash, so ids stay identical across Rust releases (the
/// standard library's hasher makes no such promise).
fn fnv1a(backend: &str, os_key: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let bytes = backend.bytes().chain([0]).chain(os_key.bytes());
    bytes.fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

/// One camera as the OS reports it.
#[derive(Clone, PartialEq, Eq)]
pub struct CameraDevice {
    pub id: DeviceId,
    /// Display name for the pickers; UI only (FR-8.28).
    pub name: String,
    /// The OS's default camera, used when the remembered one is missing (FR-8.27).
    pub is_default: bool,
}

impl fmt::Debug for CameraDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CameraDevice")
            .field("is_default", &self.is_default)
            .finish_non_exhaustive()
    }
}

/// A change between two device list snapshots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceEvent {
    Removed(DeviceId),
    /// Same camera, new name or default flag.
    Changed(CameraDevice),
    Added(CameraDevice),
}

/// The current cameras, kept in OS order.
#[derive(Clone, Debug, Default)]
pub struct DeviceList {
    devices: Vec<CameraDevice>,
}

impl DeviceList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn devices(&self) -> &[CameraDevice] {
        &self.devices
    }

    pub fn get(&self, id: &DeviceId) -> Option<&CameraDevice> {
        self.devices.iter().find(|device| &device.id == id)
    }

    /// Replaces the list with a fresh enumeration and returns what changed, in a fixed order:
    /// all removals (old order), then changes and additions (new order). Removals come first so
    /// a consumer that stops the session of a vanished camera does so before it sees a new one.
    ///
    /// Duplicate ids in `snapshot` keep only their first entry; backends are expected to give
    /// every camera its own key.
    pub fn update(&mut self, snapshot: Vec<CameraDevice>) -> Vec<DeviceEvent> {
        let mut next: Vec<CameraDevice> = Vec::with_capacity(snapshot.len());
        for device in snapshot {
            if !next.iter().any(|known| known.id == device.id) {
                next.push(device);
            }
        }

        let mut events: Vec<DeviceEvent> = self
            .devices
            .iter()
            .filter(|old| !next.iter().any(|new| new.id == old.id))
            .map(|old| DeviceEvent::Removed(old.id.clone()))
            .collect();
        for new in &next {
            match self.get(&new.id) {
                Some(old) if old == new => {}
                Some(_) => events.push(DeviceEvent::Changed(new.clone())),
                None => events.push(DeviceEvent::Added(new.clone())),
            }
        }

        self.devices = next;
        events
    }

    /// The camera to open: the remembered one if it is present, else the OS default, else the
    /// first camera (FR-8.27).
    pub fn pick(&self, remembered: Option<&DeviceId>) -> Option<&CameraDevice> {
        remembered
            .and_then(|id| self.get(id))
            .or_else(|| self.devices.iter().find(|device| device.is_default))
            .or_else(|| self.devices.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(key: &str, name: &str, is_default: bool) -> CameraDevice {
        CameraDevice {
            id: DeviceId::from_os_key("v4l2", key),
            name: name.to_owned(),
            is_default,
        }
    }

    #[test]
    fn ids_are_stable_and_distinct() {
        let a = DeviceId::from_os_key("v4l2", "/dev/v4l/by-id/usb-Cam_A-video-index0");
        assert_eq!(
            a,
            DeviceId::from_os_key("v4l2", "/dev/v4l/by-id/usb-Cam_A-video-index0")
        );
        // Pinned value: a changed hash would forget every user's remembered camera.
        assert_eq!(a.as_str(), "v4l2-cf6b0725b104fe88");
        assert_ne!(
            a,
            DeviceId::from_os_key("v4l2", "/dev/v4l/by-id/usb-Cam_B-video-index0")
        );
        assert_ne!(
            a,
            DeviceId::from_os_key("portal", "/dev/v4l/by-id/usb-Cam_A-video-index0")
        );
        // The separator keeps "ab" + "c" apart from "a" + "bc".
        assert_ne!(
            DeviceId::from_os_key("ab", "c"),
            DeviceId::from_os_key("a", "bc")
        );
    }

    #[test]
    fn ids_survive_a_settings_round_trip_and_hide_in_debug() {
        let id = DeviceId::from_os_key("mf", r"\\?\usb#vid_046d&pid_0825#cam");
        assert_eq!(DeviceId::from_stored(id.as_str().to_owned()), id);
        assert!(
            !id.as_str().contains("vid_046d"),
            "the raw OS key is not stored"
        );
        assert_eq!(format!("{id:?}"), "DeviceId(..)");
        let device = cam("k", "Jane's Webcam", false);
        assert!(!format!("{device:?}").contains("Jane"));
    }

    #[test]
    fn same_camera_keeps_its_id_when_the_os_reorders_the_list() {
        let mut list = DeviceList::new();
        list.update(vec![cam("a", "A", true), cam("b", "B", false)]);
        let events = list.update(vec![cam("b", "B", false), cam("a", "A", true)]);
        assert!(
            events.is_empty(),
            "reordering is not a hot-plug: {events:?}"
        );
        assert_eq!(list.devices()[0].id, DeviceId::from_os_key("v4l2", "b"));
    }

    #[test]
    fn hot_plug_events_list_removals_first_then_changes_and_additions_in_os_order() {
        let mut list = DeviceList::new();
        assert_eq!(
            list.update(vec![cam("a", "A", true), cam("b", "B", false)]),
            vec![
                DeviceEvent::Added(cam("a", "A", true)),
                DeviceEvent::Added(cam("b", "B", false)),
            ]
        );

        let events = list.update(vec![
            cam("c", "C", false),
            cam("b", "B", true),
            cam("d", "D", false),
        ]);
        assert_eq!(
            events,
            vec![
                DeviceEvent::Removed(DeviceId::from_os_key("v4l2", "a")),
                DeviceEvent::Added(cam("c", "C", false)),
                DeviceEvent::Changed(cam("b", "B", true)),
                DeviceEvent::Added(cam("d", "D", false)),
            ]
        );

        assert_eq!(
            list.update(Vec::new()),
            vec![
                DeviceEvent::Removed(DeviceId::from_os_key("v4l2", "c")),
                DeviceEvent::Removed(DeviceId::from_os_key("v4l2", "b")),
                DeviceEvent::Removed(DeviceId::from_os_key("v4l2", "d")),
            ]
        );
    }

    #[test]
    fn duplicate_ids_keep_the_first_entry() {
        let mut list = DeviceList::new();
        let events = list.update(vec![cam("a", "First", false), cam("a", "Second", false)]);
        assert_eq!(events, vec![DeviceEvent::Added(cam("a", "First", false))]);
        assert_eq!(list.devices().len(), 1);
    }

    #[test]
    fn pick_prefers_remembered_then_default_then_first() {
        let mut list = DeviceList::new();
        assert!(list.pick(None).is_none());
        list.update(vec![cam("a", "A", false), cam("b", "B", true)]);
        let a = DeviceId::from_os_key("v4l2", "a");
        let gone = DeviceId::from_os_key("v4l2", "gone");
        assert_eq!(list.pick(Some(&a)).map(|d| d.name.as_str()), Some("A"));
        assert_eq!(list.pick(Some(&gone)).map(|d| d.name.as_str()), Some("B"));
        assert_eq!(list.pick(None).map(|d| d.name.as_str()), Some("B"));
        list.update(vec![cam("a", "A", false)]);
        assert_eq!(list.pick(Some(&gone)).map(|d| d.name.as_str()), Some("A"));
    }
}
