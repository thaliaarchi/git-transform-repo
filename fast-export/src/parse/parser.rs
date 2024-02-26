use std::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    io::{self, BufRead},
    ops::Range,
    str,
    sync::atomic::{AtomicBool, Ordering},
};

use bstr::ByteSlice;
use thiserror::Error;

use crate::{
    command::{Blob, Command, Done, Mark, OriginalOid, Progress},
    parse::{DataSpan, DataState, Result},
};

/// A zero-copy pull parser for fast-export streams.
///
/// It uses only as much memory as the single largest command command in the
/// stream. Any references to parsed bytes returned by the parser are
/// invalidated when [`Parser::next`] is called and must be first copied in
/// order to retain them. Returned references can safely be used by multiple
/// threads, to be processed in parallel.
///
/// Commands are parsed separately from data streams. To read a data stream,
/// open a [`DataReader`](super::DataReader) from the returned [`DataStream`](super::DataStream)
/// with [`DataStream::open`](super::DataStream::open).
pub struct Parser<R> {
    /// The input reader being parsed.
    ///
    /// `input` is mutated in two separate ways: while reading a command with
    /// `Parser::next` or while reading a data stream with a `DataReader`.
    /// `Parser::next` already has exclusive access to the parser, because it
    /// requires `&mut`, and the caller cannot retain any `&`-references during
    /// it. Reading from the `DataReader` happens after `&`-slices of
    /// `command_buf` have been returned to the caller, so it uses `UnsafeCell`
    /// to modify `input` and `data_state`. That is safely performed by ensuring
    /// only a single instance of `DataReader` can be constructed at a time by
    /// guarding its construction with `Parser::data_opened`.
    pub(super) input: UnsafeCell<Input<R>>,

    /// A buffer containing all of the current command and its sub-commands.
    pub(super) command_buf: Vec<u8>,
    /// The current selection in `command_buf`, which is being processed.
    cursor: Span,

    /// Whether a `DataReader` has been opened for reading. This guards
    /// `DataStream::open`, to ensure that only one `DataReader` can be opened
    /// per call to `Parser::next`.
    pub(super) data_opened: AtomicBool,
    /// The state for reading a data stream.
    ///
    /// It may only be mutated under `&` within the `DataReader`.
    pub(super) data_state: UnsafeCell<DataState>,

    /// Whether the previous command ended with an optional LF.
    has_optional_lf: bool,
}

// SAFETY: All `UnsafeCell` fields are guaranteed only be modified by a single
// thread. When mutation occurs under an `&`-reference, it is atomically guarded
// by `Parser::data_opened` to ensure it can only happen by one thread. See the
// invariants of `Parser::input`.
unsafe impl<R> Sync for Parser<R> {}

/// A range of bytes within `command_buf`.
///
/// This is used instead of directly slicing `command_buf` so that ranges can be
/// safely saved while the buffer is still being grown. After the full command
/// has been read (except for a data stream, which is read separately),
/// `command_buf` will not change until the next call to `next`, and slices can
/// be made and returned to the caller.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) struct Span {
    start: usize,
    end: usize,
}

/// Input for a fast-export stream.
pub(super) struct Input<R> {
    /// Reader for the fast-export stream.
    pub(super) r: R,
    /// Whether the reader has reached EOF.
    pub(super) eof: bool,
}

/// An error from parsing a fast-export stream, including IO errors.
#[derive(Debug, Error)]
#[error(transparent)]
pub enum StreamError {
    Parse(#[from] ParseError),
    Io(#[from] io::Error),
}

/// An error from parsing a fast-export stream.
#[derive(Clone, Error, PartialEq, Eq, Hash)]
#[error("{kind}: {:?}", line.as_bstr())]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub line: Vec<u8>,
}

/// A kind of error from parsing a command in a fast-export stream.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Hash)]
pub enum ParseErrorKind {
    /// The mark is not a valid integer. fast-import allows more forms of
    /// ill-formatted integers than here.
    #[error("invalid mark")]
    InvalidMark,
    /// fast-import allows `mark :0`, but it 0 is used for when no mark has been
    /// set.
    // TODO: Revisit this after parsing fast-export streams from git fast-export
    // and other tools.
    #[error("cannot use :0 as a mark")]
    ZeroMark,

    /// A `data` command is required here.
    #[error("expected 'data' command")]
    ExpectedDataCommand,
    /// The data stream was not read to completion by [`DataReader`](super::DataReader)
    /// before the next command was parsed. If you want to close it early, call
    /// [`DataReader::skip_rest`](super::DataReader::skip_rest).
    #[error("data stream was not read to the end")]
    UnfinishedData,
    /// The data reader has already been closed by [`DataReader::close`](super::DataReader::close).
    #[error("data reader is closed")]
    ClosedData,
    /// The length for a counted `data` command is not a valid integer.
    #[error("invalid data length")]
    InvalidDataLength,
    /// EOF was reached before reading the complete counted data stream.
    #[error("unexpected EOF in data stream")]
    DataUnexpectedEof,
    /// fast-import accepts opening, but not closing, delimiters that contain
    /// NUL, so it will never terminate such data. This error detects that
    /// early.
    #[error("data delimiter contains NUL ('\\0')")]
    DataDelimContainsNul,
    /// fast-import accepts an empty delimiter, but receiving that is most
    /// likely an error, so we reject it.
    #[error("data delimiter is empty")]
    EmptyDelim,
    /// EOF was reached before encountering the data delimiter.
    #[error("unterminated delimited data stream")]
    UnterminatedData,

    /// The command is not recognized.
    #[error("unsupported command")]
    UnsupportedCommand,
    /// Unexpected blank line instead of a command.
    #[error("unexpected blank line")]
    UnexpectedBlank,
}

use ParseErrorKind as ErrorKind;

impl<R: BufRead> Parser<R> {
    /// Creates a new `Parser` for reading the given input.
    #[inline]
    pub fn new(input: R) -> Self {
        Parser {
            input: UnsafeCell::new(Input {
                r: input,
                eof: false,
            }),
            command_buf: Vec::new(),
            cursor: Span::from(0..0),
            data_opened: AtomicBool::new(false),
            data_state: UnsafeCell::new(DataState::new()),
            has_optional_lf: false,
        }
    }

    /// Parses the next command in the fast-export stream.
    ///
    /// The parsed commands borrow from the parser's buffer, so need to be
    /// copied before calling `next` again to retain them.
    ///
    // Corresponds to the loop in `cmd_fast_import` in fast-import.c.
    pub fn next(&mut self) -> Result<Command<'_, &[u8], R>> {
        // Finish reading the previous data stream, if the user didn't.
        if !self.data_state.get_mut().finished() {
            self.skip_data()?;
        }

        self.command_buf.clear();
        self.bump_command()?;

        // Consume an optional trailing LF from the previous command.
        if self.has_optional_lf {
            self.has_optional_lf = false;
            if self.line_remaining().is_empty() {
                self.command_buf.clear();
                self.bump_command()?;
            }
        }

        if self.input.get_mut().eof {
            Ok(Command::Done(Done::Eof))
        } else if self.eat_if_equals(b"blob") {
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
        } else if self.eat_if_equals(b"checkpoint") {
            self.parse_checkpoint()
        } else if self.eat_if_equals(b"done") {
            Ok(Command::Done(Done::Explicit))
        } else if self.eat_if_equals(b"alias") {
            self.parse_alias()
        } else if self.eat_prefix(b"progress ") {
            self.parse_progress()
        } else if self.eat_prefix(b"feature ") {
            self.parse_feature()
        } else if self.eat_prefix(b"option ") {
            self.parse_option()
        } else if self.line_remaining().is_empty() {
            Err(self.err(ErrorKind::UnexpectedBlank))
        } else {
            Err(self.err(ErrorKind::UnsupportedCommand))
        }
    }

    // Corresponds to `parse_new_blob` in fast-import.c.
    fn parse_blob(&mut self) -> Result<Command<'_, &[u8], R>> {
        self.bump_command()?;
        let mark = self.parse_mark()?;
        let original_oid = self.parse_original_oid()?;
        let data = self.parse_data()?;

        Ok(Command::Blob(Blob {
            mark,
            original_oid: original_oid.map(|oid| OriginalOid {
                oid: self.slice_cmd(oid),
            }),
            data: data.slice(self),
        }))
    }

    // Corresponds to `parse_new_commit` in fast-import.c.
    fn parse_commit(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_new_tag` in fast-import.c.
    fn parse_tag(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_reset_branch` in fast-import.c.
    fn parse_reset(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_ls` in fast-import.c.
    fn parse_ls(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_get_mark` in fast-import.c.
    fn parse_get_mark(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_checkpoint` in fast-import.c.
    fn parse_checkpoint(&mut self) -> Result<Command<'_, &[u8], R>> {
        self.has_optional_lf = true;
        Ok(Command::Checkpoint)
    }

    // Corresponds to `parse_alias` in fast-import.c.
    fn parse_alias(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_progress` in fast-import.c.
    fn parse_progress(&mut self) -> Result<Command<'_, &[u8], R>> {
        self.has_optional_lf = true;
        Ok(Command::Progress(Progress {
            message: self.line_remaining(),
        }))
    }

    // Corresponds to `parse_feature` in fast-import.c.
    fn parse_feature(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_option` in fast-import.c.
    fn parse_option(&mut self) -> Result<Command<'_, &[u8], R>> {
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
            let mark =
                parse_u64(self.line_remaining()).ok_or_else(|| self.err(ErrorKind::InvalidMark))?;
            self.bump_command()?;
            let mark = Mark::new(mark).ok_or_else(|| self.err(ErrorKind::ZeroMark))?;
            Ok(Some(mark))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_original_identifier` in fast-import.c.
    fn parse_original_oid(&mut self) -> Result<Option<Span>> {
        if self.eat_prefix(b"original-oid ") {
            let original_oid = self.cursor;
            self.bump_command()?;
            Ok(Some(original_oid))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_and_store_blob` in fast-import.c.
    fn parse_data(&mut self) -> Result<DataSpan> {
        if !self.eat_prefix(b"data ") {
            return Err(self.err(ErrorKind::ExpectedDataCommand));
        }
        let header = if self.eat_prefix(b"<<") {
            let delim = self.cursor;
            if delim.is_empty() {
                return Err(self.err(ErrorKind::EmptyDelim));
            } else if self.slice_cmd(delim).contains(&b'\0') {
                return Err(self.err(ErrorKind::DataDelimContainsNul));
            }
            DataSpan::Delimited { delim }
        } else {
            let len = parse_u64(self.line_remaining())
                .ok_or_else(|| self.err(ErrorKind::InvalidDataLength))?;
            DataSpan::Counted { len }
        };
        self.data_state.get_mut().set(header, &mut self.data_opened);
        self.has_optional_lf = true;
        Ok(header)
    }

    /// Reads a line from input into `self.line_buf`, stripping the LF
    /// delimiter, and skipping any comment lines that start with `#`. Lines may
    /// contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `read_next_command` in fast-import.c.
    fn bump_command(&mut self) -> io::Result<()> {
        if self.input.get_mut().eof {
            self.cursor.start = self.cursor.end;
            return Ok(());
        }
        loop {
            self.cursor = self.input.get_mut().read_line(&mut self.command_buf)?;
            if !self.slice_cmd(self.cursor).starts_with(b"#") || self.input.get_mut().eof {
                break;
            }
        }
        Ok(())
    }

    /// Borrows a range of the command.
    #[inline(always)]
    fn slice_cmd(&self, span: Span) -> &[u8] {
        &self.command_buf[Range::from(span)]
    }

    /// Returns the remainder of the line at the cursor.
    #[inline(always)]
    fn line_remaining(&self) -> &[u8] {
        self.slice_cmd(self.cursor)
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
    fn eat_if_equals(&mut self, b: &[u8]) -> bool {
        if self.line_remaining() == b {
            self.cursor.start = self.cursor.end;
            true
        } else {
            false
        }
    }

    /// Creates a parse error at the cursor.
    #[inline(never)]
    pub(super) fn err(&self, kind: ParseErrorKind) -> StreamError {
        // TODO: Improve error reporting:
        // - Use an error reporting library like miette or Ariadne.
        // - Track line in data stream.
        // - Would it be useful to include the error range for `io::Error`s from
        //   `self.input.r`? Such errors seem unlikely to be related to the
        //   syntax or semantics of the data. It could possibly be tracked by
        //   recording the size of the buffer with `fill_buf` before calling
        //   `read`.
        let line = if self.data_opened.load(Ordering::Acquire) {
            b"<<parsing data stream>>".to_vec()
        } else {
            self.line_remaining().to_owned()
        };
        StreamError::Parse(ParseError { kind, line })
    }
}

impl<R: Debug> Debug for Parser<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parser")
            .field("input", &self.input)
            .field("command_buf", &self.command_buf.as_bstr())
            .field("cursor", &self.cursor)
            .field("data_opened", &self.data_opened)
            .field("data_state", &self.data_state)
            .field("has_optional_lf", &self.has_optional_lf)
            .finish()
    }
}

impl Span {
    #[inline(always)]
    pub(super) fn slice<'a, B: AsRef<[u8]>>(&self, bytes: &'a B) -> &'a [u8] {
        &bytes.as_ref()[Range::from(*self)]
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        !(self.start < self.end)
    }
}

impl From<Range<usize>> for Span {
    #[inline(always)]
    fn from(range: Range<usize>) -> Self {
        Span {
            start: range.start,
            end: range.end,
        }
    }
}

impl From<Span> for Range<usize> {
    #[inline(always)]
    fn from(span: Span) -> Self {
        span.start..span.end
    }
}

impl Debug for Span {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

impl<R: BufRead> Input<R> {
    /// Reads a line from `input` into `buf`, stripping the LF delimiter. Lines
    /// may contain any bytes (including NUL), except for LF.
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
        Ok(Span::from(start..end))
    }
}

impl Debug for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParseError")
            .field("kind", &self.kind)
            .field("line", &self.line.as_bstr())
            .finish()
    }
}

impl From<io::ErrorKind> for StreamError {
    #[inline]
    fn from(kind: io::ErrorKind) -> Self {
        StreamError::Io(kind.into())
    }
}

impl From<StreamError> for io::Error {
    #[inline]
    fn from(err: StreamError) -> Self {
        match err {
            StreamError::Parse(err) => io::Error::new(io::ErrorKind::InvalidData, err),
            StreamError::Io(err) => err,
        }
    }
}

#[inline]
fn parse_u64(b: &[u8]) -> Option<u64> {
    // TODO: Make an integer parsing routine, to precisely control the grammar
    // and protect from upstream changes.
    if b.starts_with(b"+") {
        return None;
    }
    // SAFETY: from_str_radix operates on byes and accepts only ASCII.
    u64::from_str_radix(unsafe { str::from_utf8_unchecked(b) }, 10).ok()
}
