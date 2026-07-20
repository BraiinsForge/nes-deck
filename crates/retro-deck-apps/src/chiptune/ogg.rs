//! Pure-Rust Ogg Vorbis decoder with fixed-size stereo output blocks.

use std::collections::TryReserveError;
use std::error::Error;
use std::fmt;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use lewton::VorbisError;
use lewton::inside_ogg::OggStreamReader;
use retro_deck_platform::file::{BoundedReadError, read_regular_bounded};

const SAMPLE_RATE: u32 = 44_100;
const FRAMES_PER_BLOCK: usize = 735;
const MAXIMUM_FILE_BYTES: usize = 16 * 1_024 * 1_024;
const OGG_CAPTURE: &[u8; 4] = b"OggS";

type Reader = OggStreamReader<Cursor<Arc<[u8]>>>;

/// One decoder block borrowed until the next mutable decoder operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OggBlock<'frames> {
    frames: &'frames [[i16; 2]],
    ended: bool,
}

impl<'frames> OggBlock<'frames> {
    /// Decoded stereo frames, possibly shorter than one 60 Hz block at EOF.
    #[must_use]
    pub const fn frames(self) -> &'frames [[i16; 2]] {
        self.frames
    }

    /// Whether the decoder encountered the physical end of the stream.
    #[must_use]
    pub const fn ended(self) -> bool {
        self.ended
    }
}

/// Stateful 44.1 kHz mono-or-stereo Ogg Vorbis decoder.
pub struct OggDecoder {
    bytes: Arc<[u8]>,
    reader: Reader,
    channels: usize,
    stream_serial: u32,
    title: String,
    artist: String,
    length_milliseconds: Option<u64>,
    position_frames: u64,
    packet: Vec<i16>,
    packet_offset: usize,
    output: Vec<[i16; 2]>,
}

impl fmt::Debug for OggDecoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OggDecoder")
            .field("bytes", &self.bytes.len())
            .field("channels", &self.channels)
            .field("stream_serial", &self.stream_serial)
            .field("title", &self.title)
            .field("artist", &self.artist)
            .field("length_milliseconds", &self.length_milliseconds)
            .field("position_frames", &self.position_frames)
            .field("packet_samples", &self.packet.len())
            .field("packet_offset", &self.packet_offset)
            .finish_non_exhaustive()
    }
}

impl OggDecoder {
    /// Open and validate one bounded regular `.ogg` file without following a
    /// final symlink.
    ///
    /// # Errors
    ///
    /// Returns [`OggDecoderError`] for file-boundary, Vorbis-header, channel,
    /// sample-rate, or fixed-output allocation failure.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, OggDecoderError> {
        let bytes =
            read_regular_bounded(path, MAXIMUM_FILE_BYTES).map_err(OggDecoderError::ReadFile)?;
        Self::from_bytes(bytes)
    }

    fn from_bytes(bytes: Vec<u8>) -> Result<Self, OggDecoderError> {
        let bytes: Arc<[u8]> = bytes.into();
        let reader = make_reader(Arc::clone(&bytes))?;
        let channels = usize::from(reader.ident_hdr.audio_channels);
        if !(1..=2).contains(&channels) {
            return Err(OggDecoderError::UnsupportedChannels(channels));
        }
        if reader.ident_hdr.audio_sample_rate != SAMPLE_RATE {
            return Err(OggDecoderError::UnsupportedSampleRate(
                reader.ident_hdr.audio_sample_rate,
            ));
        }
        let stream_serial = reader.stream_serial();
        let length_milliseconds = last_granule(&bytes, stream_serial)
            .and_then(|frames| frames.checked_mul(1_000))
            .map(|scaled| scaled / u64::from(SAMPLE_RATE));
        let title = comment(&reader, "TITLE").unwrap_or_default();
        let artist = comment(&reader, "ARTIST").unwrap_or_default();
        let mut output = Vec::new();
        output
            .try_reserve_exact(FRAMES_PER_BLOCK)
            .map_err(OggDecoderError::AllocateOutput)?;
        Ok(Self {
            bytes,
            reader,
            channels,
            stream_serial,
            title,
            artist,
            length_milliseconds,
            position_frames: 0,
            packet: Vec::new(),
            packet_offset: 0,
            output,
        })
    }

    /// Vorbis `TITLE` comment, or an empty string.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Vorbis `ARTIST` comment, or an empty string.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn artist(&self) -> &str {
        &self.artist
    }

    /// Current decoded position in milliseconds.
    #[must_use]
    pub const fn position_milliseconds(&self) -> u64 {
        self.position_frames.saturating_mul(1_000) / 44_100
    }

    /// Stream duration derived from the final valid granule position.
    #[must_use]
    pub const fn length_milliseconds(&self) -> Option<u64> {
        self.length_milliseconds
    }

    /// Decode at most 735 stereo frames, exactly one 60 Hz application tick.
    ///
    /// # Errors
    ///
    /// Returns [`OggDecoderError`] when a packet is damaged, changes stream
    /// format, or is not aligned to complete input frames.
    pub fn decode_block(&mut self) -> Result<OggBlock<'_>, OggDecoderError> {
        self.output.clear();
        let mut ended = false;
        while self.output.len() < FRAMES_PER_BLOCK {
            if self.packet_offset < self.packet.len() {
                let frame = if self.channels == 1 {
                    let Some(&sample) = self.packet.get(self.packet_offset) else {
                        return Err(OggDecoderError::MisalignedPacket);
                    };
                    self.packet_offset = self.packet_offset.saturating_add(1);
                    [sample, sample]
                } else {
                    let Some(samples) = self
                        .packet
                        .get(self.packet_offset..self.packet_offset.saturating_add(2))
                    else {
                        return Err(OggDecoderError::MisalignedPacket);
                    };
                    let [left, right] = samples else {
                        return Err(OggDecoderError::MisalignedPacket);
                    };
                    self.packet_offset = self.packet_offset.saturating_add(2);
                    [*left, *right]
                };
                self.output.push(frame);
                continue;
            }

            if let Some(packet) = self
                .reader
                .read_dec_packet_itl()
                .map_err(OggDecoderError::Decode)?
            {
                self.validate_stream()?;
                if packet.len() % self.channels != 0 {
                    return Err(OggDecoderError::MisalignedPacket);
                }
                self.packet = packet;
                self.packet_offset = 0;
            } else {
                ended = true;
                break;
            }
        }
        self.position_frames = self
            .position_frames
            .saturating_add(u64::try_from(self.output.len()).unwrap_or(u64::MAX));
        Ok(OggBlock {
            frames: &self.output,
            ended,
        })
    }

    /// Seek back to the first audio packet and clear buffered output.
    ///
    /// # Errors
    ///
    /// Returns [`OggDecoderError`] if the retained validated bytes no longer
    /// form a supported stream. No filesystem access occurs.
    pub fn restart(&mut self) -> Result<(), OggDecoderError> {
        self.reader = make_reader(Arc::clone(&self.bytes))?;
        self.validate_stream()?;
        if self.reader.stream_serial() != self.stream_serial {
            return Err(OggDecoderError::StreamChanged);
        }
        self.position_frames = 0;
        self.packet.clear();
        self.packet_offset = 0;
        self.output.clear();
        Ok(())
    }

    fn validate_stream(&self) -> Result<(), OggDecoderError> {
        let channels = usize::from(self.reader.ident_hdr.audio_channels);
        if channels != self.channels {
            return Err(OggDecoderError::UnsupportedChannels(channels));
        }
        if self.reader.ident_hdr.audio_sample_rate != SAMPLE_RATE {
            return Err(OggDecoderError::UnsupportedSampleRate(
                self.reader.ident_hdr.audio_sample_rate,
            ));
        }
        Ok(())
    }
}

/// Ogg file, format, decode, or bounded-output failure.
#[derive(Debug)]
pub enum OggDecoderError {
    /// The trusted file boundary rejected the input path or payload.
    ReadFile(BoundedReadError),
    /// Lewton rejected a header or audio packet.
    Decode(VorbisError),
    /// Only mono and stereo sources are supported.
    UnsupportedChannels(usize),
    /// Native playback accepts 44.1 kHz sources.
    UnsupportedSampleRate(u32),
    /// An interleaved packet ended between complete frames.
    MisalignedPacket,
    /// Reconstructing a retained stream produced another serial number.
    StreamChanged,
    /// The fixed 735-frame output block could not be reserved.
    AllocateOutput(TryReserveError),
}

impl fmt::Display for OggDecoderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadFile(source) => write!(formatter, "cannot read Ogg file: {source}"),
            Self::Decode(source) => write!(formatter, "cannot decode Ogg Vorbis: {source}"),
            Self::UnsupportedChannels(channels) => {
                write!(
                    formatter,
                    "Ogg Vorbis has {channels} channels; expected one or two"
                )
            }
            Self::UnsupportedSampleRate(rate) => {
                write!(
                    formatter,
                    "Ogg Vorbis rate is {rate} Hz; expected {SAMPLE_RATE} Hz"
                )
            }
            Self::MisalignedPacket => formatter.write_str("Ogg Vorbis packet is not frame-aligned"),
            Self::StreamChanged => formatter.write_str("Ogg Vorbis stream identity changed"),
            Self::AllocateOutput(source) => {
                write!(formatter, "cannot allocate Ogg output block: {source}")
            }
        }
    }
}

impl Error for OggDecoderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadFile(source) => Some(source),
            Self::Decode(source) => Some(source),
            Self::AllocateOutput(source) => Some(source),
            Self::UnsupportedChannels(_)
            | Self::UnsupportedSampleRate(_)
            | Self::MisalignedPacket
            | Self::StreamChanged => None,
        }
    }
}

fn make_reader(bytes: Arc<[u8]>) -> Result<Reader, OggDecoderError> {
    OggStreamReader::new(Cursor::new(bytes)).map_err(OggDecoderError::Decode)
}

fn comment(reader: &Reader, expected: &str) -> Option<String> {
    reader
        .comment_hdr
        .comment_list
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(expected))
        .map(|(_, value)| value.clone())
}

fn last_granule(bytes: &[u8], stream_serial: u32) -> Option<u64> {
    let mut offset = 0_usize;
    let mut last = None;
    while offset.saturating_add(27) <= bytes.len() {
        if bytes.get(offset..offset + 4) != Some(OGG_CAPTURE.as_slice()) {
            offset = offset.saturating_add(1);
            continue;
        }
        let version = *bytes.get(offset + 4)?;
        let segment_count = usize::from(*bytes.get(offset + 26)?);
        let header_end = offset.checked_add(27)?.checked_add(segment_count)?;
        let segments = bytes.get(offset + 27..header_end)?;
        let body_bytes = segments.iter().fold(0_usize, |total, segment| {
            total.saturating_add(usize::from(*segment))
        });
        let page_end = header_end.checked_add(body_bytes)?;
        if version != 0 || page_end > bytes.len() {
            offset = offset.saturating_add(1);
            continue;
        }
        let serial = read_u32(bytes.get(offset + 14..offset + 18)?)?;
        let granule = read_u64(bytes.get(offset + 6..offset + 14)?)?;
        if serial == stream_serial && granule != u64::MAX {
            last = Some(granule);
        }
        offset = page_end;
    }
    last
}

fn read_u32(bytes: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn read_u64(bytes: &[u8]) -> Option<u64> {
    let bytes: [u8; 8] = bytes.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_page(serial: u32, granule: u64, body: &[u8]) -> Vec<u8> {
        let segment_length = u8::try_from(body.len()).expect("test Ogg page body fits one segment");
        let mut page = Vec::from(*b"OggS");
        page.push(0);
        page.push(0);
        page.extend_from_slice(&granule.to_le_bytes());
        page.extend_from_slice(&serial.to_le_bytes());
        page.extend_from_slice(&0_u32.to_le_bytes());
        page.extend_from_slice(&0_u32.to_le_bytes());
        page.push(1);
        page.push(segment_length);
        page.extend_from_slice(body);
        page
    }

    #[test]
    fn granule_scan_uses_the_last_complete_matching_page() {
        let mut bytes = fixture_page(7, 100, b"one");
        bytes.extend_from_slice(b"noise");
        bytes.extend_from_slice(&fixture_page(8, 999, b"other"));
        bytes.extend_from_slice(&fixture_page(7, 250, b"two"));
        bytes.extend_from_slice(b"OggS\0");
        assert_eq!(last_granule(&bytes, 7), Some(250));
        assert_eq!(last_granule(&bytes, 8), Some(999));
        assert_eq!(last_granule(&bytes, 9), None);
    }

    #[test]
    fn tracked_ogg_decodes_and_restarts_deterministically() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../chiptunes/crazy.ogg");
        let mut decoder = OggDecoder::open(path).expect("tracked Ogg opens");
        let mut first = Vec::new();
        let mut peak = 0_i32;
        for block_index in 0..60 {
            let block = decoder.decode_block().expect("tracked Ogg block decodes");
            if block_index == 0 {
                first.extend_from_slice(block.frames());
            }
            for frame in block.frames() {
                peak = peak.max(i32::from(frame[0]).abs());
                peak = peak.max(i32::from(frame[1]).abs());
            }
            assert!(!block.ended());
        }
        assert!(peak > 0);
        assert!(decoder.length_milliseconds().is_some());
        decoder.restart().expect("tracked Ogg restarts");
        let restarted = decoder.decode_block().expect("restarted Ogg block decodes");
        assert_eq!(restarted.frames(), first);
        assert_eq!(decoder.position_milliseconds(), 16);
    }
}
