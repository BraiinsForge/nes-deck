//! Graceful process shutdown requested by standard termination signals.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::{SigId, flag, low_level};

/// Process-wide `SIGINT` and `SIGTERM` flag with scoped registrations.
#[derive(Debug)]
pub struct ShutdownFlag {
    requested: Arc<AtomicBool>,
    registrations: [SigId; 2],
}

impl ShutdownFlag {
    /// Install async-signal-safe handlers for graceful application exit.
    ///
    /// # Errors
    ///
    /// Returns the operating-system registration failure. A partial
    /// registration is removed before returning.
    pub fn install() -> io::Result<Self> {
        let requested = Arc::new(AtomicBool::new(false));
        let interrupt = flag::register(SIGINT, Arc::clone(&requested))?;
        let terminate = match flag::register(SIGTERM, Arc::clone(&requested)) {
            Ok(registration) => registration,
            Err(error) => {
                let _ = low_level::unregister(interrupt);
                return Err(error);
            }
        };
        Ok(Self {
            requested,
            registrations: [interrupt, terminate],
        })
    }

    /// Whether either registered signal has arrived.
    #[must_use]
    pub fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

impl Drop for ShutdownFlag {
    fn drop(&mut self) {
        for registration in self.registrations {
            let _ = low_level::unregister(registration);
        }
    }
}
