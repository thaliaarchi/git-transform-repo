use std::{
    fmt::{self, Debug, Formatter},
    io::{self, BufRead},
    ops::Range,
    str,
};

use thiserror::Error;

use crate::command::{Blob, Command, Data, Done, Mark, OriginalOid, Progress};

type Result<T> = std::result::Result<T, ParseError>;

/// Parser for fast-export streams.
pub struct Parser<R: BufRead> {
    input: R,
    command_buf: Vec<u8>,
    cursor: Span,
    eof: bool,
}

type Span = Range<usize>;

/// An error from parsing a fast-export stream.
#[derive(Debug, Error)]
#[error(transparent)]
pub enum ParseError {
    Command(#[from] CommandError),
    Io(#[from] io::Error),
}

/// An error from parsing a command in a fast-export stream.
#[derive(Clone, Debug, Error, PartialEq, Eq, Hash)]
#[error("{kind}: {}", String::from_utf8_lossy(line))]
pub struct CommandError {
    pub kind: CommandErrorKind,
    pub line: Vec<u8>,
}

/// A kind of error from parsing a command in a fast-export stream.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Hash)]
pub enum CommandErrorKind {
    #[error("invalid mark")]
    InvalidMark,
    #[error("cannot use :0 as a mark")]
    ZeroMark,
    #[error("unsupported command")]
    UnsupportedCommand,
}

use CommandErrorKind as ErrorKind;

impl<R: BufRead> Parser<R> {
    /// Parses the next command in the fast-export stream.
    ///
    /// The parsed commands borrow from the parser's buffer, so need to be
    /// copied if they are retained.
    ///
    // Corresponds to the loop in `cmd_fast_import` in fast-import.c.
    pub fn next(&mut self) -> Result<Command<'_>> {
        self.command_buf.clear();
        self.bump_line()?;
        if self.eof {
            return Ok(Command::Done(Done::Eof));
        }
        if self.eat_all(b"blob") {
            self.parse_blob()
        } else if self.eat_prefix(b"commit ") {
            self.parse_commit()
        } else if self.eat_prefix(b"tag ") {
            self.parse_tag()
        } else if self.eat_prefix(b"reset ") {
            self.parse_reset()
        } else if self.eat_prefix(b"ls ") {
            self.parse_ls()
        } else if self.eat_prefix(b"cat-blob ") {
            self.parse_cat_blob()
        } else if self.eat_prefix(b"get-mark ") {
            self.parse_get_mark()
        } else if self.eat_all(b"checkpoint") {
            self.parse_checkpoint()
        } else if self.eat_all(b"done") {
            Ok(Command::Done(Done::Explicit))
        } else if self.eat_all(b"alias") {
            self.parse_alias()
        } else if self.eat_prefix(b"progress ") {
            self.parse_progress()
        } else if self.eat_prefix(b"feature ") {
            self.parse_feature()
        } else if self.eat_prefix(b"option ") {
            self.parse_option()
        } else {
            Err(self.err(ErrorKind::UnsupportedCommand))
        }
    }

    // Corresponds to `parse_new_blob` in fast-import.c.
    fn parse_blob(&mut self) -> Result<Command<'_>> {
        self.bump_line()?;
        let mark = self.parse_mark()?;
        let original_oid = self.parse_original_oid()?;
        let (data, delim) = self.parse_data()?;
        Ok(Command::Blob(Blob {
            mark,
            original_oid: original_oid.map(|sp| OriginalOid { oid: self.get(sp) }),
            data: Data {
                data: self.get(data),
                delim: delim.map(|sp| self.get(sp)),
            },
        }))
    }

    // Corresponds to `parse_new_commit` in fast-import.c.
    fn parse_commit(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    // Corresponds to `parse_new_tag` in fast-import.c.
    fn parse_tag(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    // Corresponds to `parse_reset_branch` in fast-import.c.
    fn parse_reset(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    // Corresponds to `parse_ls` in fast-import.c.
    fn parse_ls(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    // Corresponds to `parse_get_mark` in fast-import.c.
    fn parse_get_mark(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    // Corresponds to `parse_checkpoint` in fast-import.c.
    fn parse_checkpoint(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    // Corresponds to `parse_alias` in fast-import.c.
    fn parse_alias(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    // Corresponds to `parse_progress` in fast-import.c.
    fn parse_progress(&mut self) -> Result<Command<'_>> {
        let message = self.cursor();
        self.skip_optional_lf()?;
        Ok(Command::Progress(Progress {
            message: self.get(message),
        }))
    }

    // Corresponds to `parse_feature` in fast-import.c.
    fn parse_feature(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    // Corresponds to `parse_option` in fast-import.c.
    fn parse_option(&mut self) -> Result<Command<'_>> {
        todo!()
    }

    /// # Differences from fast-import
    ///
    /// `mark :0` is rejected here, but not by fast-import.
    ///
    /// filter-repo does not check any errors for this integer. It allows `+`
    /// sign, parse errors, empty digits, and junk after the integer.
    ///
    // Corresponds to `parse_mark` in fast-import.c.
    fn parse_mark(&mut self) -> Result<Option<Mark>> {
        if self.eat_prefix(b"mark :") {
            let rest = self.line_remaining();
            // SAFETY: from_str_radix operates on byes and accepts only ASCII.
            let mark = u64::from_str_radix(unsafe { str::from_utf8_unchecked(rest) }, 10)
                .map_err(|_| self.err(ErrorKind::InvalidMark))?;
            self.bump_line()?;
            let mark = Mark::new(mark).ok_or_else(|| self.err(ErrorKind::ZeroMark))?;
            Ok(Some(mark))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_original_identifier` in fast-import.c.
    fn parse_original_oid(&mut self) -> Result<Option<Span>> {
        if self.eat_prefix(b"original-oid ") {
            let original_oid = self.cursor();
            self.bump_line()?;
            Ok(Some(original_oid))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_and_store_blob` in fast-import.c.
    fn parse_data(&mut self) -> Result<(Span, Option<Span>)> {
        todo!()
    }

    /// Reads a line from input into `self.line_buf`, stripping the LF
    /// delimiter, and skipping any comment lines that start with `#`. Lines may
    /// contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `read_next_command` in fast-import.c.
    fn bump_line(&mut self) -> io::Result<()> {
        while !self.eof {
            self.bump_line_raw()?;
            match self.line_remaining() {
                [b'#', ..] => continue,
                _ => break,
            }
        }
        Ok(())
    }

    /// Reads a line from input into `self.line_buf`, stripping the LF
    /// delimiter. Lines may contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `strbuf_getline_lf` in strbuf.c.
    fn bump_line_raw(&mut self) -> io::Result<()> {
        debug_assert!(!self.eof, "already at EOF");
        let start = self.command_buf.len();
        self.input.read_until(b'\n', &mut self.command_buf)?;
        let mut end = self.command_buf.len();
        if let [.., b'\n'] = &self.command_buf[start..] {
            end -= 1;
        } else {
            // EOF is reached in `read_until` iff the delimiter is not included.
            self.eof = true;
        }
        self.cursor = start..end;
        Ok(())
    }

    // Corresponds to `skip_optional_lf` in fast-import.c.
    fn skip_optional_lf(&mut self) -> io::Result<()> {
        todo!()
    }

    /// Returns the range of text on a line, that is currently being processed.
    #[inline(always)]
    fn cursor(&self) -> Span {
        self.cursor.clone()
    }

    /// Returns the text in the command at the cursor.
    #[inline(always)]
    fn get(&self, range: Range<usize>) -> &[u8] {
        &self.command_buf[range]
    }

    /// Returns the remainder of the line at the cursor.
    #[inline(always)]
    fn line_remaining(&self) -> &[u8] {
        &self.command_buf[self.cursor()]
    }

    /// Consumes text at the cursor on the current line, if it matches the
    /// prefix, and returns whether the cursor was bumped.
    //
    // Corresponds to `skip_prefix` in git-compat-util.c
    #[inline(always)]
    fn eat_prefix(&mut self, prefix: &[u8]) -> bool {
        if self.line_remaining().starts_with(prefix) {
            self.cursor.start += prefix.len();
            true
        } else {
            false
        }
    }

    /// Consumes the remainder of the current line, if it matches the bytes, and
    /// returns whether the cursor was bumped.
    #[inline(always)]
    fn eat_all(&mut self, b: &[u8]) -> bool {
        if self.line_remaining() == b {
            self.cursor.start = self.cursor.end;
            true
        } else {
            false
        }
    }

    /// Creates a parse error at the cursor.
    #[inline(never)]
    fn err(&self, kind: CommandErrorKind) -> ParseError {
        ParseError::Command(CommandError {
            kind,
            line: self.line_remaining().to_owned(),
        })
    }
}

impl<R: BufRead + Clone> Clone for Parser<R> {
    fn clone(&self) -> Self {
        Parser {
            input: self.input.clone(),
            command_buf: self.command_buf.clone(),
            cursor: self.cursor.clone(),
            eof: self.eof,
        }
    }
}

impl<R: BufRead + Debug> Debug for Parser<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parser")
            .field("input", &self.input)
            .field("command_buf", &self.command_buf)
            .field("cursor", &self.cursor)
            .field("eof", &self.eof)
            .finish()
    }
}

impl<R: BufRead + PartialEq> PartialEq for Parser<R> {
    fn eq(&self, other: &Self) -> bool {
        self.input == other.input
            && self.command_buf == other.command_buf
            && self.cursor == other.cursor
            && self.eof == other.eof
    }
}

impl<R: BufRead + Eq> Eq for Parser<R> {}
