//! Single-session ownership for context-free libretro callbacks.

#![cfg_attr(
    not(feature = "libretro-linked"),
    allow(
        dead_code,
        reason = "the default host build has no statically linked core; production builds enable libretro-linked"
    )
)]

use std::cell::Cell;
use std::error::Error;
use std::ffi::{c_uint, c_void};
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;
use std::ptr;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

use retro_deck_platform::{
    audio::ApplicationPcm,
    input::KeyboardState,
    wayland::{PresentOutcome, WaylandPresentation},
};

use super::audio::{AudioBatchError, stereo_frames};
use super::environment::{Environment, EnvironmentError};
use super::video::{VideoCallbackError, VideoFrameLayout};
use super::{
    JoypadButton, JoypadState, LibretroCore, abi, joypad_from_keyboard, zx_keyboard_key_pressed,
};

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static TEST_SESSION: Mutex<()> = Mutex::new(());

thread_local! {
    static CALLBACK_STATE: Cell<*mut CallbackState> = const { Cell::new(ptr::null_mut()) };
}

#[derive(Debug)]
struct CallbackState {
    core: LibretroCore,
    environment: Environment,
    player_one: JoypadState,
    player_two: JoypadState,
    keyboard: KeyboardState,
    audio: Option<ApplicationPcm>,
    audio_batch_error: Option<AudioBatchError>,
    presentation: Option<WaylandPresentation>,
    video_error: Option<VideoCallbackError>,
}

/// Exclusive binding between one core session and its context-free callbacks.
#[derive(Debug)]
pub(super) struct CallbackBinding {
    state: Box<CallbackState>,
    not_send_or_sync: PhantomData<Rc<()>>,
}

impl CallbackBinding {
    /// Claim the process-wide libretro callback slot on the current thread.
    pub(super) fn install(
        core: LibretroCore,
        directory: &Path,
    ) -> Result<Self, CallbackBindingError> {
        SESSION_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| CallbackBindingError::AlreadyActive)?;

        let environment = match Environment::new(core, directory) {
            Ok(environment) => environment,
            Err(source) => {
                SESSION_ACTIVE.store(false, Ordering::Release);
                return Err(CallbackBindingError::Environment(source));
            }
        };
        let mut binding = Self {
            state: Box::new(CallbackState {
                core,
                environment,
                player_one: JoypadState::default(),
                player_two: JoypadState::default(),
                keyboard: KeyboardState::default(),
                audio: None,
                audio_batch_error: None,
                presentation: None,
                video_error: None,
            }),
            not_send_or_sync: PhantomData,
        };
        let pointer = ptr::from_mut(binding.state.as_mut());
        let installed = CALLBACK_STATE.with(|slot| {
            if slot.get().is_null() {
                slot.set(pointer);
                true
            } else {
                false
            }
        });
        if !installed {
            SESSION_ACTIVE.store(false, Ordering::Release);
            return Err(CallbackBindingError::AlreadyActive);
        }
        Ok(binding)
    }

    #[allow(
        clippy::unused_self,
        reason = "requiring a live binding to obtain callbacks documents the ownership invariant"
    )]
    pub(super) const fn environment_callback(&self) -> abi::EnvironmentCallback {
        environment_callback
    }

    #[allow(
        clippy::unused_self,
        reason = "requiring a live binding to obtain callbacks documents the ownership invariant"
    )]
    pub(super) const fn input_poll_callback(&self) -> abi::InputPollCallback {
        input_poll_callback
    }

    #[allow(
        clippy::unused_self,
        reason = "requiring a live binding to obtain callbacks documents the ownership invariant"
    )]
    pub(super) const fn input_state_callback(&self) -> abi::InputStateCallback {
        input_state_callback
    }

    #[allow(
        clippy::unused_self,
        reason = "requiring a live binding to obtain callbacks documents the ownership invariant"
    )]
    pub(super) const fn audio_sample_callback(&self) -> abi::AudioSampleCallback {
        audio_sample_callback
    }

    #[allow(
        clippy::unused_self,
        reason = "requiring a live binding to obtain callbacks documents the ownership invariant"
    )]
    pub(super) const fn audio_sample_batch_callback(&self) -> abi::AudioSampleBatchCallback {
        audio_sample_batch_callback
    }

    #[allow(
        clippy::unused_self,
        reason = "requiring a live binding to obtain callbacks documents the ownership invariant"
    )]
    pub(super) const fn video_refresh_callback(&self) -> abi::VideoRefreshCallback {
        video_refresh_callback
    }

    /// Attach the sole application PCM sender, returning it if one is present.
    #[must_use]
    pub(super) fn attach_audio(&mut self, audio: ApplicationPcm) -> Option<ApplicationPcm> {
        if self.state.audio.is_some() {
            Some(audio)
        } else {
            self.state.audio = Some(audio);
            None
        }
    }

    /// Detach the application PCM sender for explicit release.
    #[must_use]
    pub(super) fn take_audio(&mut self) -> Option<ApplicationPcm> {
        self.state.audio.take()
    }

    /// Borrow the application PCM sender for gate, volume, and diagnostics.
    pub(super) const fn audio(&self) -> Option<&ApplicationPcm> {
        self.state.audio.as_ref()
    }

    /// Take the first malformed batch observed since the previous call.
    pub(super) fn take_audio_batch_error(&mut self) -> Option<AudioBatchError> {
        self.state.audio_batch_error.take()
    }

    /// Attach the sole presentation, returning it unchanged if one is present.
    #[must_use]
    pub(super) fn attach_presentation(
        &mut self,
        presentation: WaylandPresentation,
    ) -> Option<WaylandPresentation> {
        if self.state.presentation.is_some() {
            Some(presentation)
        } else {
            self.state.presentation = Some(presentation);
            None
        }
    }

    /// Borrow the active presentation for polling and shutdown checks.
    pub(super) const fn presentation(&self) -> Option<&WaylandPresentation> {
        self.state.presentation.as_ref()
    }

    /// Borrow the active presentation for nonblocking event dispatch.
    pub(super) const fn presentation_mut(&mut self) -> Option<&mut WaylandPresentation> {
        self.state.presentation.as_mut()
    }

    /// Take the first frame or presentation failure since the previous call.
    pub(super) fn take_video_error(&mut self) -> Option<VideoCallbackError> {
        self.state.video_error.take()
    }

    pub(super) fn set_input(
        &mut self,
        player_one: JoypadState,
        player_two: JoypadState,
        keyboard: KeyboardState,
    ) {
        self.state.player_one = if self.state.core == LibretroCore::Fuse {
            player_one
        } else {
            player_one.merged(joypad_from_keyboard(keyboard))
        };
        self.state.player_two = player_two;
        self.state.keyboard = keyboard;
    }
}

impl Drop for CallbackBinding {
    fn drop(&mut self) {
        let pointer = ptr::from_mut(self.state.as_mut());
        CALLBACK_STATE.with(|slot| {
            if slot.get() == pointer {
                slot.set(ptr::null_mut());
            }
        });
        SESSION_ACTIVE.store(false, Ordering::Release);
    }
}

/// Failure to claim or initialize the single libretro callback session.
#[derive(Debug)]
pub enum CallbackBindingError {
    /// Another core session still owns process-global libretro symbols.
    AlreadyActive,
    /// Stable environment strings could not be prepared.
    Environment(EnvironmentError),
}

impl fmt::Display for CallbackBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyActive => formatter.write_str("a libretro core session is already active"),
            Self::Environment(source) => source.fmt(formatter),
        }
    }
}

impl Error for CallbackBindingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AlreadyActive => None,
            Self::Environment(source) => Some(source),
        }
    }
}

#[cfg(test)]
pub(super) fn serialize_test_sessions() -> MutexGuard<'static, ()> {
    TEST_SESSION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

unsafe extern "C" fn environment_callback(command: c_uint, data: *mut c_void) -> bool {
    CALLBACK_STATE.with(|slot| {
        // SAFETY: A non-null slot points to the binding's live boxed state,
        // and `CallbackBinding` cannot leave the installing thread.
        let Some(state) = (unsafe { slot.get().as_mut() }) else {
            return false;
        };
        // SAFETY: The trusted core supplies command-specific data, and the
        // binding guarantees exclusive same-thread access to this state.
        unsafe { state.environment.dispatch(command, data) }
    })
}

const unsafe extern "C" fn input_poll_callback() {}

unsafe extern "C" fn video_refresh_callback(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch_bytes: usize,
) {
    if data.is_null() {
        return;
    }
    CALLBACK_STATE.with(|slot| {
        // SAFETY: A non-null slot points to the binding's live boxed state,
        // and `CallbackBinding` cannot leave the installing thread.
        let Some(state) = (unsafe { slot.get().as_mut() }) else {
            return;
        };
        let layout = match VideoFrameLayout::new(
            state.environment.pixel_format(),
            width,
            height,
            pitch_bytes,
        ) {
            Ok(layout) => layout,
            Err(error) => {
                record_video_error(state, VideoCallbackError::Frame(error));
                return;
            }
        };
        // SAFETY: The trusted core owns the readable callback extent. The
        // checked layout rejects unaligned and excessive borrows.
        let frame = match unsafe { layout.frame(data) } {
            Ok(frame) => frame,
            Err(error) => {
                record_video_error(state, VideoCallbackError::Frame(error));
                return;
            }
        };
        let Some(presentation) = &mut state.presentation else {
            return;
        };
        match presentation.present(frame) {
            Ok(PresentOutcome::Submitted | PresentOutcome::Busy) => {}
            Err(error) => record_video_error(state, VideoCallbackError::Presentation(error)),
        }
    });
}

fn record_video_error(state: &mut CallbackState, error: VideoCallbackError) {
    if state.video_error.is_none() {
        state.video_error = Some(error);
    }
}

unsafe extern "C" fn audio_sample_callback(left: i16, right: i16) {
    CALLBACK_STATE.with(|slot| {
        // SAFETY: A non-null slot points to the binding's live boxed state,
        // and `CallbackBinding` cannot leave the installing thread.
        let Some(state) = (unsafe { slot.get().as_ref() }) else {
            return;
        };
        if let Some(audio) = &state.audio {
            audio.submit_stereo(&[[left, right]]);
        }
    });
}

unsafe extern "C" fn audio_sample_batch_callback(data: *const i16, frames: usize) -> usize {
    CALLBACK_STATE.with(|slot| {
        // SAFETY: A non-null slot points to the binding's live boxed state,
        // and `CallbackBinding` cannot leave the installing thread.
        let Some(state) = (unsafe { slot.get().as_mut() }) else {
            return frames;
        };
        // SAFETY: The trusted core owns this callback extent. The helper
        // rejects null, unaligned, and unreasonably large batches before use.
        match unsafe { stereo_frames(data, frames) } {
            Ok(samples) => {
                if let Some(audio) = &state.audio {
                    audio.submit_stereo(samples);
                }
            }
            Err(error) => {
                if state.audio_batch_error.is_none() {
                    state.audio_batch_error = Some(error);
                }
            }
        }
        frames
    })
}

unsafe extern "C" fn input_state_callback(
    port: c_uint,
    device: c_uint,
    index: c_uint,
    identifier: c_uint,
) -> i16 {
    CALLBACK_STATE.with(|slot| {
        // SAFETY: A non-null slot points to the binding's live boxed state,
        // and `CallbackBinding` cannot leave the installing thread.
        let Some(state) = (unsafe { slot.get().as_ref() }) else {
            return 0;
        };
        let device = device & abi::DEVICE_MASK;
        let Ok(port_index) = usize::try_from(port) else {
            return 0;
        };
        let Some(port_device) = state.core.input_ports().get(port_index).copied() else {
            return 0;
        };
        if device == abi::DEVICE_KEYBOARD {
            if !port_device.is_keyboard() || index != 0 {
                return 0;
            }
            return zx_keyboard_key_pressed(identifier, state.keyboard, state.player_one).into();
        }
        if device != abi::DEVICE_JOYPAD || !port_device.is_joypad() || index != 0 {
            return 0;
        }
        let joypad = match (state.core, port) {
            (LibretroCore::Fuse, 0) => state
                .player_one
                .without(JoypadButton::B)
                .without(JoypadButton::Select),
            (_, 0) => state.player_one,
            (_, 1) => state.player_two,
            _ => return 0,
        };
        i16::try_from(joypad.value(identifier)).unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_deck_audio::{SampleRate, Volume};
    use retro_deck_platform::input::{Button, ButtonSet, MediumRawKey};
    use std::ptr;
    use std::thread;

    #[test]
    fn binding_routes_callbacks_only_during_its_lifetime() {
        let _test_session = serialize_test_sessions();
        let callback;
        {
            let binding = CallbackBinding::install(LibretroCore::Fceumm, Path::new("/roms/nes"))
                .expect("first callback binding");
            callback = binding.environment_callback();
            let mut version = 0_u32;
            // SAFETY: `version` has the type required by this command.
            assert!(unsafe {
                callback(
                    abi::ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
                    ptr::from_mut(&mut version).cast(),
                )
            });
            assert_eq!(version, 2);
        }

        let mut version = 0_u32;
        // SAFETY: The callback rejects the request before accessing `version`
        // because no binding is active on this thread.
        assert!(!unsafe {
            callback(
                abi::ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
                ptr::from_mut(&mut version).cast(),
            )
        });
        assert_eq!(version, 0);
    }

    #[test]
    fn process_allows_only_one_active_session() {
        let _test_session = serialize_test_sessions();
        let first = CallbackBinding::install(LibretroCore::Gambatte, Path::new("/roms/gb"))
            .expect("first callback binding");
        assert!(matches!(
            CallbackBinding::install(LibretroCore::Fuse, Path::new("/roms/zx")),
            Err(CallbackBindingError::AlreadyActive)
        ));
        drop(first);
        assert!(CallbackBinding::install(LibretroCore::Fuse, Path::new("/roms/zx")).is_ok());
    }

    #[test]
    fn callbacks_from_an_unbound_thread_fail_closed() {
        let _test_session = serialize_test_sessions();
        let binding = CallbackBinding::install(LibretroCore::Fceumm, Path::new("/roms/nes"))
            .expect("first callback binding");
        let callback = binding.environment_callback();
        let result = thread::spawn(move || {
            let mut version = 0_u32;
            // SAFETY: The foreign callback is callable, but the new thread has
            // no state and therefore returns before dereferencing `version`.
            let accepted = unsafe {
                callback(
                    abi::ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
                    ptr::from_mut(&mut version).cast(),
                )
            };
            (accepted, version)
        })
        .join()
        .expect("callback test thread");
        assert_eq!(result, (false, 0));
    }

    #[test]
    fn negotiated_state_remains_owned_by_the_binding() {
        let _test_session = serialize_test_sessions();
        let binding = CallbackBinding::install(LibretroCore::Gambatte, Path::new("/roms/gb"))
            .expect("first callback binding");
        let callback = binding.environment_callback();
        let mut format = abi::PIXEL_FORMAT_RGB565;
        // SAFETY: `format` has the type required by this command.
        assert!(unsafe {
            callback(
                abi::ENVIRONMENT_SET_PIXEL_FORMAT,
                ptr::from_mut(&mut format).cast(),
            )
        });
        assert_eq!(
            binding.state.environment.pixel_format(),
            super::super::PixelFormat::Rgb565
        );
    }

    #[test]
    fn console_cores_merge_keyboard_controls_into_player_one() {
        let _test_session = serialize_test_sessions();
        let mut binding = CallbackBinding::install(LibretroCore::Fceumm, Path::new("/roms/nes"))
            .expect("first callback binding");
        let player_one = JoypadState::from_buttons(
            ButtonSet::empty()
                .with(Button::B, true)
                .with(Button::Left, true),
        );
        let player_two = JoypadState::from_buttons(ButtonSet::empty().with(Button::Start, true));
        let space = MediumRawKey::new(57).expect("Space key code");
        binding.set_input(
            player_one,
            player_two,
            KeyboardState::empty().with(space, true),
        );
        let callback = binding.input_state_callback();
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(0, abi::DEVICE_JOYPAD, 0, 8) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(0, abi::DEVICE_JOYPAD, 0, 0) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(0, abi::DEVICE_JOYPAD, 0, 6) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(1, abi::DEVICE_JOYPAD, 0, 3) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(0, abi::DEVICE_JOYPAD, 0, 256) }, 0x141);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(2, abi::DEVICE_JOYPAD, 0, 8) }, 0);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(0, abi::DEVICE_KEYBOARD, 0, 32) }, 0);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(0, abi::DEVICE_JOYPAD, 1, 8) }, 0);
    }

    #[test]
    fn zx_keeps_keyboard_and_joystick_queries_separate() {
        let _test_session = serialize_test_sessions();
        let mut binding = CallbackBinding::install(LibretroCore::Fuse, Path::new("/roms/zx"))
            .expect("first callback binding");
        let letter_a = MediumRawKey::new(30).expect("A key code");
        binding.set_input(
            JoypadState::from_buttons(
                ButtonSet::empty()
                    .with(Button::A, true)
                    .with(Button::B, true)
                    .with(Button::Start, true)
                    .with(Button::Select, true),
            ),
            JoypadState::from_buttons(ButtonSet::empty().with(Button::B, true)),
            KeyboardState::empty().with(letter_a, true),
        );
        let callback = binding.input_state_callback();
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(2, abi::DEVICE_KEYBOARD, 0, 97) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(2, abi::DEVICE_KEYBOARD, 0, 282) }, 0);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(2, abi::DEVICE_KEYBOARD, 0, 32) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(2, abi::DEVICE_KEYBOARD, 0, 304) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(2, abi::DEVICE_KEYBOARD, 0, 13) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(2, abi::DEVICE_KEYBOARD, 0, 306) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(0, abi::DEVICE_KEYBOARD, 0, 97) }, 0);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(2, abi::DEVICE_JOYPAD, 0, 8) }, 0);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(
            unsafe { callback(0, abi::device_subclass(abi::DEVICE_JOYPAD, 1), 0, 8) },
            1
        );
        // SAFETY: Fuse must not also interpret B as joystick Up.
        assert_eq!(unsafe { callback(0, abi::DEVICE_JOYPAD, 0, 0) }, 0);
        // SAFETY: Fuse must not use Select to open its keyboard overlay.
        assert_eq!(unsafe { callback(0, abi::DEVICE_JOYPAD, 0, 2) }, 0);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(1, abi::DEVICE_JOYPAD, 0, 0) }, 1);
        // SAFETY: The poll callback has no parameters or side effects.
        unsafe { binding.input_poll_callback()() };
    }

    #[test]
    fn audio_callbacks_consume_disabled_and_malformed_batches() {
        let _test_session = serialize_test_sessions();
        let mut binding = CallbackBinding::install(LibretroCore::Gambatte, Path::new("/roms/gb"))
            .expect("callback binding");
        let batch = binding.audio_sample_batch_callback();
        let samples = [1_i16, -1, 2, -2];
        // SAFETY: `samples` contains two aligned stereo frames.
        assert_eq!(unsafe { batch(samples.as_ptr(), 2) }, 2);
        // SAFETY: A null nonempty batch is rejected before dereference.
        assert_eq!(unsafe { batch(ptr::null(), 7) }, 7);
        // SAFETY: The callback rejects this excessive size before dereference.
        assert_eq!(unsafe { batch(samples.as_ptr(), 65_537) }, 65_537);
        assert_eq!(
            binding.take_audio_batch_error(),
            Some(AudioBatchError::NullData)
        );
        assert_eq!(binding.take_audio_batch_error(), None);

        let single = binding.audio_sample_callback();
        // SAFETY: The single-frame callback has no pointer parameters.
        unsafe { single(i16::MIN, i16::MAX) };
        drop(binding);
        // SAFETY: A stale callback consumes without accessing callback state.
        assert_eq!(unsafe { batch(ptr::null(), 3) }, 3);
    }

    #[test]
    fn audio_callbacks_submit_to_an_inactive_client_without_device_io() {
        let _test_session = serialize_test_sessions();
        let mut binding = CallbackBinding::install(LibretroCore::Fceumm, Path::new("/roms/nes"))
            .expect("callback binding");
        let rate = SampleRate::new(48_000).expect("valid sample rate");
        let audio = ApplicationPcm::silent(rate, Volume::MUTED);
        assert!(binding.attach_audio(audio).is_none());

        let single = binding.audio_sample_callback();
        // SAFETY: The single-frame callback has no pointer parameters.
        unsafe { single(100, -100) };
        let samples = [1_i16, -1, 2, -2];
        // SAFETY: `samples` contains two aligned stereo frames.
        assert_eq!(
            unsafe { binding.audio_sample_batch_callback()(samples.as_ptr(), 2) },
            2
        );

        let audio = binding.take_audio().expect("attached PCM sender");
        assert_eq!(audio.stats().inactive_dropped_samples, 3);
    }

    #[test]
    fn video_callbacks_skip_duplicates_and_record_the_first_invalid_frame() {
        let _test_session = serialize_test_sessions();
        let mut binding = CallbackBinding::install(LibretroCore::Fceumm, Path::new("/roms/nes"))
            .expect("callback binding");
        assert!(binding.presentation().is_none());
        assert!(binding.presentation_mut().is_none());
        let callback = binding.video_refresh_callback();

        // SAFETY: Null denotes a duplicate frame and is never dereferenced.
        unsafe { callback(ptr::null(), c_uint::MAX, c_uint::MAX, usize::MAX) };
        assert!(binding.take_video_error().is_none());

        let pixels = [0_u32; 4];
        // SAFETY: `pixels` is an aligned packed 2-by-2 XRGB8888 frame.
        unsafe { callback(pixels.as_ptr().cast(), 2, 2, 8) };
        assert!(binding.take_video_error().is_none());

        // SAFETY: The zero width is rejected before pixel memory is read.
        unsafe { callback(pixels.as_ptr().cast(), 0, 2, 8) };
        let unaligned = pixels.as_ptr().cast::<u8>().wrapping_add(1).cast();
        // SAFETY: The unaligned pointer is rejected before dereference, and
        // the earlier error remains the diagnostic until it is taken.
        unsafe { callback(unaligned, 2, 2, 8) };
        assert!(matches!(
            binding.take_video_error(),
            Some(VideoCallbackError::Frame(
                super::super::video::VideoFrameError::InvalidDimensions
            ))
        ));

        // SAFETY: The unaligned pointer is rejected before dereference.
        unsafe { callback(unaligned, 2, 2, 8) };
        assert!(matches!(
            binding.take_video_error(),
            Some(VideoCallbackError::Frame(
                super::super::video::VideoFrameError::UnalignedData
            ))
        ));
    }
}
