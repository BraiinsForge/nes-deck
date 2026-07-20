//! Narrow Rust ownership wrapper for Game Music Emu's stable C interface.

use std::collections::TryReserveError;
use std::error::Error;
use std::ffi::{CStr, c_char, c_int, c_long, c_void};
use std::fmt;
use std::path::Path;
use std::ptr::NonNull;

use retro_deck_platform::file::{BoundedReadError, read_regular_bounded};

const SAMPLE_RATE: c_int = 44_100;
const FRAMES_PER_BLOCK: usize = 735;
const SAMPLES_PER_BLOCK: c_int = 1_470;
const MAXIMUM_FILE_BYTES: usize = 16 * 1_024 * 1_024;

/// One Game Music Emu block borrowed until the next decoder operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GmeBlock<'frames> {
    frames: &'frames [[i16; 2]],
    ended: bool,
}

impl<'frames> GmeBlock<'frames> {
    /// Exactly 735 stereo frames generated for one 60 Hz application tick.
    #[must_use]
    pub const fn frames(self) -> &'frames [[i16; 2]] {
        self.frames
    }

    /// Whether Game Music Emu considers this track complete.
    #[must_use]
    pub const fn ended(self) -> bool {
        self.ended
    }
}

/// Owned Game Music Emu instance and copied current-track metadata.
pub struct GmeDecoder {
    emulator: NonNull<c_void>,
    track_index: usize,
    track_count: usize,
    title: String,
    game: String,
    author: String,
    system: String,
    length_milliseconds: Option<u64>,
    output: Vec<[i16; 2]>,
}

impl GmeDecoder {
    /// Open and validate one bounded regular GME-supported file without
    /// following a final symlink.
    ///
    /// # Errors
    ///
    /// Returns [`GmeDecoderError`] for file, core, track, or allocation
    /// failure.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, GmeDecoderError> {
        let bytes =
            read_regular_bounded(path, MAXIMUM_FILE_BYTES).map_err(GmeDecoderError::ReadFile)?;
        Self::from_bytes(&bytes)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, GmeDecoderError> {
        let size = c_long::try_from(bytes.len()).map_err(|_| GmeDecoderError::FileTooLarge)?;
        let mut candidate = std::ptr::null_mut();
        // SAFETY: `bytes` is readable for `size` bytes, `candidate` is a valid
        // output pointer, and GME copies the supplied data before returning.
        let error = unsafe {
            gme_open_data(
                bytes.as_ptr().cast::<c_void>(),
                size,
                &mut candidate,
                SAMPLE_RATE,
            )
        };
        if !error.is_null() {
            if let Some(emulator) = NonNull::new(candidate) {
                // SAFETY: GME returned this candidate through its ownership
                // output parameter, so it must be released on error.
                unsafe { gme_delete(emulator.as_ptr()) };
            }
            return Err(GmeDecoderError::Open(copy_error(error)));
        }
        let emulator = NonNull::new(candidate)
            .ok_or_else(|| GmeDecoderError::Open("GME returned no emulator".to_owned()))?;
        // SAFETY: `emulator` is live until this wrapper's Drop implementation.
        let reported_track_count = unsafe { gme_track_count(emulator.as_ptr()) };
        let Some(track_count) = usize::try_from(reported_track_count)
            .ok()
            .filter(|count| *count > 0)
        else {
            // SAFETY: validation failed before ownership moved into `Self`.
            unsafe { gme_delete(emulator.as_ptr()) };
            return Err(GmeDecoderError::InvalidTrackCount(reported_track_count));
        };
        let mut output = Vec::new();
        if let Err(source) = output.try_reserve_exact(FRAMES_PER_BLOCK) {
            // SAFETY: allocation failed before ownership moved into `Self`.
            unsafe { gme_delete(emulator.as_ptr()) };
            return Err(GmeDecoderError::AllocateOutput(source));
        }
        output.resize(FRAMES_PER_BLOCK, [0, 0]);
        let mut decoder = Self {
            emulator,
            track_index: 0,
            track_count,
            title: String::new(),
            game: String::new(),
            author: String::new(),
            system: String::new(),
            length_milliseconds: None,
            output,
        };
        decoder.start_track(0)?;
        Ok(decoder)
    }

    /// Decoder-reported song name, possibly empty.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Decoder-reported game name, possibly empty.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn game(&self) -> &str {
        &self.game
    }

    /// Decoder-reported author, possibly empty.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Decoder-reported emulated system, possibly empty.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn system(&self) -> &str {
        &self.system
    }

    /// Current zero-based subsong index.
    #[must_use]
    pub const fn track_index(&self) -> usize {
        self.track_index
    }

    /// Total subsongs exposed by the current file.
    #[must_use]
    pub const fn track_count(&self) -> usize {
        self.track_count
    }

    /// Current core playback position in milliseconds.
    #[must_use]
    pub fn position_milliseconds(&self) -> u64 {
        // SAFETY: `self.emulator` is owned and live for this call.
        let position = unsafe { gme_tell(self.emulator.as_ptr()) };
        u64::try_from(position).unwrap_or_default()
    }

    /// Current metadata-derived playback duration.
    #[must_use]
    pub const fn length_milliseconds(&self) -> Option<u64> {
        self.length_milliseconds
    }

    /// Decode exactly one 60 Hz stereo block.
    ///
    /// # Errors
    ///
    /// Returns [`GmeDecoderError`] when the core rejects generation.
    pub fn decode_block(&mut self) -> Result<GmeBlock<'_>, GmeDecoderError> {
        // SAFETY: the array-backed vector is writable for exactly 1,470
        // contiguous i16 samples, and the owned emulator is live.
        let error = unsafe {
            gme_play(
                self.emulator.as_ptr(),
                SAMPLES_PER_BLOCK,
                self.output.as_mut_ptr().cast::<i16>(),
            )
        };
        if !error.is_null() {
            return Err(GmeDecoderError::Play(copy_error(error)));
        }
        // SAFETY: querying completion borrows the same live emulator.
        let ended = unsafe { gme_track_ended(self.emulator.as_ptr()) } != 0;
        Ok(GmeBlock {
            frames: &self.output,
            ended,
        })
    }

    /// Select the previous or next subsong with wraparound.
    ///
    /// # Errors
    ///
    /// Returns [`GmeDecoderError`] if GME cannot start or describe the target.
    pub fn change_track(&mut self, direction: i8) -> Result<(), GmeDecoderError> {
        let target = if direction < 0 {
            self.track_index
                .checked_sub(1)
                .unwrap_or(self.track_count.saturating_sub(1))
        } else {
            self.track_index.saturating_add(1) % self.track_count
        };
        self.start_track(target)
    }

    /// Select one exact zero-based subsong.
    ///
    /// # Errors
    ///
    /// Returns [`GmeDecoderError`] if the index is out of range or GME cannot
    /// start or describe it.
    pub fn select_track(&mut self, track: usize) -> Result<(), GmeDecoderError> {
        self.start_track(track)
    }

    /// Restart the current subsong from its beginning.
    ///
    /// # Errors
    ///
    /// Returns [`GmeDecoderError`] if GME cannot restart or describe it.
    pub fn restart(&mut self) -> Result<(), GmeDecoderError> {
        self.start_track(self.track_index)
    }

    fn start_track(&mut self, track: usize) -> Result<(), GmeDecoderError> {
        if track >= self.track_count {
            return Err(GmeDecoderError::InvalidTrack(track));
        }
        let track_number =
            c_int::try_from(track).map_err(|_| GmeDecoderError::InvalidTrack(track))?;
        // SAFETY: the owned emulator is live and `track_number` is within the
        // count reported by that same instance.
        let error = unsafe { gme_start_track(self.emulator.as_ptr(), track_number) };
        if !error.is_null() {
            return Err(GmeDecoderError::StartTrack(copy_error(error)));
        }

        let mut info = std::ptr::null_mut();
        // SAFETY: the emulator is live and `info` is a valid output pointer.
        let error = unsafe { gme_track_info(self.emulator.as_ptr(), &mut info, track_number) };
        if !error.is_null() {
            return Err(GmeDecoderError::TrackInfo(copy_error(error)));
        }
        let info = NonNull::new(info).ok_or(GmeDecoderError::MissingTrackInfo)?;
        let info = InfoGuard(info);
        // SAFETY: `InfoGuard` owns a live GME info allocation for this scope.
        let values = unsafe { info.0.as_ref() };
        self.title = copy_string(values.song);
        self.game = copy_string(values.game);
        self.author = copy_string(values.author);
        self.system = copy_string(values.system);
        self.length_milliseconds = u64::try_from(values.play_length).ok();
        self.track_index = track;
        self.output.fill([0, 0]);
        Ok(())
    }
}

impl fmt::Debug for GmeDecoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GmeDecoder")
            .field("track_index", &self.track_index)
            .field("track_count", &self.track_count)
            .field("title", &self.title)
            .field("game", &self.game)
            .field("author", &self.author)
            .field("system", &self.system)
            .field("length_milliseconds", &self.length_milliseconds)
            .finish_non_exhaustive()
    }
}

impl Drop for GmeDecoder {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the emulator until Drop.
        unsafe { gme_delete(self.emulator.as_ptr()) };
    }
}

/// Game Music Emu file, allocation, core, or track failure.
#[derive(Debug)]
pub enum GmeDecoderError {
    /// The trusted file boundary rejected the input path or payload.
    ReadFile(BoundedReadError),
    /// The payload length cannot cross the C ABI.
    FileTooLarge,
    /// GME rejected the payload.
    Open(String),
    /// GME exposed no playable subsongs.
    InvalidTrackCount(c_int),
    /// A requested subsong is outside the reported count.
    InvalidTrack(usize),
    /// Starting a valid subsong failed.
    StartTrack(String),
    /// GME returned an error while retrieving metadata.
    TrackInfo(String),
    /// GME returned no metadata allocation for a valid subsong.
    MissingTrackInfo,
    /// Generating the next PCM block failed.
    Play(String),
    /// The fixed output block could not be reserved.
    AllocateOutput(TryReserveError),
}

impl fmt::Display for GmeDecoderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadFile(source) => write!(formatter, "cannot read GME file: {source}"),
            Self::FileTooLarge => {
                formatter.write_str("GME file length does not fit the native ABI")
            }
            Self::Open(message) => write!(formatter, "cannot open GME file: {message}"),
            Self::InvalidTrackCount(count) => write!(formatter, "GME reported {count} tracks"),
            Self::InvalidTrack(track) => write!(formatter, "GME track {track} is out of range"),
            Self::StartTrack(message) => write!(formatter, "cannot start GME track: {message}"),
            Self::TrackInfo(message) => write!(formatter, "cannot read GME metadata: {message}"),
            Self::MissingTrackInfo => formatter.write_str("GME returned no track metadata"),
            Self::Play(message) => write!(formatter, "cannot generate GME audio: {message}"),
            Self::AllocateOutput(source) => {
                write!(formatter, "cannot allocate GME output: {source}")
            }
        }
    }
}

impl Error for GmeDecoderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadFile(source) => Some(source),
            Self::AllocateOutput(source) => Some(source),
            Self::FileTooLarge
            | Self::Open(_)
            | Self::InvalidTrackCount(_)
            | Self::InvalidTrack(_)
            | Self::StartTrack(_)
            | Self::TrackInfo(_)
            | Self::MissingTrackInfo
            | Self::Play(_) => None,
        }
    }
}

#[repr(C)]
struct GmeInfo {
    _length: c_int,
    _intro_length: c_int,
    _loop_length: c_int,
    play_length: c_int,
    _reserved_integers: [c_int; 12],
    system: *const c_char,
    game: *const c_char,
    song: *const c_char,
    author: *const c_char,
    _copyright: *const c_char,
    _comment: *const c_char,
    _dumper: *const c_char,
    _reserved_strings: [*const c_char; 9],
}

struct InfoGuard(NonNull<GmeInfo>);

impl Drop for InfoGuard {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the allocation returned by
        // `gme_track_info` until Drop.
        unsafe { gme_free_info(self.0.as_ptr()) };
    }
}

fn copy_error(error: *const c_char) -> String {
    if error.is_null() {
        return "unknown Game Music Emu error".to_owned();
    }
    // SAFETY: GME errors are documented as pointers to NUL-terminated static
    // strings and remain valid across this immediate copy.
    unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned()
}

fn copy_string(value: *const c_char) -> String {
    if value.is_null() {
        return String::new();
    }
    // SAFETY: fields in a live `GmeInfo` are documented NUL-terminated strings
    // and are copied before the enclosing allocation is freed.
    unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}

unsafe extern "C" {
    fn gme_open_data(
        data: *const c_void,
        size: c_long,
        output: *mut *mut c_void,
        sample_rate: c_int,
    ) -> *const c_char;
    fn gme_track_count(emulator: *const c_void) -> c_int;
    fn gme_start_track(emulator: *mut c_void, track: c_int) -> *const c_char;
    fn gme_play(emulator: *mut c_void, count: c_int, output: *mut i16) -> *const c_char;
    fn gme_track_ended(emulator: *const c_void) -> c_int;
    fn gme_tell(emulator: *const c_void) -> c_int;
    fn gme_track_info(
        emulator: *const c_void,
        output: *mut *mut GmeInfo,
        track: c_int,
    ) -> *const c_char;
    fn gme_free_info(info: *mut GmeInfo);
    fn gme_delete(emulator: *mut c_void);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verbatim Game Music Emu 0.6.3 `test.nsf`, SHA-256
    // a8015d01d67f78c4fd6f5dd02a5dd04239e5aeeb2f90f8717050b4ed85effbe5.
    // It is distributed with the LGPL-2.1-or-later upstream test suite.
    const TEST_NSF_HEX: &str = "4e45534d1a010101008000800c80546574726973202847422900000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004e696e74656e646f0000000000000000000000000000000000000000000000001a4100000000000000000000000082000000a9038d1540201380200d8160205280204c8160a900850ca60ce60cbd0781f0f3850fa60fe60fbd5780f02210094980f0e6850d4c22801869d3aaa9478d0040bd3a828d0240bd078209088d0340a50d850e60c60ef0cc60ff8c45494e49444945494249414945494e494449454942494149454945424a4744474440808c4945424947454442413d41444947808c4c47494c49454044450000000000808c4c4a4947454445474945474a494a4c00440045474900470049004e004b004e50510050004e4c4b494b484900000000004c0000864e4f8c4e004c4a4c494a474e00008650518c50004e4d4e4b4d49504d494d5053564d554d534d510053515051864e508c4e0000000080012501354500a9008508a608e608bd0182f0f3850ba60be60bbd5181f02210094980f0e685094c1c811869d3aaa9478d0440bd3a828d0640bd078209088d0740a509850a60c60af0cc60ff8c360042004100360039003d0042003e003d00420039003d00360042003b0034004000808c390032003e003b003d3d3d3b3938808c380039003d003d00390034002d00808c3d4045403b403d4039403840394045403b403d4039403840393d3936423f3c3f383f3c3f3d00440038003d4044403b403a3d36863d008c3a3d3b00000000003c3f38863f008c3c3f3d00000000003d003800350031003300350039003b003d008636008c000000000080012501354500030303030302020202020202010101010101010101010101000000000000000000000000000000000000000000000000000000f7be885626f8cea57f5b3919fbdec3aa927b66523f2d1c0cfdeee1d4c8bdb2a89f968d857e767069635e58534f4a46423e3a37";

    fn test_nsf() -> Vec<u8> {
        TEST_NSF_HEX
            .as_bytes()
            .chunks_exact(2)
            .filter_map(|pair| {
                let [high, low] = pair else {
                    return None;
                };
                Some((nibble(*high)? << 4) | nibble(*low)?)
            })
            .collect()
    }

    const fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }

    #[test]
    fn upstream_nsf_decodes_and_restarts_deterministically() {
        let bytes = test_nsf();
        assert_eq!(bytes.len(), 749);
        let mut decoder = GmeDecoder::from_bytes(&bytes).expect("upstream test NSF opens");
        assert_eq!(decoder.track_count(), 1);
        assert_eq!(decoder.game(), "Tetris (GB)");
        assert_eq!(decoder.system(), "Nintendo NES");
        let first = decoder
            .decode_block()
            .expect("upstream test NSF decodes")
            .frames()
            .to_vec();
        let mut peak = 0_i32;
        for _ in 0..60 {
            let block = decoder.decode_block().expect("upstream test NSF decodes");
            for frame in block.frames() {
                peak = peak.max(i32::from(frame[0]).abs());
                peak = peak.max(i32::from(frame[1]).abs());
            }
        }
        assert!(peak > 0);
        decoder.restart().expect("upstream test NSF restarts");
        let restarted = decoder.decode_block().expect("restarted test NSF decodes");
        assert_eq!(restarted.frames(), first);
        assert_eq!(decoder.position_milliseconds(), 16);
    }

    #[test]
    fn invalid_payload_is_rejected_without_an_emulator() {
        assert!(matches!(
            GmeDecoder::from_bytes(b"not game music"),
            Err(GmeDecoderError::Open(_))
        ));
    }
}
