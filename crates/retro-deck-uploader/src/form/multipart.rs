//! Strict, bounded `multipart/form-data` ROM intake.

use std::{
    fmt,
    ops::Range,
    str::{self, FromStr as _},
};

use crate::rom::{GameTitle, System};

use super::FormError;

/// Largest accepted `multipart/form-data` request, including framing.
pub const MAXIMUM_UPLOAD_REQUEST_BYTES: usize = 12 * 1_024 * 1_024;

const MAXIMUM_MULTIPART_HEADER_BYTES: usize = 4_096;
const MAXIMUM_MULTIPART_PARTS: usize = 4;
const MAXIMUM_FILENAME_BYTES: usize = 255;
const MAXIMUM_TEXT_FIELD_BYTES: usize = 256;
const MAXIMUM_BOUNDARY_BYTES: usize = 70;

/// A complete, structurally validated ROM upload form.
pub struct RomUploadForm {
    csrf: String,
    system: System,
    title: String,
    filename: String,
    body: Vec<u8>,
    rom: Range<usize>,
}

impl RomUploadForm {
    /// Decode exactly `csrf`, `system`, `title`, and one `rom` file part.
    ///
    /// The input buffer becomes the form's backing storage. ROM bytes are
    /// represented by a checked range instead of being copied.
    ///
    /// # Errors
    ///
    /// Returns [`FormError`] for an unsupported content type, an unsafe or
    /// malformed boundary, malformed multipart framing or headers, repeated
    /// or unknown parts, invalid text, or a missing required part.
    pub fn parse(content_type: &str, body: Vec<u8>) -> Result<Self, FormError> {
        if body.len() > MAXIMUM_UPLOAD_REQUEST_BYTES {
            return Err(FormError::BodyTooLarge);
        }
        let boundary = multipart_boundary(content_type)?;
        MultipartDecoder::new(body, &boundary)?.decode()
    }

    /// Submitted anti-forgery token.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn csrf(&self) -> &str {
        &self.csrf
    }

    /// Selected ROM system.
    #[must_use]
    pub const fn system(&self) -> System {
        self.system
    }

    /// Validated display title.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Browser-supplied basename used only for format selection.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Uploaded ROM or ZIP bytes without a second allocation.
    #[must_use]
    pub fn contents(&self) -> &[u8] {
        self.body.get(self.rom.clone()).unwrap_or_default()
    }
}

impl fmt::Debug for RomUploadForm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RomUploadForm")
            .field("csrf", &"[redacted]")
            .field("system", &self.system)
            .field("title", &self.title)
            .field("filename", &self.filename)
            .field("rom_bytes", &self.rom.len())
            .finish_non_exhaustive()
    }
}

fn multipart_boundary(content_type: &str) -> Result<String, FormError> {
    let segments = parameter_segments(content_type).map_err(|()| FormError::InvalidBoundary)?;
    let Some(media_type) = segments.first() else {
        return Err(FormError::UnsupportedMediaType);
    };
    if !media_type.eq_ignore_ascii_case("multipart/form-data") {
        return Err(FormError::UnsupportedMediaType);
    }
    let mut boundary = None;
    for segment in segments.iter().skip(1) {
        let (name, value) = parameter(segment).map_err(|()| FormError::InvalidBoundary)?;
        if !name.eq_ignore_ascii_case("boundary") || boundary.is_some() {
            return Err(FormError::InvalidBoundary);
        }
        boundary = Some(value);
    }
    let boundary = boundary.ok_or(FormError::InvalidBoundary)?;
    if boundary.is_empty()
        || boundary.len() > MAXIMUM_BOUNDARY_BYTES
        || boundary.ends_with(' ')
        || !boundary.bytes().all(valid_boundary_byte)
    {
        return Err(FormError::InvalidBoundary);
    }
    Ok(boundary)
}

const fn valid_boundary_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'\''
                | b'('
                | b')'
                | b'+'
                | b'_'
                | b','
                | b'-'
                | b'.'
                | b'/'
                | b':'
                | b'='
                | b'?'
                | b' '
        )
}

fn parameter_segments(value: &str) -> Result<Vec<&str>, ()> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && character == '\\' {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == ';' && !quoted {
            let segment = value.get(start..index).ok_or(())?.trim();
            if segment.is_empty() {
                return Err(());
            }
            segments.push(segment);
            start = index.saturating_add(character.len_utf8());
        }
    }
    if quoted || escaped {
        return Err(());
    }
    let segment = value.get(start..).ok_or(())?.trim();
    if segment.is_empty() {
        return Err(());
    }
    segments.push(segment);
    Ok(segments)
}

fn parameter(segment: &str) -> Result<(&str, String), ()> {
    let (name, raw_value) = segment.split_once('=').ok_or(())?;
    let name = name.trim();
    let raw_value = raw_value.trim();
    if name.is_empty() || !name.bytes().all(valid_parameter_name_byte) {
        return Err(());
    }
    decode_parameter_value(raw_value).map(|value| (name, value))
}

const fn valid_parameter_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

fn decode_parameter_value(raw: &str) -> Result<String, ()> {
    if let Some(quoted) = raw.strip_prefix('"') {
        let quoted = quoted.strip_suffix('"').ok_or(())?;
        let mut decoded = String::with_capacity(quoted.len());
        let mut characters = quoted.chars();
        while let Some(character) = characters.next() {
            if character == '\\' {
                let escaped = characters.next().ok_or(())?;
                if !matches!(escaped, '\\' | '"') {
                    return Err(());
                }
                decoded.push(escaped);
            } else if character == '"' || character.is_control() {
                return Err(());
            } else {
                decoded.push(character);
            }
        }
        return Ok(decoded);
    }
    if raw.is_empty() || !raw.bytes().all(valid_token_byte) {
        return Err(());
    }
    Ok(raw.to_owned())
}

const fn valid_token_byte(byte: u8) -> bool {
    byte > 0x20
        && byte < 0x7f
        && !matches!(
            byte,
            b'(' | b')'
                | b'<'
                | b'>'
                | b'@'
                | b','
                | b';'
                | b':'
                | b'\\'
                | b'"'
                | b'/'
                | b'['
                | b']'
                | b'?'
                | b'='
                | b'{'
                | b'}'
        )
}

struct MultipartDecoder {
    body: Vec<u8>,
    marker: Vec<u8>,
    cursor: usize,
    parts: usize,
    builder: UploadBuilder,
}

impl MultipartDecoder {
    fn new(body: Vec<u8>, boundary: &str) -> Result<Self, FormError> {
        let mut delimiter = Vec::with_capacity(boundary.len().saturating_add(2));
        delimiter.extend_from_slice(b"--");
        delimiter.extend_from_slice(boundary.as_bytes());
        let mut marker = Vec::with_capacity(delimiter.len().saturating_add(2));
        marker.extend_from_slice(b"\r\n");
        marker.extend_from_slice(&delimiter);
        if !body.starts_with(&delimiter) {
            return Err(FormError::MalformedMultipart);
        }
        let cursor = delimiter.len();
        let rest = body.get(cursor..).ok_or(FormError::MalformedMultipart)?;
        if !rest.starts_with(b"\r\n") {
            return Err(FormError::MalformedMultipart);
        }
        Ok(Self {
            body,
            marker,
            cursor: cursor.saturating_add(2),
            parts: 0,
            builder: UploadBuilder::default(),
        })
    }

    fn decode(mut self) -> Result<RomUploadForm, FormError> {
        loop {
            self.parts = self.parts.saturating_add(1);
            if self.parts > MAXIMUM_MULTIPART_PARTS {
                return Err(FormError::UnexpectedPart);
            }
            let remaining = self
                .body
                .get(self.cursor..)
                .ok_or(FormError::MalformedMultipart)?;
            let header_length =
                find_bytes(remaining, b"\r\n\r\n").ok_or(FormError::MalformedPartHeaders)?;
            if header_length == 0 || header_length > MAXIMUM_MULTIPART_HEADER_BYTES {
                return Err(FormError::MalformedPartHeaders);
            }
            let header_end = self.cursor.saturating_add(header_length);
            let header = self
                .body
                .get(self.cursor..header_end)
                .ok_or(FormError::MalformedPartHeaders)?;
            let part_header = PartHeader::parse(header)?;
            let content_start = header_end.saturating_add(4);
            let content = self
                .body
                .get(content_start..)
                .ok_or(FormError::MalformedMultipart)?;
            let marker_offset =
                find_delimiter(content, &self.marker).ok_or(FormError::MalformedMultipart)?;
            let content_end = content_start.saturating_add(marker_offset);
            let range = content_start..content_end;
            let contents = self
                .body
                .get(range.clone())
                .ok_or(FormError::MalformedMultipart)?;
            self.builder.insert(part_header, contents, range)?;

            let marker_end = content_end.saturating_add(self.marker.len());
            let suffix = self
                .body
                .get(marker_end..)
                .ok_or(FormError::MalformedMultipart)?;
            if let Some(trailing) = suffix.strip_prefix(b"--") {
                if !trailing.is_empty() && trailing != b"\r\n" {
                    return Err(FormError::MalformedMultipart);
                }
                return self.builder.finish(self.body);
            }
            if !suffix.starts_with(b"\r\n") {
                return Err(FormError::MalformedMultipart);
            }
            self.cursor = marker_end.saturating_add(2);
        }
    }
}

fn find_delimiter(haystack: &[u8], marker: &[u8]) -> Option<usize> {
    let mut offset = 0;
    while let Some(candidate) = find_bytes(haystack.get(offset..)?, marker) {
        let position = offset.saturating_add(candidate);
        let suffix = haystack.get(position.saturating_add(marker.len())..)?;
        if suffix.starts_with(b"\r\n")
            || suffix
                .strip_prefix(b"--")
                .is_some_and(|trailing| trailing.is_empty() || trailing == b"\r\n")
        {
            return Some(position);
        }
        offset = position.saturating_add(1);
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[derive(Debug)]
struct PartHeader {
    name: String,
    filename: Option<String>,
    has_content_type: bool,
}

impl PartHeader {
    fn parse(headers: &[u8]) -> Result<Self, FormError> {
        let mut disposition = None;
        let mut has_content_type = false;
        let mut remaining = headers;
        loop {
            let (line, next) = match find_bytes(remaining, b"\r\n") {
                Some(end) => (
                    remaining
                        .get(..end)
                        .ok_or(FormError::MalformedPartHeaders)?,
                    Some(
                        remaining
                            .get(end.saturating_add(2)..)
                            .ok_or(FormError::MalformedPartHeaders)?,
                    ),
                ),
                None => (remaining, None),
            };
            parse_header_line(line, &mut disposition, &mut has_content_type)?;
            let Some(next) = next else {
                break;
            };
            remaining = next;
        }
        let disposition = disposition.ok_or(FormError::MalformedPartHeaders)?;
        let segments =
            parameter_segments(disposition).map_err(|()| FormError::MalformedPartHeaders)?;
        let Some(kind) = segments.first() else {
            return Err(FormError::MalformedPartHeaders);
        };
        if !kind.eq_ignore_ascii_case("form-data") {
            return Err(FormError::MalformedPartHeaders);
        }
        let mut name = None;
        let mut filename = None;
        for segment in segments.iter().skip(1) {
            let (parameter_name, value) =
                parameter(segment).map_err(|()| FormError::MalformedPartHeaders)?;
            if parameter_name.eq_ignore_ascii_case("name") && name.is_none() {
                name = Some(value);
            } else if parameter_name.eq_ignore_ascii_case("filename") && filename.is_none() {
                filename = Some(value);
            } else {
                return Err(FormError::MalformedPartHeaders);
            }
        }
        Ok(Self {
            name: name.ok_or(FormError::MalformedPartHeaders)?,
            filename,
            has_content_type,
        })
    }
}

fn parse_header_line<'a>(
    line: &'a [u8],
    disposition: &mut Option<&'a str>,
    has_content_type: &mut bool,
) -> Result<(), FormError> {
    if line.is_empty() || line.contains(&b'\r') || line.contains(&b'\n') {
        return Err(FormError::MalformedPartHeaders);
    }
    let separator = line
        .iter()
        .position(|byte| *byte == b':')
        .ok_or(FormError::MalformedPartHeaders)?;
    let name = line
        .get(..separator)
        .ok_or(FormError::MalformedPartHeaders)?;
    let value = line
        .get(separator.saturating_add(1)..)
        .ok_or(FormError::MalformedPartHeaders)?;
    let value = str::from_utf8(trim_ascii(value)).map_err(|_| FormError::InvalidUtf8)?;
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(FormError::MalformedPartHeaders);
    }
    if name.eq_ignore_ascii_case(b"content-disposition") && disposition.is_none() {
        *disposition = Some(value);
    } else if name.eq_ignore_ascii_case(b"content-type") && !*has_content_type {
        *has_content_type = true;
    } else {
        return Err(FormError::MalformedPartHeaders);
    }
    Ok(())
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = value.get(1..).unwrap_or_default();
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = value
            .get(..value.len().saturating_sub(1))
            .unwrap_or_default();
    }
    value
}

#[derive(Debug, Default)]
struct UploadBuilder {
    csrf: Option<String>,
    system: Option<System>,
    title: Option<String>,
    filename: Option<String>,
    rom: Option<Range<usize>>,
}

impl UploadBuilder {
    fn insert(
        &mut self,
        header: PartHeader,
        contents: &[u8],
        range: Range<usize>,
    ) -> Result<(), FormError> {
        match header.name.as_str() {
            "csrf" if header.filename.is_none() && !header.has_content_type => {
                insert_once(&mut self.csrf, parse_token(contents)?)
            }
            "system" if header.filename.is_none() && !header.has_content_type => {
                let value = parse_text(contents)?;
                let system = System::from_str(value).map_err(|_| FormError::InvalidTextField)?;
                insert_once(&mut self.system, system)
            }
            "title" if header.filename.is_none() && !header.has_content_type => {
                let value = parse_text(contents)?;
                GameTitle::new(value).map_err(|_| FormError::InvalidTextField)?;
                insert_once(&mut self.title, value.to_owned())
            }
            "rom" if header.filename.is_some() => {
                let filename = header.filename.ok_or(FormError::InvalidFilename)?;
                validate_filename(&filename)?;
                if self.rom.is_some() || self.filename.is_some() {
                    return Err(FormError::RepeatedField);
                }
                self.filename = Some(filename);
                self.rom = Some(range);
                Ok(())
            }
            "csrf" | "system" | "title" | "rom" => Err(FormError::MalformedPartHeaders),
            _ => Err(FormError::UnexpectedPart),
        }
    }

    fn finish(self, body: Vec<u8>) -> Result<RomUploadForm, FormError> {
        Ok(RomUploadForm {
            csrf: self.csrf.ok_or(FormError::MissingPart)?,
            system: self.system.ok_or(FormError::MissingPart)?,
            title: self.title.ok_or(FormError::MissingPart)?,
            filename: self.filename.ok_or(FormError::MissingPart)?,
            body,
            rom: self.rom.ok_or(FormError::MissingPart)?,
        })
    }
}

fn insert_once<T>(destination: &mut Option<T>, value: T) -> Result<(), FormError> {
    if destination.replace(value).is_some() {
        Err(FormError::RepeatedField)
    } else {
        Ok(())
    }
}

fn parse_text(contents: &[u8]) -> Result<&str, FormError> {
    if contents.is_empty() || contents.len() > MAXIMUM_TEXT_FIELD_BYTES {
        return Err(FormError::InvalidTextField);
    }
    str::from_utf8(contents).map_err(|_| FormError::InvalidUtf8)
}

fn parse_token(contents: &[u8]) -> Result<String, FormError> {
    let token = parse_text(contents)?;
    if token.len() != 43
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(FormError::InvalidTextField);
    }
    Ok(token.to_owned())
}

fn validate_filename(filename: &str) -> Result<(), FormError> {
    if filename.is_empty()
        || filename.len() > MAXIMUM_FILENAME_BYTES
        || filename.contains(['/', '\\', '\0'])
        || filename.chars().any(char::is_control)
    {
        Err(FormError::InvalidFilename)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Part<'a> = (&'a str, Option<&'a str>, Option<&'a str>, &'a [u8]);

    const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn multipart(parts: &[Part<'_>]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, filename, content_type, contents) in parts {
            body.extend_from_slice(b"--boundary\r\nContent-Disposition: form-data; name=\"");
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(b"\"");
            if let Some(filename) = filename {
                body.extend_from_slice(b"; filename=\"");
                body.extend_from_slice(filename.as_bytes());
                body.extend_from_slice(b"\"");
            }
            body.extend_from_slice(b"\r\n");
            if let Some(content_type) = content_type {
                body.extend_from_slice(b"Content-Type: ");
                body.extend_from_slice(content_type.as_bytes());
                body.extend_from_slice(b"\r\n");
            }
            body.extend_from_slice(b"\r\n");
            body.extend_from_slice(contents);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(b"--boundary--\r\n");
        body
    }

    fn complete_parts(rom: &[u8]) -> [Part<'_>; 4] {
        [
            ("csrf", None, None, TOKEN.as_bytes()),
            ("system", None, None, b"chip8"),
            ("title", None, None, b"Space Racer"),
            (
                "rom",
                Some("space-racer.ch8"),
                Some("application/octet-stream"),
                rom,
            ),
        ]
    }

    #[test]
    fn decodes_complete_multipart_without_copying_rom() -> Result<(), FormError> {
        let bytes = b"\x00\xff\r\nnot-a--boundary";
        let body = multipart(&complete_parts(bytes));
        let form = RomUploadForm::parse("multipart/form-data; boundary=boundary", body)?;
        assert_eq!(form.csrf(), TOKEN);
        assert_eq!(form.system(), System::Chip8);
        assert_eq!(form.title(), "Space Racer");
        assert_eq!(form.filename(), "space-racer.ch8");
        assert_eq!(form.contents(), bytes);
        assert!(format!("{form:?}").contains("[redacted]"));
        assert!(!format!("{form:?}").contains(TOKEN));
        Ok(())
    }

    #[test]
    fn accepts_quoted_boundary_and_arbitrary_part_order() -> Result<(), FormError> {
        let mut parts = complete_parts(b"rom");
        parts.reverse();
        let form = RomUploadForm::parse(
            "Multipart/Form-Data; boundary=\"boundary\"",
            multipart(&parts),
        )?;
        assert_eq!(form.contents(), b"rom");
        Ok(())
    }

    #[test]
    fn ignores_boundary_prefixes_inside_file_content() -> Result<(), FormError> {
        let bytes = b"before\r\n--boundary-nope\r\n--boundary--still-data\r\nafter";
        let form = RomUploadForm::parse(
            "multipart/form-data; boundary=boundary",
            multipart(&complete_parts(bytes)),
        )?;
        assert_eq!(form.contents(), bytes);
        Ok(())
    }

    #[test]
    fn rejects_invalid_content_types_and_boundaries() {
        let body = multipart(&complete_parts(b"rom"));
        for content_type in [
            "text/plain",
            "multipart/form-data",
            "multipart/form-data; boundary=",
            "multipart/form-data; boundary=a; boundary=b",
            "multipart/form-data; charset=utf-8; boundary=boundary",
            "multipart/form-data; boundary=bad*boundary",
        ] {
            assert!(
                RomUploadForm::parse(content_type, body.clone()).is_err(),
                "{content_type}"
            );
        }
    }

    #[test]
    fn rejects_missing_repeated_unknown_and_excess_parts() {
        let complete = complete_parts(b"rom");
        assert_eq!(
            RomUploadForm::parse(
                "multipart/form-data; boundary=boundary",
                multipart(complete.get(..3).unwrap_or_default()),
            )
            .map(|_| ()),
            Err(FormError::MissingPart)
        );

        let [csrf, system, title, rom] = complete;
        let repeated = [csrf, csrf, system, title, rom];
        assert_eq!(
            RomUploadForm::parse(
                "multipart/form-data; boundary=boundary",
                multipart(&repeated),
            )
            .map(|_| ()),
            Err(FormError::RepeatedField)
        );

        let unknown = [("extra", None, None, b"value".as_slice())];
        assert_eq!(
            RomUploadForm::parse(
                "multipart/form-data; boundary=boundary",
                multipart(&unknown),
            )
            .map(|_| ()),
            Err(FormError::UnexpectedPart)
        );
    }

    #[test]
    fn rejects_file_metadata_on_text_and_paths_as_filenames() {
        let bad_text = [
            (
                "csrf",
                Some("token.txt"),
                Some("text/plain"),
                TOKEN.as_bytes(),
            ),
            ("system", None, None, b"chip8".as_slice()),
            ("title", None, None, b"Game".as_slice()),
            ("rom", Some("game.ch8"), None, b"rom".as_slice()),
        ];
        assert_eq!(
            RomUploadForm::parse(
                "multipart/form-data; boundary=boundary",
                multipart(&bad_text),
            )
            .map(|_| ()),
            Err(FormError::MalformedPartHeaders)
        );

        let mut bad_filename = complete_parts(b"rom");
        if let Some(part) = bad_filename.last_mut() {
            part.1 = Some("folder/game.ch8");
        }
        assert_eq!(
            RomUploadForm::parse(
                "multipart/form-data; boundary=boundary",
                multipart(&bad_filename),
            )
            .map(|_| ()),
            Err(FormError::InvalidFilename)
        );
    }

    #[test]
    fn rejects_malformed_framing_and_oversized_body() {
        assert_eq!(
            RomUploadForm::parse(
                "multipart/form-data; boundary=boundary",
                b"--boundary\nwrong".to_vec(),
            )
            .map(|_| ()),
            Err(FormError::MalformedMultipart)
        );
        assert_eq!(
            RomUploadForm::parse(
                "multipart/form-data; boundary=boundary",
                vec![0; MAXIMUM_UPLOAD_REQUEST_BYTES.saturating_add(1)],
            )
            .map(|_| ()),
            Err(FormError::BodyTooLarge)
        );
    }
}
