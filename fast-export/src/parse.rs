use std::{
    fmt::{self, Debug, Formatter},
    io::{self, BufRead},
};

use thiserror::Error;

use crate::command::{Command, Done, Progress};

type Result<T> = std::result::Result<T, ParseError>;

/// Parser for fast-export streams.
pub struct Parser<R: BufRead> {
    input: R,
    line_buf: Vec<u8>,
    eof: bool,
}

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
    #[error("unsupported command")]
    UnsupportedCommand,
}

impl<R: BufRead> Parser<R> {
    /// Parses the next command in the fast-export stream.
    ///
    /// The parsed commands borrow from the parser's buffer, so need to be
    /// copied if they are retained.
    ///
    // Corresponds to the loop in `cmd_fast_import` in fast-import.c.
    pub fn next(&mut self) -> Result<Command<'_>> {
        self.read_line()?;
        if self.eof {
            return Ok(Command::Done(Done::Eof));
        }
        let line = &self.line_buf[..];
        if line == b"blob" {
            self.parse_blob()
        } else if line.starts_with(b"commit ") {
            self.parse_commit()
        } else if line.starts_with(b"tag ") {
            self.parse_tag()
        } else if line.starts_with(b"reset ") {
            self.parse_reset()
        } else if line.starts_with(b"ls ") {
            self.parse_ls()
        } else if line.starts_with(b"cat-blob ") {
            self.parse_cat_blob()
        } else if line.starts_with(b"get-mark ") {
            self.parse_get_mark()
        } else if line == b"checkpoint" {
            self.parse_checkpoint()
        } else if line == b"done" {
            Ok(Command::Done(Done::Explicit))
        } else if line == b"alias" {
            self.parse_alias()
        } else if line.starts_with(b"progress ") {
            self.parse_progress()
        } else if line.starts_with(b"feature ") {
            self.parse_feature()
        } else if line.starts_with(b"option ") {
            self.parse_option()
        } else {
            Err(ParseError::new(CommandErrorKind::UnsupportedCommand, line))
        }
    }

    // Corresponds to `parse_new_blob` in fast-import.c.
    fn parse_blob(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf == b"blob");
        todo!()
    }

    // Corresponds to `parse_new_commit` in fast-import.c.
    fn parse_commit(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf.starts_with(b"commit "));
        let _arg = &self.line_buf[b"commit ".len()..];
        todo!()
    }

    // Corresponds to `parse_new_tag` in fast-import.c.
    fn parse_tag(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf.starts_with(b"tag "));
        let _arg = &self.line_buf[b"tag ".len()..];
        todo!()
    }

    // Corresponds to `parse_reset_branch` in fast-import.c.
    fn parse_reset(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf.starts_with(b"reset "));
        let _arg = &self.line_buf[b"reset ".len()..];
        todo!()
    }

    // Corresponds to `parse_ls` in fast-import.c.
    fn parse_ls(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf.starts_with(b"ls "));
        let _arg = &self.line_buf[b"ls ".len()..];
        todo!()
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf.starts_with(b"cat-blob "));
        let _arg = &self.line_buf[b"cat-blob ".len()..];
        todo!()
    }

    // Corresponds to `parse_get_mark` in fast-import.c.
    fn parse_get_mark(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf.starts_with(b"get-mark "));
        let _arg = &self.line_buf[b"get-mark ".len()..];
        todo!()
    }

    // Corresponds to `parse_checkpoint` in fast-import.c.
    fn parse_checkpoint(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf == b"checkpoint");
        todo!()
    }

    // Corresponds to `parse_alias` in fast-import.c.
    fn parse_alias(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf == b"alias");
        todo!()
    }

    // Corresponds to `parse_progress` in fast-import.c.
    fn parse_progress(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf.starts_with(b"progress "));
        self.skip_optional_lf()?;
        let message = &self.line_buf[b"progress ".len()..];
        Ok(Command::Progress(Progress { message }))
    }

    // Corresponds to `parse_feature` in fast-import.c.
    fn parse_feature(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf.starts_with(b"feature "));
        let _feature = &self.line_buf[b"feature ".len()..];
        todo!()
    }

    // Corresponds to `parse_option` in fast-import.c.
    fn parse_option(&mut self) -> Result<Command<'_>> {
        debug_assert!(self.line_buf.starts_with(b"option "));
        let _option = &self.line_buf[b"option ".len()..];
        todo!()
    }

    /// Reads a line from input into `self.line_buf`, stripping the LF
    /// delimiter, and skipping any comment lines that start with `#`. Lines may
    /// contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `read_next_command` in fast-import.c.
    fn read_line(&mut self) -> io::Result<()> {
        while !self.eof {
            self.read_line_raw()?;
            match &self.line_buf[..] {
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
    fn read_line_raw(&mut self) -> io::Result<()> {
        debug_assert!(!self.eof, "already at EOF");
        self.line_buf.clear();
        self.input.read_until(b'\n', &mut self.line_buf)?;
        if let [.., b'\n'] = &self.line_buf[..] {
            self.line_buf.pop();
        } else {
            // EOF is reached in `read_until` iff the delimiter is not included.
            self.eof = true;
        }
        Ok(())
    }

    // Corresponds to `skip_optional_lf` in fast-import.c.
    fn skip_optional_lf(&mut self) -> io::Result<()> {
        todo!()
    }
}

impl<R: BufRead + Clone> Clone for Parser<R> {
    fn clone(&self) -> Self {
        Parser {
            input: self.input.clone(),
            line_buf: self.line_buf.clone(),
            eof: self.eof,
        }
    }
}

impl<R: BufRead + Debug> Debug for Parser<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parser")
            .field("input", &self.input)
            .field("line_buf", &String::from_utf8_lossy(&self.line_buf))
            .field("eof", &self.eof)
            .finish()
    }
}

impl<R: BufRead + PartialEq> PartialEq for Parser<R> {
    fn eq(&self, other: &Self) -> bool {
        self.input == other.input && self.line_buf == other.line_buf && self.eof == other.eof
    }
}

impl<R: BufRead + Eq> Eq for Parser<R> {}

impl ParseError {
    #[inline]
    fn new(kind: CommandErrorKind, line: &[u8]) -> Self {
        ParseError::Command(CommandError::new(kind, line))
    }
}

impl CommandError {
    #[inline]
    fn new(kind: CommandErrorKind, line: &[u8]) -> Self {
        CommandError {
            kind,
            line: line.to_owned(),
        }
    }
}
