use crate::{input, wayland};
use rustix::event::{PollFd, Timespec, poll};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputDispatch {
    pub ready: bool,
    pub control_count: usize,
    pub touch_count: usize,
    pub touch_lost: bool,
    pub rescan: bool,
    pub shutdown: bool,
}

pub fn dispatch(wayland_backend: bool, timeout_ms: u32) -> Result<InputDispatch, String> {
    if wayland_backend {
        wayland::dispatch_inputs(timeout_ms)
    } else {
        input::dispatch_inputs(timeout_ms)
    }
}

pub(crate) fn wait(descriptors: &mut [PollFd<'_>], timeout_ms: u32) -> Result<usize, String> {
    wait_for(descriptors, Duration::from_millis(u64::from(timeout_ms)))
}

pub(crate) fn wait_for(descriptors: &mut [PollFd<'_>], timeout: Duration) -> Result<usize, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = Timespec {
            tv_sec: remaining.as_secs() as i64,
            tv_nsec: i64::from(remaining.subsec_nanos()),
        };
        match poll(descriptors, Some(&timeout)) {
            Ok(ready) => return Ok(ready),
            Err(rustix::io::Errno::INTR) if Instant::now() < deadline => continue,
            Err(rustix::io::Errno::INTR) => return Ok(0),
            Err(error) => return Err(format!("cannot poll dashboard input: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls;
    use rustix::event::PollFlags;
    use std::io::Write;
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixStream;

    #[test]
    fn polls_one_shared_readiness_snapshot() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(&[1]).unwrap();
        let mut descriptors = [PollFd::from_borrowed_fd(reader.as_fd(), PollFlags::IN)];
        assert_eq!(wait(&mut descriptors, 0).unwrap(), 1);
        assert!(descriptors[0].revents().contains(PollFlags::IN));
    }

    #[test]
    fn closed_fbdev_inputs_form_an_empty_snapshot() {
        controls::close();
        input::close_touch();
        assert_eq!(dispatch(false, 0).unwrap(), InputDispatch::default());
    }
}
