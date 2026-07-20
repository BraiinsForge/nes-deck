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

use super::environment::{Environment, EnvironmentError};
use super::{LibretroCore, PixelFormat, abi};

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

thread_local! {
    static CALLBACK_STATE: Cell<*mut CallbackState> = const { Cell::new(ptr::null_mut()) };
}

#[derive(Debug)]
struct CallbackState {
    environment: Environment,
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
            state: Box::new(CallbackState { environment }),
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
