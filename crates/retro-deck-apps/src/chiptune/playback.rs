//! Playlist and decoder ownership independent from input, rendering, and audio.

use std::collections::TryReserveError;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

#[cfg(feature = "chiptune-gme")]
use super::GmeDecoder;
use super::{ChiptuneCatalog, OggDecoder, PlaybackMode};

const FRAMES_PER_BLOCK: usize = 735;

/// Outcome of one fixed decoder tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackTick {
    frames: usize,
    ended: bool,
}

impl PlaybackTick {
    /// Stereo frames copied into the player's fixed output block.
    #[must_use]
    pub const fn frames(self) -> usize {
        self.frames
    }

    /// Whether the decoder encountered the end of the current track.
    #[must_use]
    pub const fn ended(self) -> bool {
        self.ended
    }
}

/// One active decoder selected from a bounded immutable catalog.
#[derive(Debug)]
pub struct ChiptunePlayer {
    files: Box<[PathBuf]>,
    file_index: usize,
    decoder: Decoder,
    title: String,
    subtitle: String,
    system: String,
    random: RandomState,
    output: Vec<[i16; 2]>,
}

impl ChiptunePlayer {
    /// Open the first playable file, skipping malformed catalog entries.
    ///
    /// # Errors
    ///
    /// Returns [`ChiptunePlayerError`] if the catalog is empty, every decoder
    /// rejects its payload, or the fixed output block cannot be allocated.
    pub fn open(catalog: ChiptuneCatalog, random_seed: u32) -> Result<Self, ChiptunePlayerError> {
        Self::open_files(catalog.into_files(), random_seed)
    }

    /// Open exactly one caller-selected music file.
    ///
    /// This is intended for bounded diagnostics and preview generation. The
    /// normal device player should use [`Self::open`] with a discovered
    /// catalog.
    ///
    /// # Errors
    ///
    /// Returns [`ChiptunePlayerError`] when the decoder rejects the file or
    /// the fixed output block cannot be allocated.
    pub fn open_file(
        path: impl AsRef<Path>,
        random_seed: u32,
    ) -> Result<Self, ChiptunePlayerError> {
        Self::open_files(Box::new([path.as_ref().to_path_buf()]), random_seed)
    }

    fn open_files(files: Box<[PathBuf]>, random_seed: u32) -> Result<Self, ChiptunePlayerError> {
        if files.is_empty() {
            return Err(ChiptunePlayerError::NoFiles);
        }
        let mut selected = None;
        let mut last_error = String::new();
        for (index, path) in files.iter().enumerate() {
            match Decoder::open(path) {
                Ok(decoder) => {
                    selected = Some((index, decoder));
                    break;
                }
                Err(error) => last_error = format!("{}: {error}", path.display()),
            }
        }
        let Some((file_index, decoder)) = selected else {
            return Err(ChiptunePlayerError::NoPlayableFiles {
                attempted: files.len(),
                last_error,
            });
        };
        let mut output = Vec::new();
        output
            .try_reserve_exact(FRAMES_PER_BLOCK)
            .map_err(ChiptunePlayerError::AllocateOutput)?;
        let mut player = Self {
            files,
            file_index,
            decoder,
            title: String::new(),
            subtitle: String::new(),
            system: String::new(),
            random: RandomState::new(random_seed),
            output,
        };
        player.refresh_metadata();
        Ok(player)
    }

    /// Decode one 60 Hz block and copy it into fixed player-owned storage.
    ///
    /// # Errors
    ///
    /// Returns [`ChiptunePlayerError`] if the active decoder rejects a packet.
    pub fn decode_block(&mut self) -> Result<PlaybackTick, ChiptunePlayerError> {
        let ended = match &mut self.decoder {
            Decoder::Ogg(decoder) => {
                let block = decoder
                    .decode_block()
                    .map_err(ChiptuneDecoderError::Ogg)
                    .map_err(ChiptunePlayerError::Decoder)?;
                self.output.clear();
                self.output.extend_from_slice(block.frames());
                block.ended()
            }
            #[cfg(feature = "chiptune-gme")]
            Decoder::Gme(decoder) => {
                let block = decoder
                    .decode_block()
                    .map_err(ChiptuneDecoderError::Gme)
                    .map_err(ChiptunePlayerError::Decoder)?;
                self.output.clear();
                self.output.extend_from_slice(block.frames());
                block.ended()
            }
        };
        Ok(PlaybackTick {
            frames: self.output.len(),
            ended,
        })
    }

    /// Apply the configured behavior after a decoder reports completion.
    ///
    /// # Errors
    ///
    /// Returns [`ChiptunePlayerError`] if restarting or selecting another
    /// playable file or subsong fails.
    pub fn advance_after_end(&mut self, mode: PlaybackMode) -> Result<(), ChiptunePlayerError> {
        match mode {
            PlaybackMode::LoopOne => self.restart(),
            PlaybackMode::Shuffle => self.change_random(),
            PlaybackMode::LoopAll if self.track_index().saturating_add(1) < self.track_count() => {
                self.select_track(self.track_index().saturating_add(1))
            }
            PlaybackMode::LoopAll => self.change_file(1),
        }
    }

    /// Open the previous or next playable catalog file with wraparound.
    ///
    /// A rejected candidate never replaces the current working decoder.
    ///
    /// # Errors
    ///
    /// Returns [`ChiptunePlayerError`] if every candidate is rejected.
    pub fn change_file(&mut self, direction: i8) -> Result<(), ChiptunePlayerError> {
        let count = self.files.len();
        let current = self.file_index;
        let mut last_error = String::new();
        for distance in 1..=count {
            let candidate = if direction < 0 {
                current
                    .checked_sub(distance % count)
                    .unwrap_or_else(|| current + count - distance % count)
            } else {
                current.saturating_add(distance) % count
            };
            let Some(path) = self.files.get(candidate) else {
                continue;
            };
            match Decoder::open(path) {
                Ok(decoder) => {
                    self.decoder = decoder;
                    self.file_index = candidate;
                    self.output.clear();
                    self.refresh_metadata();
                    return Ok(());
                }
                Err(error) => last_error = format!("{}: {error}", path.display()),
            }
        }
        Err(ChiptunePlayerError::NoPlayableFiles {
            attempted: count,
            last_error,
        })
    }

    /// Select the previous or next subsong with wraparound.
    ///
    /// # Errors
    ///
    /// Returns [`ChiptunePlayerError`] for a single-track decoder or core
    /// failure.
    #[cfg_attr(
        not(feature = "chiptune-gme"),
        allow(
            unused_variables,
            reason = "direction is meaningful only when the GME decoder is compiled"
        )
    )]
    pub fn change_track(&mut self, direction: i8) -> Result<(), ChiptunePlayerError> {
        match &mut self.decoder {
            Decoder::Ogg(_) => Err(ChiptunePlayerError::SingleTrack),
            #[cfg(feature = "chiptune-gme")]
            Decoder::Gme(decoder) => decoder
                .change_track(direction)
                .map_err(ChiptuneDecoderError::Gme)
                .map_err(ChiptunePlayerError::Decoder),
        }?;
        self.output.clear();
        self.refresh_metadata();
        Ok(())
    }

    /// Ordered current file position.
    #[must_use]
    pub const fn file_index(&self) -> usize {
        self.file_index
    }

    /// Total files retained by the bounded catalog.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Current zero-based subsong index.
    #[must_use]
    pub const fn track_index(&self) -> usize {
        match &self.decoder {
            Decoder::Ogg(_) => 0,
            #[cfg(feature = "chiptune-gme")]
            Decoder::Gme(decoder) => decoder.track_index(),
        }
    }

    /// Total subsongs exposed by the active decoder.
    #[must_use]
    pub const fn track_count(&self) -> usize {
        match &self.decoder {
            Decoder::Ogg(_) => 1,
            #[cfg(feature = "chiptune-gme")]
            Decoder::Gme(decoder) => decoder.track_count(),
        }
    }

    /// Preferred current title with a filename fallback.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Current artist or game-and-author line.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn subtitle(&self) -> &str {
        &self.subtitle
    }

    /// Current decoder system or format label.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn system(&self) -> &str {
        &self.system
    }

    /// Current decoder position in milliseconds.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "the optional native GME position query is not const"
    )]
    pub fn position_milliseconds(&self) -> u64 {
        match &self.decoder {
            Decoder::Ogg(decoder) => decoder.position_milliseconds(),
            #[cfg(feature = "chiptune-gme")]
            Decoder::Gme(decoder) => decoder.position_milliseconds(),
        }
    }

    /// Known current track duration in milliseconds.
    #[must_use]
    pub const fn length_milliseconds(&self) -> Option<u64> {
        match &self.decoder {
            Decoder::Ogg(decoder) => decoder.length_milliseconds(),
            #[cfg(feature = "chiptune-gme")]
            Decoder::Gme(decoder) => decoder.length_milliseconds(),
        }
    }

    /// Most recently decoded stereo block for audio and visualization.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref Vec to a slice"
    )]
    pub fn waveform(&self) -> &[[i16; 2]] {
        &self.output
    }

    fn restart(&mut self) -> Result<(), ChiptunePlayerError> {
        match &mut self.decoder {
            Decoder::Ogg(decoder) => decoder.restart().map_err(ChiptuneDecoderError::Ogg),
            #[cfg(feature = "chiptune-gme")]
            Decoder::Gme(decoder) => decoder.restart().map_err(ChiptuneDecoderError::Gme),
        }
        .map_err(ChiptunePlayerError::Decoder)?;
        self.output.clear();
        self.refresh_metadata();
        Ok(())
    }

    #[cfg_attr(
        not(feature = "chiptune-gme"),
        allow(
            unused_variables,
            reason = "track is meaningful only when the GME decoder is compiled"
        )
    )]
    fn select_track(&mut self, track: usize) -> Result<(), ChiptunePlayerError> {
        match &mut self.decoder {
            Decoder::Ogg(_) => Err(ChiptunePlayerError::SingleTrack),
            #[cfg(feature = "chiptune-gme")]
            Decoder::Gme(decoder) => decoder
                .select_track(track)
                .map_err(ChiptuneDecoderError::Gme)
                .map_err(ChiptunePlayerError::Decoder),
        }?;
        self.output.clear();
        self.refresh_metadata();
        Ok(())
    }

    fn change_random(&mut self) -> Result<(), ChiptunePlayerError> {
        let count = self.files.len();
        let offset = if count > 1 {
            1 + self.random.next_usize(count - 1)
        } else {
            0
        };
        let start = self.file_index.saturating_add(offset) % count;
        let mut last_error = String::new();
        let mut replacement = None;
        for attempt in 0..count {
            let candidate = start.saturating_add(attempt) % count;
            if count > 1 && candidate == self.file_index {
                continue;
            }
            let Some(path) = self.files.get(candidate) else {
                continue;
            };
            match Decoder::open(path) {
                Ok(decoder) => {
                    replacement = Some((candidate, decoder));
                    break;
                }
                Err(error) => last_error = format!("{}: {error}", path.display()),
            }
        }
        let Some((candidate, decoder)) = replacement else {
            return Err(ChiptunePlayerError::NoPlayableFiles {
                attempted: count,
                last_error,
            });
        };
        self.decoder = decoder;
        self.file_index = candidate;
        self.output.clear();
        self.refresh_metadata();
        let tracks = self.track_count();
        if tracks > 1 {
            let track = self.random.next_usize(tracks);
            self.select_track(track)?;
        }
        Ok(())
    }

    fn refresh_metadata(&mut self) {
        let fallback = self
            .files
            .get(self.file_index)
            .map_or_else(String::new, |path| filename_title(path));
        match &self.decoder {
            Decoder::Ogg(decoder) => {
                self.title = if decoder.title().is_empty() {
                    fallback
                } else {
                    decoder.title().to_owned()
                };
                decoder.artist().clone_into(&mut self.subtitle);
                "OGG VORBIS".clone_into(&mut self.system);
            }
            #[cfg(feature = "chiptune-gme")]
            Decoder::Gme(decoder) => {
                self.title = if decoder.title().is_empty() {
                    fallback
                } else {
                    decoder.title().to_owned()
                };
                decoder.game().clone_into(&mut self.subtitle);
                if !decoder.author().is_empty() {
                    if !self.subtitle.is_empty() {
                        self.subtitle.push_str(" - ");
                    }
                    self.subtitle.push_str(decoder.author());
                }
                decoder.system().clone_into(&mut self.system);
            }
        }
    }
}

/// Playlist, decoder, or fixed-output failure.
#[derive(Debug)]
pub enum ChiptunePlayerError {
    /// The catalog contains no supported paths.
    NoFiles,
    /// Every attempted path was rejected.
    NoPlayableFiles {
        /// Paths attempted in this navigation operation.
        attempted: usize,
        /// Final bounded diagnostic.
        last_error: String,
    },
    /// The active format exposes only one track.
    SingleTrack,
    /// One active decoder operation failed.
    Decoder(ChiptuneDecoderError),
    /// The fixed output block could not be reserved.
    AllocateOutput(TryReserveError),
}

impl fmt::Display for ChiptunePlayerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFiles => formatter.write_str("no supported chiptunes found"),
            Self::NoPlayableFiles {
                attempted,
                last_error,
            } => write!(
                formatter,
                "none of {attempted} chiptunes opened; last error: {last_error}"
            ),
            Self::SingleTrack => formatter.write_str("current chiptune has one track"),
            Self::Decoder(source) => write!(formatter, "chiptune decoder failed: {source}"),
            Self::AllocateOutput(source) => {
                write!(formatter, "cannot allocate chiptune output: {source}")
            }
        }
    }
}

impl Error for ChiptunePlayerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Decoder(source) => Some(source),
            Self::AllocateOutput(source) => Some(source),
            Self::NoFiles | Self::NoPlayableFiles { .. } | Self::SingleTrack => None,
        }
    }
}

#[derive(Debug)]
enum Decoder {
    Ogg(Box<OggDecoder>),
    #[cfg(feature = "chiptune-gme")]
    Gme(GmeDecoder),
}

impl Decoder {
    fn open(path: &Path) -> Result<Self, ChiptuneDecoderError> {
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ogg"))
        {
            return OggDecoder::open(path)
                .map(Box::new)
                .map(Self::Ogg)
                .map_err(ChiptuneDecoderError::Ogg);
        }
        #[cfg(feature = "chiptune-gme")]
        {
            GmeDecoder::open(path)
                .map(Self::Gme)
                .map_err(ChiptuneDecoderError::Gme)
        }
        #[cfg(not(feature = "chiptune-gme"))]
        {
            Err(ChiptuneDecoderError::GameMusicUnavailable)
        }
    }
}

/// Failure from the selected native music decoder.
#[derive(Debug)]
pub enum ChiptuneDecoderError {
    /// Pure-Rust Ogg Vorbis failure.
    Ogg(super::OggDecoderError),
    #[cfg(feature = "chiptune-gme")]
    /// Game Music Emu core failure.
    Gme(super::GmeDecoderError),
    #[cfg(not(feature = "chiptune-gme"))]
    /// A non-Ogg file was selected in a build without Game Music Emu.
    GameMusicUnavailable,
}

impl fmt::Display for ChiptuneDecoderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ogg(source) => source.fmt(formatter),
            #[cfg(feature = "chiptune-gme")]
            Self::Gme(source) => source.fmt(formatter),
            #[cfg(not(feature = "chiptune-gme"))]
            Self::GameMusicUnavailable => formatter.write_str("Game Music Emu support is disabled"),
        }
    }
}

impl Error for ChiptuneDecoderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Ogg(source) => Some(source),
            #[cfg(feature = "chiptune-gme")]
            Self::Gme(source) => Some(source),
            #[cfg(not(feature = "chiptune-gme"))]
            Self::GameMusicUnavailable => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RandomState(u32);

impl RandomState {
    const fn new(seed: u32) -> Self {
        Self(if seed == 0 { 0x6d2b_79f5 } else { seed })
    }

    fn next_usize(&mut self, modulus: usize) -> usize {
        if modulus == 0 {
            return 0;
        }
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        usize::try_from(self.0).unwrap_or_default() % modulus
    }
}

fn filename_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .replace(['-', '_'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> ChiptuneCatalog {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../chiptunes");
        ChiptuneCatalog::scan(path).expect("tracked chiptune catalog scans")
    }

    #[test]
    fn tracked_playlist_opens_and_moves_without_reallocating_output() {
        let mut player = ChiptunePlayer::open(catalog(), 1).expect("tracked playlist opens");
        assert_eq!(player.file_count(), 10);
        assert_eq!(player.file_index(), 0);
        let tick = player.decode_block().expect("tracked playlist decodes");
        assert_eq!(tick.frames(), FRAMES_PER_BLOCK);
        assert!(!tick.ended());
        let allocation = player.waveform().as_ptr();
        player.change_file(1).expect("next tracked file opens");
        assert_eq!(player.file_index(), 1);
        let tick = player.decode_block().expect("next tracked file decodes");
        assert_eq!(tick.frames(), FRAMES_PER_BLOCK);
        assert_eq!(allocation, player.waveform().as_ptr());
        player.change_file(-1).expect("previous tracked file opens");
        assert_eq!(player.file_index(), 0);
    }

    #[test]
    fn loop_one_restarts_the_current_file() {
        let mut player = ChiptunePlayer::open(catalog(), 2).expect("tracked playlist opens");
        let first = player
            .decode_block()
            .expect("tracked playlist decodes")
            .frames();
        assert_eq!(first, FRAMES_PER_BLOCK);
        let expected = player.waveform().to_vec();
        player
            .advance_after_end(PlaybackMode::LoopOne)
            .expect("loop-one restart succeeds");
        player.decode_block().expect("restarted playlist decodes");
        assert_eq!(player.waveform(), expected);
    }

    #[test]
    fn filename_fallback_is_human_readable() {
        assert_eq!(
            filename_title(Path::new("/music/opening-theme.ogg")),
            "opening theme"
        );
    }

    #[test]
    fn one_explicit_file_opens_without_catalog_discovery() {
        let catalog = catalog();
        let Some(path) = catalog.files().first() else {
            return;
        };
        let mut player =
            ChiptunePlayer::open_file(path, 7).expect("one tracked chiptune opens directly");
        assert_eq!(player.file_count(), 1);
        assert_eq!(player.file_index(), 0);
        assert_eq!(
            player
                .decode_block()
                .expect("direct chiptune decodes")
                .frames(),
            FRAMES_PER_BLOCK
        );
    }

    #[test]
    fn shuffle_selects_another_file_when_available() {
        let mut player = ChiptunePlayer::open(catalog(), 0x1234).expect("tracked playlist opens");
        let before = player.file_index();
        player
            .advance_after_end(PlaybackMode::Shuffle)
            .expect("tracked playlist shuffles");
        assert_ne!(player.file_index(), before);
    }
}
