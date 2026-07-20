//! Bounded exclusive keyboard ownership for dashboard navigation.

use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};

use evdev::{AttributeSetRef, Device, EventSummary, KeyCode};

use super::linux::event_paths;
use super::{InputError, KeyboardKey};

const INPUT_DIRECTORY: &str = "/dev/input";
const MAXIMUM_KEYBOARDS: usize = 4;
const MAXIMUM_EVENTS_PER_DRAIN: usize = 64;

/// Result of one explicit dashboard-keyboard hotplug scan.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyboardScanStats {
    attached: usize,
    connected: usize,
}

impl KeyboardScanStats {
    /// Keyboards newly opened and exclusively grabbed by this scan.
    #[must_use]
    pub const fn attached(self) -> usize {
        self.attached
    }

    /// Keyboards connected after the scan.
    #[must_use]
    pub const fn connected(self) -> usize {
        self.connected
    }
}

/// Result of one bounded nonblocking keyboard drain.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyboardDrainStats {
    emitted: usize,
    dropped: usize,
    disconnected: usize,
}

impl KeyboardDrainStats {
    /// Navigation events appended to the caller's buffer.
    #[must_use]
    pub const fn emitted(self) -> usize {
        self.emitted
    }

    /// Navigation events consumed after the fixed output bound was full.
    #[must_use]
    pub const fn dropped(self) -> usize {
        self.dropped
    }

    /// Keyboards closed after an unrecoverable read failure.
    #[must_use]
    pub const fn disconnected(self) -> usize {
        self.disconnected
    }
}

#[derive(Debug)]
struct KeyboardDevice {
    device: Device,
    path: PathBuf,
    tracker: KeyboardTracker,
}

/// Up to four exclusively grabbed keyboards suitable for dashboard navigation.
///
/// Ownership is deliberately separate from controllers. The dashboard drops
/// all keyboard descriptors before starting a child so games, terminals, and
/// language environments receive the original keyboard directly.
#[derive(Debug)]
pub struct KeyboardDevices {
    input_directory: PathBuf,
    keyboards: Vec<KeyboardDevice>,
}

impl KeyboardDevices {
    /// Discover and exclusively grab suitable keyboards below `/dev/input`.
    ///
    /// Devices that cannot be opened, queried, made nonblocking, or grabbed
    /// are skipped. An empty set is valid.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Scan`] only when the input directory cannot be
    /// enumerated safely.
    pub fn discover() -> Result<Self, InputError> {
        Self::discover_in(Path::new(INPUT_DIRECTORY))
    }

    fn discover_in(input_directory: &Path) -> Result<Self, InputError> {
        let mut devices = Self {
            input_directory: input_directory.to_owned(),
            keyboards: Vec::new(),
        };
        let _stats = devices.rescan()?;
        Ok(devices)
    }

    /// Number of keyboards currently owned by the dashboard.
    #[must_use]
    pub fn keyboard_count(&self) -> usize {
        self.keyboards.len()
    }

    /// Borrow descriptors for inclusion in the dashboard's aggregate poll.
    pub fn file_descriptors(&self) -> impl Iterator<Item = BorrowedFd<'_>> + '_ {
        self.keyboards
            .iter()
            .map(|keyboard| keyboard.device.as_fd())
    }

    /// Scan for new keyboards without replacing connected descriptors.
    ///
    /// The sorted event-node order is stable. A candidate is accepted only
    /// after an exclusive grab and an initial key-state query both succeed.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Scan`] without disturbing connected keyboards
    /// when the input directory cannot be enumerated.
    pub fn rescan(&mut self) -> Result<KeyboardScanStats, InputError> {
        let paths = event_paths(&self.input_directory)?;
        let mut attached = 0;
        for path in paths {
            if self.keyboards.len() >= MAXIMUM_KEYBOARDS {
                break;
            }
            if self.keyboards.iter().any(|keyboard| keyboard.path == path) {
                continue;
            }
            let Ok(device) = Device::open(&path) else {
                continue;
            };
            let Some(keyboard) = configure_keyboard(device, path) else {
                continue;
            };
            self.keyboards.push(keyboard);
            attached += 1;
        }
        Ok(KeyboardScanStats {
            attached,
            connected: self.keyboards.len(),
        })
    }

    /// Drain every available keyboard report without waiting.
    ///
    /// At most 64 normalized presses or permitted repeats are appended. All
    /// remaining reports are still consumed so a noisy device cannot grow the
    /// buffer or replay stale navigation later. Broken devices are closed and
    /// reported rather than failing the dashboard.
    pub fn drain_into(&mut self, output: &mut Vec<KeyboardKey>) -> KeyboardDrainStats {
        let mut collector = KeyboardCollector::new(output);
        let mut index = 0;
        while index < self.keyboards.len() {
            let drained = self.keyboards.get_mut(index).map_or(Ok(()), |keyboard| {
                drain_keyboard(keyboard, &mut |key| collector.emit(key))
            });
            if drained.is_ok() {
                index += 1;
            } else {
                self.keyboards.remove(index);
                collector.disconnect();
            }
        }
        collector.stats()
    }

    /// Release all exclusive grabs before a managed child starts.
    ///
    /// A later [`Self::rescan`] reacquires keyboards that remain connected.
    pub fn release_for_child(&mut self) {
        self.keyboards.clear();
    }
}

impl Default for KeyboardDevices {
    fn default() -> Self {
        Self {
            input_directory: PathBuf::from(INPUT_DIRECTORY),
            keyboards: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct KeyboardTracker {
    left_shift: bool,
    right_shift: bool,
}

impl KeyboardTracker {
    fn from_key_state(keys: &AttributeSetRef<KeyCode>) -> Self {
        Self {
            left_shift: keys.contains(KeyCode::KEY_LEFTSHIFT),
            right_shift: keys.contains(KeyCode::KEY_RIGHTSHIFT),
        }
    }

    fn resynchronize(&mut self, keys: &AttributeSetRef<KeyCode>) {
        *self = Self::from_key_state(keys);
    }

    fn apply(&mut self, code: KeyCode, value: i32) -> Option<KeyboardKey> {
        if code == KeyCode::KEY_LEFTSHIFT {
            self.left_shift = value != 0;
            return None;
        }
        if code == KeyCode::KEY_RIGHTSHIFT {
            self.right_shift = value != 0;
            return None;
        }
        let fresh_press = value == 1;
        let permitted_repeat = value == 2 && key_repeats(code);
        if !fresh_press && !permitted_repeat {
            return None;
        }
        key_to_navigation(code, self.left_shift || self.right_shift)
    }
}

struct KeyboardCollector<'output> {
    output: &'output mut Vec<KeyboardKey>,
    emitted: usize,
    dropped: usize,
    disconnected: usize,
}

impl<'output> KeyboardCollector<'output> {
    const fn new(output: &'output mut Vec<KeyboardKey>) -> Self {
        Self {
            output,
            emitted: 0,
            dropped: 0,
            disconnected: 0,
        }
    }

    fn emit(&mut self, key: KeyboardKey) {
        if self.emitted < MAXIMUM_EVENTS_PER_DRAIN {
            self.output.push(key);
            self.emitted += 1;
        } else {
            self.dropped += 1;
        }
    }

    const fn disconnect(&mut self) {
        self.disconnected += 1;
    }

    const fn stats(&self) -> KeyboardDrainStats {
        KeyboardDrainStats {
            emitted: self.emitted,
            dropped: self.dropped,
            disconnected: self.disconnected,
        }
    }
}

fn configure_keyboard(mut device: Device, path: PathBuf) -> Option<KeyboardDevice> {
    if !device.supported_keys().is_some_and(is_dashboard_keyboard) {
        return None;
    }
    device.set_nonblocking(true).ok()?;
    device.grab().ok()?;
    let keys = device.get_key_state().ok()?;
    Some(KeyboardDevice {
        device,
        path,
        tracker: KeyboardTracker::from_key_state(&keys),
    })
}

fn is_dashboard_keyboard(keys: &AttributeSetRef<KeyCode>) -> bool {
    keys.contains(KeyCode::KEY_ENTER)
        && keys.contains(KeyCode::KEY_ESC)
        && keys.contains(KeyCode::KEY_TAB)
        && keys.contains(KeyCode::KEY_UP)
        && keys.contains(KeyCode::KEY_DOWN)
        && keys.contains(KeyCode::KEY_LEFT)
        && keys.contains(KeyCode::KEY_RIGHT)
        && (keys.contains(KeyCode::KEY_LEFTSHIFT) || keys.contains(KeyCode::KEY_RIGHTSHIFT))
}

const fn key_repeats(code: KeyCode) -> bool {
    code.0 == KeyCode::KEY_UP.0
        || code.0 == KeyCode::KEY_DOWN.0
        || code.0 == KeyCode::KEY_LEFT.0
        || code.0 == KeyCode::KEY_RIGHT.0
}

const fn key_to_navigation(code: KeyCode, shift: bool) -> Option<KeyboardKey> {
    match code {
        KeyCode::KEY_ENTER | KeyCode::KEY_KPENTER => Some(KeyboardKey::Enter),
        KeyCode::KEY_ESC => Some(KeyboardKey::Escape),
        KeyCode::KEY_UP => Some(KeyboardKey::Up),
        KeyCode::KEY_DOWN => Some(KeyboardKey::Down),
        KeyCode::KEY_LEFT => Some(KeyboardKey::Left),
        KeyCode::KEY_RIGHT => Some(KeyboardKey::Right),
        KeyCode::KEY_TAB if shift => Some(KeyboardKey::BackTab),
        KeyCode::KEY_TAB => Some(KeyboardKey::Tab),
        _ => None,
    }
}

fn drain_keyboard(
    keyboard: &mut KeyboardDevice,
    emit: &mut impl FnMut(KeyboardKey),
) -> io::Result<()> {
    loop {
        let events = match keyboard.device.fetch_events() {
            Ok(events) => events,
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(source) => return Err(source),
        };
        let mut count = 0;
        for event in events {
            count += 1;
            let EventSummary::Key(_, code, value) = event.destructure() else {
                continue;
            };
            if let Some(key) = keyboard.tracker.apply(code, value) {
                emit(key);
            }
        }
        if count == 0 {
            let keys = keyboard.device.get_key_state()?;
            keyboard.tracker.resynchronize(&keys);
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use evdev::AttributeSet;

    use super::*;

    fn complete_keyboard() -> AttributeSet<KeyCode> {
        [
            KeyCode::KEY_ENTER,
            KeyCode::KEY_ESC,
            KeyCode::KEY_TAB,
            KeyCode::KEY_UP,
            KeyCode::KEY_DOWN,
            KeyCode::KEY_LEFT,
            KeyCode::KEY_RIGHT,
            KeyCode::KEY_LEFTSHIFT,
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn capability_filter_rejects_partial_key_interfaces() {
        let complete = complete_keyboard();
        assert!(is_dashboard_keyboard(&complete));

        for missing in [
            KeyCode::KEY_ENTER,
            KeyCode::KEY_ESC,
            KeyCode::KEY_TAB,
            KeyCode::KEY_UP,
            KeyCode::KEY_DOWN,
            KeyCode::KEY_LEFT,
            KeyCode::KEY_RIGHT,
            KeyCode::KEY_LEFTSHIFT,
        ] {
            let mut partial = complete.clone();
            partial.remove(missing);
            assert!(!is_dashboard_keyboard(&partial));
        }

        let mut right_shift_only = complete;
        right_shift_only.remove(KeyCode::KEY_LEFTSHIFT);
        right_shift_only.insert(KeyCode::KEY_RIGHTSHIFT);
        assert!(is_dashboard_keyboard(&right_shift_only));
    }

    #[test]
    fn tracker_emits_presses_and_only_directional_repeats() {
        let mut tracker = KeyboardTracker::default();
        assert_eq!(
            tracker.apply(KeyCode::KEY_ENTER, 1),
            Some(KeyboardKey::Enter)
        );
        assert_eq!(tracker.apply(KeyCode::KEY_ENTER, 2), None);
        assert_eq!(tracker.apply(KeyCode::KEY_ENTER, 0), None);
        assert_eq!(tracker.apply(KeyCode::KEY_LEFT, 2), Some(KeyboardKey::Left));
        assert_eq!(tracker.apply(KeyCode::KEY_TAB, 2), None);
    }

    #[test]
    fn either_shift_changes_tab_and_releases_cleanly() {
        let mut tracker = KeyboardTracker::default();
        assert_eq!(tracker.apply(KeyCode::KEY_LEFTSHIFT, 1), None);
        assert_eq!(
            tracker.apply(KeyCode::KEY_TAB, 1),
            Some(KeyboardKey::BackTab)
        );
        assert_eq!(tracker.apply(KeyCode::KEY_RIGHTSHIFT, 1), None);
        assert_eq!(tracker.apply(KeyCode::KEY_LEFTSHIFT, 0), None);
        assert_eq!(
            tracker.apply(KeyCode::KEY_TAB, 1),
            Some(KeyboardKey::BackTab)
        );
        assert_eq!(tracker.apply(KeyCode::KEY_RIGHTSHIFT, 0), None);
        assert_eq!(tracker.apply(KeyCode::KEY_TAB, 1), Some(KeyboardKey::Tab));
    }

    #[test]
    fn collector_has_a_fixed_bound_without_losing_consumption_counts() {
        let mut output = vec![KeyboardKey::Enter];
        let mut collector = KeyboardCollector::new(&mut output);
        for _ in 0..(MAXIMUM_EVENTS_PER_DRAIN + 3) {
            collector.emit(KeyboardKey::Down);
        }
        collector.disconnect();
        assert_eq!(
            collector.stats(),
            KeyboardDrainStats {
                emitted: MAXIMUM_EVENTS_PER_DRAIN,
                dropped: 3,
                disconnected: 1,
            }
        );
        assert_eq!(output.len(), MAXIMUM_EVENTS_PER_DRAIN + 1);
    }

    #[test]
    fn empty_keyboard_set_is_a_valid_fallback() {
        let mut keyboards = KeyboardDevices::default();
        let mut output = Vec::new();
        assert_eq!(keyboards.keyboard_count(), 0);
        assert_eq!(
            keyboards.drain_into(&mut output),
            KeyboardDrainStats::default()
        );
        assert!(output.is_empty());
    }
}
