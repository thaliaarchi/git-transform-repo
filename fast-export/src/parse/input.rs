// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    cell::UnsafeCell,
    io::{self, BufRead, Read},
};

use crate::{
    command::DataHeader,
    parse::{BufPool, DataReaderError, DataState, PResult, ParseError},
};

/// Input for a fast-export stream.
pub(super) struct Input<R> {
    /// Reader for the fast-export stream.
    r: R,
    /// Whether the reader has reached EOF.
    eof: bool,
    /// The current line number.
    line: u64,
}

pub(super) struct BufInput<R> {
    input: UnsafeCell<Input<R>>,
    lines: BufPool,
    unread: UnsafeCell<bool>,
}

impl<R: BufRead> Input<R> {
    #[inline(always)]
    pub fn new(input: R) -> Self {
        Input {
            r: input,
            eof: false,
            line: 0,
        }
    }

    /// Reads a line from this input into `buf`, stripping the LF delimiter.
    /// Lines may contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `strbuf_getline_lf` in strbuf.c.
    #[inline(always)]
    fn read_line<'a>(&mut self, buf: &'a mut Vec<u8>) -> io::Result<&'a [u8]> {
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
        Ok(&buf[start..end])
    }

    /// Reads all of the counted data stream into `buf`.
    pub fn read_counted_data_to_end(&mut self, len: u64, buf: &mut Vec<u8>) -> PResult<usize> {
        if usize::try_from(len).is_err() {
            return Err(io::ErrorKind::OutOfMemory.into());
        }
        buf.reserve(len as usize);
        let start = buf.len();
        let n = (&mut self.r).take(len).read_to_end(buf)?;
        self.line += count_lf(&buf[start..]);
        if (n as u64) < len {
            return Err(ParseError::DataUnexpectedEof.into());
        }
        debug_assert!(n as u64 == len, "misbehaving Take implementation");
        Ok(n)
    }

    /// Reads all of the delimited data stream into `buf`.
    pub fn read_delimited_data_to_end(
        &mut self,
        delim: &[u8],
        buf: &mut Vec<u8>,
    ) -> PResult<usize> {
        let start = buf.len();
        loop {
            let len = buf.len();
            let line = self.read_line(buf)?;
            if line == delim {
                buf.truncate(len);
                return Ok(len - start);
            }
        }
    }

    /// Reads from the data stream into `buf`.
    pub fn read_data(&mut self, buf: &mut [u8], s: &mut DataState) -> PResult<usize> {
        if s.closed {
            return Err(DataReaderError::Closed.into());
        }
        if buf.is_empty() || s.finished {
            return Ok(0);
        }
        if s.is_counted {
            if self.eof {
                return Err(ParseError::DataUnexpectedEof.into());
            }
            let end = usize::try_from(s.len - s.len_read)
                .unwrap_or(usize::MAX)
                .min(buf.len());
            let n = self.r.read(&mut buf[..end])?;
            debug_assert!(n <= end, "misbehaving Read implementation");
            s.len_read += n as u64;
            if s.len_read >= s.len {
                debug_assert!(s.len_read == s.len, "read too many bytes");
                s.finished = true;
            }
            self.line += count_lf(&buf[..n]);
            Ok(n)
        } else {
            if s.line_offset >= s.line_buf.len() {
                if self.eof {
                    return Err(ParseError::UnterminatedData.into());
                }
                s.line_buf.clear();
                s.line_offset = 0;
                let line = self.read_line(&mut s.line_buf)?;
                if line == s.delim {
                    s.finished = true;
                    return Ok(0);
                }
                if s.line_buf.is_empty() {
                    // Avoid returning `Ok(0)`
                    return Err(ParseError::UnterminatedData.into());
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

    /// Reads to the end of the data stream without copying it.
    pub fn skip_data(&mut self, s: &mut DataState) -> PResult<u64> {
        if s.closed {
            return Err(DataReaderError::Closed.into());
        }
        if s.finished {
            return Ok(0);
        }
        let start_len = s.len_read;
        if s.is_counted {
            while s.len_read < s.len {
                let buf = self.r.fill_buf()?;
                if buf.is_empty() {
                    self.eof = true;
                    return Err(ParseError::DataUnexpectedEof.into());
                }
                let n = usize::try_from(s.len - s.len_read)
                    .unwrap_or(usize::MAX)
                    .min(buf.len());
                self.line += count_lf(&buf[..n]);
                self.r.consume(n);
                s.len_read += n as u64;
            }
        } else {
            s.len_read += (s.line_buf.len() - s.line_offset) as u64;
            loop {
                if self.eof {
                    return Err(ParseError::UnterminatedData.into());
                }
                s.line_buf.clear();
                let line = self.read_line(&mut s.line_buf)?;
                if line == s.delim {
                    break;
                }
                s.len_read += s.line_buf.len() as u64;
            }
        }
        s.finished = true;
        Ok(s.len_read - start_len)
    }

    #[inline(always)]
    pub fn eof(&self) -> bool {
        self.eof
    }
}

impl<R: BufRead> BufInput<R> {
    /// The number of lines (excluding data streams) from before the current
    /// command to show in a crash dump.
    const CONTEXT_LINES_BEFORE: usize = 20;

    #[inline]
    pub fn new(input: R) -> Self {
        BufInput {
            input: UnsafeCell::new(Input::new(input)),
            lines: BufPool::new(),
            unread: UnsafeCell::new(false),
        }
    }

    /// Truncates the contextual lines shown in a crash dump to a fixed amount.
    #[inline]
    pub fn truncate_context(&mut self) {
        let len = Self::CONTEXT_LINES_BEFORE + *self.unread.get_mut() as usize;
        self.lines.truncate_back(len);
    }

    /// Reads a line from this input, stripping the LF delimiter and skipping
    /// any comment lines that start with `#`. Lines may contain any bytes
    /// (including NUL), except for LF.
    ///
    // Corresponds to `read_next_command` in fast-import.c.
    fn read_directive(&self) -> io::Result<Option<&[u8]>> {
        let input = unsafe { &mut *self.input.get() };
        while !input.eof() {
            let line_buf = self.lines.push_back();
            let line = input.read_line(line_buf)?;
            if !line.starts_with(b"#") {
                return Ok(Some(line));
            }
        }
        Ok(None)
    }

    /// Reads the next directive and consumes it.
    pub fn next_directive(&self) -> io::Result<Option<&[u8]>> {
        let directive = self.peek_directive()?;
        self.bump_directive();
        Ok(directive)
    }

    /// Reads the next directive without consuming it.
    pub fn peek_directive(&self) -> io::Result<Option<&[u8]>> {
        let unread = unsafe { &mut *self.unread.get() };
        if *unread {
            let back = self.lines.back();
            debug_assert!(back.is_some(), "unread line not in BufPool");
            Ok(back)
        } else {
            *unread = true;
            self.read_directive()
        }
    }

    /// Consumes the peeked directive. `bump_directive` must be preceded by
    /// `peek_directive`.
    #[inline(always)]
    pub fn bump_directive(&self) {
        let unread = unsafe { &mut *self.unread.get() };
        debug_assert!(*unread, "bump_directive not preceded by peek_directive");
        *unread = false;
    }

    #[inline(always)]
    pub fn parse_directive<'a, F, T>(&'a self, prefix: &[u8], parse: F) -> PResult<Option<T>>
    where
        F: FnOnce(&'a [u8]) -> PResult<T>,
        T: 'a,
    {
        let line = self.peek_directive()?;
        if let Some(arg) = line.and_then(|line| line.strip_prefix(prefix)) {
            self.bump_directive();
            parse(arg).map(Some)
        } else {
            Ok(None)
        }
    }

    #[inline(always)]
    pub fn parse_directive_many<'a, F, T>(&'a self, prefix: &[u8], mut parse: F) -> PResult<Vec<T>>
    where
        F: FnMut(&'a [u8]) -> PResult<T>,
        T: 'a,
    {
        let mut directives = Vec::new();
        while let Some(directive) = self.parse_directive(prefix, &mut parse)? {
            directives.push(directive);
        }
        Ok(directives)
    }

    /// Reads from the data stream into `buf`.
    pub fn read_data(&self, buf: &mut [u8], s: &mut DataState) -> PResult<usize> {
        let input = unsafe { &mut *self.input.get() };
        // TODO: Handle optional LF
        input.read_data(buf, s)
    }

    /// Reads to the end of the data stream without copying it.
    pub fn skip_data(&self, s: &mut DataState) -> PResult<u64> {
        let input = unsafe { &mut *self.input.get() };
        let n = input.skip_data(s)?;
        // TODO: Could consume LF twice
        // TODO: Move DataState handling to BufInput
        self.skip_optional_lf()?;
        Ok(n)
    }

    /// Reads all of the data stream into `buf`.
    pub fn read_data_to_end(&self, header: DataHeader<&[u8]>, buf: &mut Vec<u8>) -> PResult<usize> {
        let input = unsafe { &mut *self.input.get() };
        let len = match header {
            DataHeader::Counted { len } => input.read_counted_data_to_end(len, buf)?,
            DataHeader::Delimited { delim } => input.read_delimited_data_to_end(delim, buf)?,
        };
        self.skip_optional_lf()?;
        Ok(len)
    }

    /// Skips a trailing LF, if one exists, before reading the next directive.
    pub fn skip_optional_lf(&self) -> PResult<()> {
        if self.peek_directive()? == Some(b"") {
            self.bump_directive();
        }
        Ok(())
    }
}

fn count_lf(buf: &[u8]) -> u64 {
    buf.iter().filter(|&&b| b == b'\n').count() as u64
}
