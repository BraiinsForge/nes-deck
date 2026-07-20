//! Single-session ownership for context-free libretro callbacks.

#![allow(
    dead_code,
    reason = "callback ownership is consumed by the executable in the next host migration slice"
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

use retro_deck_platform::input::KeyboardState;

use super::environment::{Environment, EnvironmentError};
use super::{
    JoypadState, LibretroCore, PixelFormat, abi, joypad_from_keyboard, medium_raw_key_for_retro,
};

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

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

    pub(super) const fn pixel_format(&self) -> PixelFormat {
        self.state.environment.pixel_format()
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
pub(super) enum CallbackBindingError {
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
        if device == abi::DEVICE_KEYBOARD {
            if state.core != LibretroCore::Fuse || port != 0 || index != 0 {
                return 0;
            }
            return medium_raw_key_for_retro(identifier)
                .is_some_and(|key| state.keyboard.contains(key))
                .into();
        }
        if device != abi::DEVICE_JOYPAD
            || index != 0
            || usize::try_from(port)
                .map_or(true, |port| port >= state.core.controller_ports().len())
        {
            return 0;
        }
        let joypad = match port {
            0 => state.player_one,
            1 => state.player_two,
            _ => return 0,
        };
        i16::try_from(joypad.value(identifier)).unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_deck_platform::input::{Button, ButtonSet, MediumRawKey};
    use std::sync::{Mutex, MutexGuard};
    use std::thread;

    static TEST_SESSION: Mutex<()> = Mutex::new(());

    fn serialize_sessions() -> MutexGuard<'static, ()> {
        TEST_SESSION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn binding_routes_callbacks_only_during_its_lifetime() {
        let _test_session = serialize_sessions();
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
        let _test_session = serialize_sessions();
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
        let _test_session = serialize_sessions();
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
        let _test_session = serialize_sessions();
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
        assert_eq!(binding.pixel_format(), PixelFormat::Rgb565);
    }

    #[test]
    fn console_cores_merge_keyboard_controls_into_player_one() {
        let _test_session = serialize_sessions();
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
        let _test_session = serialize_sessions();
        let mut binding = CallbackBinding::install(LibretroCore::Fuse, Path::new("/roms/zx"))
            .expect("first callback binding");
        let letter_a = MediumRawKey::new(30).expect("A key code");
        binding.set_input(
            JoypadState::from_buttons(ButtonSet::empty().with(Button::A, true)),
            JoypadState::from_buttons(ButtonSet::empty().with(Button::B, true)),
            KeyboardState::empty().with(letter_a, true),
        );
        let callback = binding.input_state_callback();
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(0, abi::DEVICE_KEYBOARD, 0, 97) }, 1);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(0, abi::DEVICE_KEYBOARD, 0, 282) }, 0);
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(
            unsafe { callback(0, abi::device_subclass(abi::DEVICE_JOYPAD, 1), 0, 8) },
            1
        );
        // SAFETY: Input callbacks have no pointer parameters.
        assert_eq!(unsafe { callback(1, abi::DEVICE_JOYPAD, 0, 0) }, 1);
        // SAFETY: The poll callback has no parameters or side effects.
        unsafe { binding.input_poll_callback()() };
    }
}
