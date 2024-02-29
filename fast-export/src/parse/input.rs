// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::io::{self, BufRead, Read};

use crate::parse::{DataReaderError, DataSpan, DataState, PResult, ParseError, Span};

/// Input for a fast-export stream.
pub(super) struct Input<R> {
    /// Reader for the fast-export stream.
    r: R,
    /// Whether the reader has reached EOF.
    eof: bool,
    /// The current line number.
    line: u64,
    /// Whether the previous command ended with an optional LF.
    skip_optional_lf: bool,
}

impl<R: BufRead> Input<R> {
    #[inline(always)]
    pub(super) fn new(input: R) -> Self {
        Input {
            r: input,
            eof: false,
            line: 0,
            skip_optional_lf: false,
        }
    }

    /// Reads a line from this input into `buf`, stripping the LF delimiter.
    /// Lines may contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `strbuf_getline_lf` in strbuf.c.
    #[inline(always)]
    fn read_line(&mut self, buf: &mut Vec<u8>) -> io::Result<Span> {
        debug_assert!(!self.eof, "already at EOF");
        let start = buf.len();
        self.r.read_until(b'\n', buf)?;
        let mut end = buf.len();
        if let [.., b'\n'] = &buf[start..] {
            end -= 1;
        } else {
            // EOF is reached in `read_until` iff the delimiter is not included.
            self.eof = true;
        }
        self.line += 1;
        Ok(Span::from(start..end))
    }

    /// Reads a line from this input into `buf`, stripping the LF delimiter and
    /// skipping any comment lines that start with `#`. Lines may
    /// contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `read_next_command` in fast-import.c.
    #[inline(always)]
    pub(super) fn read_command(&mut self, buf: &mut Vec<u8>, cursor: &mut Span) -> io::Result<()> {
        loop {
            if self.eof {
                cursor.start = cursor.end;
                break;
            }
            *cursor = self.read_line(buf)?;
            let line = cursor.slice(buf);
            if self.skip_optional_lf {
                self.skip_optional_lf = false;
                if line.is_empty() {
                    // If we are at the start of a command, but the LF is from
                    // the previous, clear it.
                    if cursor.start == 0 {
                        buf.clear();
                    }
                    continue;
                }
            }
            if !line.starts_with(b"#") {
                break;
            }
        }
        Ok(())
    }

    /// Reads all of the described data stream into `command_buf`. The delimiter
    /// span in `header` must be in `command_buf`.
    pub(super) fn read_data_to_end(
        &mut self,
        header: DataSpan,
        command_buf: &mut Vec<u8>,
    ) -> PResult<()> {
        match header {
            DataSpan::Counted { len } => {
                if usize::try_from(len).is_err() {
                    return Err(io::ErrorKind::OutOfMemory.into());
                }
                // When `Read::read_buf` is stabilized, it might be worth using
                // it directly.
                let start = command_buf.len();
                let n = (&mut self.r).take(len).read_to_end(command_buf)?;
                self.line += count_lf(&command_buf[start..]);
                if (n as u64) < len {
                    return Err(ParseError::DataUnexpectedEof.into());
                }
                debug_assert!(n as u64 == len, "misbehaving Take implementation");
            }
            DataSpan::Delimited { delim } => loop {
                let len = command_buf.len();
                let line = self.read_line(command_buf)?;
                if line.slice(command_buf) == delim.slice(command_buf) {
                    command_buf.truncate(len);
                    break;
                }
            },
        }
        Ok(())
    }

    /// Reads from the current data stream into `buf`. The delimiter span must
    /// be in `command_buf`.
    #[inline(always)]
    pub(super) fn read_data(
        &mut self,
        buf: &mut [u8],
        s: &mut DataState,
        command_buf: &[u8],
    ) -> PResult<usize> {
        if s.closed {
            return Err(DataReaderError::Closed.into());
        }
        if buf.is_empty() || s.finished {
            return Ok(0);
        }
        match s.header {
            DataSpan::Counted { len } => {
                if self.eof {
                    return Err(ParseError::DataUnexpectedEof.into());
                }
                let end = usize::try_from(len - s.len_read)
                    .unwrap_or(usize::MAX)
                    .min(buf.len());
                let n = self.r.read(&mut buf[..end])?;
                debug_assert!(n <= end, "misbehaving Read implementation");
                s.len_read += n as u64;
                if s.len_read >= len {
                    debug_assert!(s.len_read == len, "read too many bytes");
                    s.finished = true;
                }
                self.line += count_lf(buf);
                Ok(n)
            }
            DataSpan::Delimited { delim } => {
                let delim = delim.slice(command_buf);
                if s.line_offset >= s.line_buf.len() {
                    if self.eof {
                        return Err(ParseError::UnterminatedData.into());
                    }
                    s.line_buf.clear();
                    s.line_offset = 0;
                    let line = self.read_line(&mut s.line_buf)?;
                    if line.slice(&s.line_buf) == delim {
                        s.finished = true;
                        return Ok(0);
                    }
                }
                let off = s.line_offset;
                let n = (s.line_buf.len() - off).min(buf.len());
                buf[..n].copy_from_slice(&s.line_buf[off..off + n]);
                s.line_offset += n;
                s.len_read += n as u64;
                Ok(n)
            }
        }
    }

    /// Reads to the end of the data stream without consuming it.
    #[inline(always)]
    pub(super) fn skip_data(&mut self, s: &mut DataState, command_buf: &[u8]) -> PResult<u64> {
        if s.closed {
            return Err(DataReaderError::Closed.into());
        }
        if s.finished {
            return Ok(0);
        }
        let start_len = s.len_read;
        match s.header {
            DataSpan::Counted { len } => {
                while s.len_read < len {
                    let buf = self.r.fill_buf()?;
                    if buf.is_empty() {
                        self.eof = true;
                        return Err(ParseError::DataUnexpectedEof.into());
                    }
                    let n = usize::try_from(len - s.len_read)
                        .unwrap_or(usize::MAX)
                        .min(buf.len());
                    self.line += count_lf(buf);
                    self.r.consume(n);
                    s.len_read += n as u64;
                }
            }
            DataSpan::Delimited { delim } => {
                let delim = delim.slice(command_buf);
                loop {
                    if self.eof {
                        return Err(ParseError::UnterminatedData.into());
                    }
                    s.line_buf.clear();
                    let line = self.read_line(&mut s.line_buf)?;
                    if line.slice(&s.line_buf) == delim {
                        break;
                    }
                    s.len_read += s.line_buf.len() as u64;
                }
            }
        }
        s.finished = true;
        Ok(s.len_read - start_len)
    }

    #[inline(always)]
    pub(super) fn eof(&self) -> bool {
        self.eof
    }

    #[inline(always)]
    pub(super) fn skip_optional_lf(&mut self) {
        debug_assert!(!self.skip_optional_lf, "already skipping optional LF");
        self.skip_optional_lf = true;
    }
}

fn count_lf(buf: &[u8]) -> u64 {
    buf.iter().filter(|&&b| b == b'\n').count() as u64
}
