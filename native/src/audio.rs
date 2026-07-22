use std::ffi::{c_char, c_int, c_ulong, c_void};
use std::io;
use std::mem;
use std::slice;
use std::sync::{Mutex, MutexGuard};

const AUDIO_DEVICE: &[u8] = b"/dev/dsp\0";
const O_WRONLY: c_int = 1;
const O_CLOEXEC: c_int = 0o2000000;
const SIG_HUP: c_int = 1;
const SIG_INT: c_int = 2;
const SIG_TERM: c_int = 15;
const SIG_DFL: usize = 0;
const WNOHANG: c_int = 1;
const ECHILD: c_int = 10;

const AFMT_S16_LE: c_int = 0x10;
const SNDCTL_DSP_SYNC: c_ulong = 0x5001;
const SNDCTL_DSP_SPEED: c_ulong = 0xc0045002;
const SNDCTL_DSP_SETFMT: c_ulong = 0xc0045005;
const SNDCTL_DSP_CHANNELS: c_ulong = 0xc0045006;
const SNDCTL_DSP_SETFRAGMENT: c_ulong = 0xc004500a;

#[derive(Clone, Copy)]
struct Tone {
    frequency: c_int,
    duration_ms: c_int,
}

pub enum PlayOutcome {
    Started,
    Busy,
}

struct Player {
    child_pid: c_int,
}

static PLAYER: Mutex<Player> = Mutex::new(Player { child_pid: -1 });

unsafe extern "C" {
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn write(fd: c_int, data: *const c_void, size: usize) -> isize;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn fork() -> c_int;
    fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    fn kill(pid: c_int, signal: c_int) -> c_int;
    fn signal(signal: c_int, handler: usize) -> usize;
    fn _exit(status: c_int) -> !;
}

pub fn play_tones(
    first_frequency: c_int,
    first_duration_ms: c_int,
    second_frequency: c_int,
    second_duration_ms: c_int,
    volume_percent: c_int,
) -> Result<PlayOutcome, String> {
    let (tones, count) = tone_sequence(
        first_frequency,
        first_duration_ms,
        second_frequency,
        second_duration_ms,
    )?;
    validate(&tones[..count], 44100, volume_percent)?;

    let mut player = player();
    player.reap_finished();
    if player.child_pid > 0 {
        return Ok(PlayOutcome::Busy);
    }

    let child = unsafe { fork() };
    if child < 0 {
        return Err(os_error("cannot start menu sound worker"));
    }
    if child == 0 {
        unsafe {
            signal(SIG_TERM, SIG_DFL);
            signal(SIG_INT, SIG_DFL);
            signal(SIG_HUP, SIG_DFL);
        }
        let result = play_blocking(&tones[..count], volume_percent);
        if let Err(error) = result.as_ref() {
            eprintln!("retrodeck: {error}");
        }
        unsafe { _exit(if result.is_ok() { 0 } else { 1 }) };
    }

    player.child_pid = child;
    Ok(PlayOutcome::Started)
}

pub fn active() -> bool {
    let mut player = player();
    player.reap_finished();
    player.child_pid > 0
}

pub fn stop() {
    player().stop();
}

pub fn finish() {
    player().finish();
}

impl Player {
    fn reap_finished(&mut self) {
        if self.child_pid <= 0 {
            return;
        }
        loop {
            let mut status = 0;
            let result = unsafe { waitpid(self.child_pid, &mut status, WNOHANG) };
            if result == 0 {
                return;
            }
            if result == self.child_pid {
                if !status_succeeded(status) {
                    eprintln!("retrodeck: menu sound worker failed");
                }
                self.child_pid = -1;
                return;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.raw_os_error() != Some(ECHILD) {
                eprintln!("retrodeck: cannot reap menu sound worker: {error}");
            }
            self.child_pid = -1;
            return;
        }
    }

    fn stop(&mut self) {
        if self.child_pid > 0 {
            unsafe { kill(self.child_pid, SIG_TERM) };
            self.wait(false);
        }
    }

    fn finish(&mut self) {
        if self.child_pid > 0 {
            self.wait(true);
        }
    }

    fn wait(&mut self, report_failure: bool) {
        loop {
            let mut status = 0;
            let result = unsafe { waitpid(self.child_pid, &mut status, 0) };
            if result == self.child_pid {
                if report_failure && !status_succeeded(status) {
                    eprintln!("retrodeck: menu sound worker failed");
                }
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.raw_os_error() != Some(ECHILD) {
                eprintln!("retrodeck: cannot finish menu sound worker: {error}");
            }
            break;
        }
        self.child_pid = -1;
    }
}

fn player() -> MutexGuard<'static, Player> {
    PLAYER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn tone_sequence(
    first_frequency: c_int,
    first_duration_ms: c_int,
    second_frequency: c_int,
    second_duration_ms: c_int,
) -> Result<([Tone; 2], usize), String> {
    let tones = [
        Tone {
            frequency: first_frequency,
            duration_ms: first_duration_ms,
        },
        Tone {
            frequency: second_frequency,
            duration_ms: second_duration_ms,
        },
    ];
    let count = if second_frequency == 0 && second_duration_ms == 0 {
        1
    } else if second_frequency > 0 && second_duration_ms > 0 {
        2
    } else {
        return Err("the optional second tone needs frequency and duration".to_owned());
    };
    Ok((tones, count))
}

fn validate(tones: &[Tone], rate: c_int, volume_percent: c_int) -> Result<(), String> {
    if tones.is_empty() {
        return Err("at least one menu tone is required".to_owned());
    }
    if !(1..=100).contains(&volume_percent) {
        return Err("menu sound volume must be between 1 and 100".to_owned());
    }
    if rate <= 0 {
        return Err("menu sound sample rate must be positive".to_owned());
    }
    if tones
        .iter()
        .any(|tone| tone.frequency <= 0 || tone.duration_ms <= 0)
    {
        return Err("menu tones need positive frequency and duration".to_owned());
    }
    Ok(())
}

fn render_tones(tones: &[Tone], rate: c_int, volume_percent: c_int) -> Result<Vec<i16>, String> {
    validate(tones, rate, volume_percent)?;
    let rate = rate as usize;
    let amplitude = (5000 * volume_percent / 100).max(256);
    let ramp_samples = (rate / 200).max(1);
    let mut samples = Vec::new();

    for tone in tones {
        let note_samples = rate
            .checked_mul(tone.duration_ms as usize)
            .ok_or_else(|| "menu tone duration is too large".to_owned())?
            / 1000;
        let note_samples = note_samples.max(1);
        samples
            .try_reserve(note_samples)
            .map_err(|_| "cannot allocate menu tone samples".to_owned())?;
        let period = ((rate as c_int) / tone.frequency).max(2) as usize;
        for index in 0..note_samples {
            let mut sample = if index % period < period / 2 {
                amplitude
            } else {
                -amplitude
            };
            let remaining = note_samples - index;
            let envelope = ramp_samples.min((index + 1).min(remaining));
            sample = (sample as i64 * envelope as i64 / ramp_samples as i64) as c_int;
            samples.push(sample as i16);
        }
    }
    Ok(samples)
}

fn play_blocking(tones: &[Tone], volume_percent: c_int) -> Result<(), String> {
    let fd = unsafe { open(AUDIO_DEVICE.as_ptr().cast(), O_WRONLY | O_CLOEXEC) };
    if fd < 0 {
        return Err(os_error("cannot open /dev/dsp for menu sound"));
    }

    let result = (|| {
        let mut fragment: c_int = (4 << 16) | 9;
        let mut format = AFMT_S16_LE;
        let mut channels: c_int = 1;
        let mut rate: c_int = 44100;
        unsafe {
            ioctl(fd, SNDCTL_DSP_SETFRAGMENT, &mut fragment as *mut c_int);
        }
        let configured = unsafe {
            ioctl(fd, SNDCTL_DSP_SETFMT, &mut format as *mut c_int) == 0
                && format == AFMT_S16_LE
                && ioctl(fd, SNDCTL_DSP_CHANNELS, &mut channels as *mut c_int) == 0
                && channels == 1
                && ioctl(fd, SNDCTL_DSP_SPEED, &mut rate as *mut c_int) == 0
                && rate > 0
        };
        if !configured {
            return Err(os_error("cannot configure menu sound"));
        }

        let samples = render_tones(tones, rate, volume_percent)?;
        let bytes = unsafe {
            slice::from_raw_parts(
                samples.as_ptr().cast::<u8>(),
                samples.len() * mem::size_of::<i16>(),
            )
        };
        write_all(fd, bytes).map_err(|error| format!("cannot play menu sound: {error}"))?;
        unsafe { ioctl(fd, SNDCTL_DSP_SYNC, 0usize) };
        Ok(())
    })();

    let close_result = unsafe { close(fd) };
    if result.is_ok() && close_result != 0 {
        return Err(os_error("cannot play menu sound"));
    }
    result
}

fn write_all(fd: c_int, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let written = unsafe { write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written > 0 {
            bytes = &bytes[written as usize..];
        } else if written == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        } else {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    Ok(())
}

fn os_error(action: &str) -> String {
    format!("{action}: {}", io::Error::last_os_error())
}

fn status_succeeded(status: c_int) -> bool {
    status & 0x7f == 0 && (status >> 8) & 0xff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(samples: &[i16]) -> u64 {
        samples
            .iter()
            .fold(14695981039346656037, |mut hash, sample| {
                for byte in sample.to_le_bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(1099511628211);
                }
                hash
            })
    }

    #[test]
    fn matches_the_legacy_menu_waveforms() {
        let cases = [
            (&[(660, 60), (880, 60)][..], 5292, 0xecf6ba7ff22d0dc2),
            (&[(523, 35)][..], 1543, 0xdfdd2a5aba3f4a04),
            (&[(659, 35)][..], 1543, 0xab6adca9dc7484b9),
            (&[(659, 25), (880, 30)][..], 2425, 0x633b4308002d1688),
            (&[(659, 25), (440, 30)][..], 2425, 0xfe15242926ff4036),
        ];
        for (notes, expected_length, expected_digest) in cases {
            let tones = notes
                .iter()
                .map(|&(frequency, duration_ms)| Tone {
                    frequency,
                    duration_ms,
                })
                .collect::<Vec<_>>();
            let samples = render_tones(&tones, 44100, 42).unwrap();
            assert_eq!(samples.len(), expected_length);
            assert_eq!(digest(&samples), expected_digest);
        }
    }

    #[test]
    fn rejects_invalid_tone_requests() {
        assert!(tone_sequence(659, 25, 0, 30).is_err());
        assert!(
            render_tones(
                &[Tone {
                    frequency: 0,
                    duration_ms: 25
                }],
                44100,
                42
            )
            .is_err()
        );
        assert!(
            render_tones(
                &[Tone {
                    frequency: 659,
                    duration_ms: 25
                }],
                44100,
                0
            )
            .is_err()
        );
    }
}
