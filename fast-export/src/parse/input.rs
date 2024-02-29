// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::io::{self, BufRead};

use crate::parse::Span;

/// Input for a fast-export stream.
pub(super) struct Input<R> {
    /// Reader for the fast-export stream.
    pub(super) r: R,
    /// Whether the reader has reached EOF.
    pub(super) eof: bool,
    /// The current line number.
    pub(super) line: u64,
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
    pub(super) fn read_line(&mut self, buf: &mut Vec<u8>) -> io::Result<Span> {
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

    #[inline(always)]
    pub(super) fn skip_optional_lf(&mut self) {
        debug_assert!(!self.skip_optional_lf, "already skipping optional LF");
        self.skip_optional_lf = true;
    }
}
