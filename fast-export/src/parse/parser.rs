// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    io::{self, BufRead},
    ops::Range,
    str,
    sync::atomic::AtomicBool,
};

use bstr::ByteSlice;
use thiserror::Error;

use crate::{
    command::{
        Blob, Branch, Command, Commit, DataHeader, Done, Encoding, Mark, OriginalOid, PersonIdent,
        Progress,
    },
    parse::{DataReaderError, DataState, DataStream, PResult},
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

/// Input for a fast-export stream.
pub(super) struct Input<R> {
    /// Reader for the fast-export stream.
    pub(super) r: R,
    /// Whether the reader has reached EOF.
    pub(super) eof: bool,
    /// The current line number.
    pub(super) line: u64,
}

/// An error from parsing a fast-export stream, including IO errors.
#[derive(Debug, Error)]
#[error(transparent)]
pub enum StreamError {
    Parse(#[from] ParseError),
    DataReader(#[from] DataReaderError),
    Io(#[from] io::Error),
}

/// A kind of error from parsing a command in a fast-export stream.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Hash)]
pub enum ParseError {
    /// The branch name contains NUL. fast-import accepts such branch names, but
    /// silently truncates them to the first NUL.
    #[error("branch name contains NUL")]
    BranchContainsNul,
    /// The person identifier contains NUL. fast-import does not read text after
    /// NUL in such commands.
    #[error("person identifier contains NUL")]
    IdentContainsNul,
    #[error("person identifier does not have '<' or '>'")]
    IdentNoLtOrGt,
    #[error("person identifier does not have '<' before '>'")]
    IdentNoLtBeforeGt,
    #[error("person identifier does not have '>' after '<'")]
    IdentNoGtAfterLt,
    #[error("person identifier does not have ' ' before '<'")]
    IdentNoSpaceBeforeLt,
    #[error("person identifier does not have ' ' after '>'")]
    IdentNoSpaceAfterGt,
    /// A `committer` command is required in a commit.
    #[error("expected committer in commit")]
    ExpectedCommitter,

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
    /// The length for a counted `data` command is not a valid integer.
    #[error("invalid data length")]
    InvalidDataLength,
    /// EOF was reached before reading the complete counted data stream.
    #[error("unexpected EOF in data stream")]
    DataUnexpectedEof,
    /// fast-import accepts opening, but not closing, delimiters that contain
    /// NUL, so it will never terminate such data. This error detects that
    /// early.
    #[error("data delimiter contains NUL")]
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

/// Spanned version of [`DataHeader`].
#[derive(Clone, Copy, Debug)]
pub(super) enum DataSpan {
    Counted { len: u64 },
    Delimited { delim: Span },
}

/// Spanned version of [`PersonIdent`].
struct PersonIdentSpan {
    name: Span,
    email: Span,
    // TODO: Parse dates
    date: Span,
}

impl<R: BufRead> Parser<R> {
    /// Creates a new `Parser` for reading the given input.
    #[inline]
    pub fn new(input: R) -> Self {
        Parser {
            input: UnsafeCell::new(Input {
                r: input,
                eof: false,
                line: 0,
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
    pub fn next(&mut self) -> PResult<Command<'_, &[u8], R>> {
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
            Err(ParseError::UnexpectedBlank.into())
        } else {
            Err(ParseError::UnsupportedCommand.into())
        }
    }

    // Corresponds to `parse_new_blob` in fast-import.c.
    fn parse_blob(&mut self) -> PResult<Command<'_, &[u8], R>> {
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
    fn parse_commit(&mut self) -> PResult<Command<'_, &[u8], R>> {
        let branch = self.cursor;
        self.validate_branch(branch)?;
        self.bump_command()?;
        let mark = self.parse_mark()?;
        let original_oid = self.parse_original_oid()?;
        let author = self.parse_person_ident(b"author ")?;
        let committer = self
            .parse_person_ident(b"committer ")?
            .ok_or(ParseError::ExpectedCommitter)?;
        let encoding = self.parse_encoding()?;
        // fast-import reads the message into memory with no size limit, unlike
        // blobs, which switch to streaming when it exceeds --big-file-threshold
        // (default 512 * 1024 * 1024).
        let message = self.parse_data()?;
        // TODO

        Ok(Command::Commit(Commit {
            branch: Branch {
                branch: self.slice_cmd(branch),
            },
            mark,
            original_oid: original_oid.map(|oid| OriginalOid {
                oid: self.slice_cmd(oid),
            }),
            author: author.map(|author| author.slice(self)),
            committer: committer.slice(self),
            encoding: encoding.map(|encoding| Encoding {
                encoding: self.slice_cmd(encoding),
            }),
            message: message.slice(self),
            // TODO
        }))
    }

    // Corresponds to `parse_new_tag` in fast-import.c.
    fn parse_tag(&mut self) -> PResult<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_reset_branch` in fast-import.c.
    fn parse_reset(&mut self) -> PResult<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_ls` in fast-import.c.
    fn parse_ls(&mut self) -> PResult<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob(&mut self) -> PResult<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_get_mark` in fast-import.c.
    fn parse_get_mark(&mut self) -> PResult<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_checkpoint` in fast-import.c.
    fn parse_checkpoint(&mut self) -> PResult<Command<'_, &[u8], R>> {
        self.has_optional_lf = true;
        Ok(Command::Checkpoint)
    }

    // Corresponds to `parse_alias` in fast-import.c.
    fn parse_alias(&mut self) -> PResult<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_progress` in fast-import.c.
    fn parse_progress(&mut self) -> PResult<Command<'_, &[u8], R>> {
        self.has_optional_lf = true;
        Ok(Command::Progress(Progress {
            message: self.line_remaining(),
        }))
    }

    // Corresponds to `parse_feature` in fast-import.c.
    fn parse_feature(&mut self) -> PResult<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_option` in fast-import.c.
    fn parse_option(&mut self) -> PResult<Command<'_, &[u8], R>> {
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
    fn parse_mark(&mut self) -> PResult<Option<Mark>> {
        if self.eat_prefix(b"mark :") {
            let mark = parse_u64(self.line_remaining()).ok_or(ParseError::InvalidMark)?;
            self.bump_command()?;
            let mark = Mark::new(mark).ok_or(ParseError::ZeroMark)?;
            Ok(Some(mark))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_original_identifier` in fast-import.c.
    fn parse_original_oid(&mut self) -> PResult<Option<Span>> {
        if self.eat_prefix(b"original-oid ") {
            let original_oid = self.cursor;
            self.bump_command()?;
            Ok(Some(original_oid))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_ident` in fast-export.c.
    fn parse_person_ident(&mut self, prefix: &[u8]) -> PResult<Option<PersonIdentSpan>> {
        if !self.eat_prefix(prefix) {
            return Ok(None);
        }
        let ident = self.cursor;
        // NUL may not appear in the name or email due to using `strcspn`.
        if self.command_buf[Range::from(ident)].contains(&b'\0') {
            return Err(ParseError::IdentContainsNul.into());
        }

        // TODO: If none of the date formats allow `<` or `>`, I can give better
        // messages like "name contains '>'".
        let Some(lt) = self.command_buf[Range::from(ident)]
            .iter()
            .position(|&b| matches!(b, b'<' | b'>'))
        else {
            return Err(ParseError::IdentNoLtOrGt.into());
        };
        let lt = ident.start + lt;
        if self.command_buf[lt] != b'<' {
            return Err(ParseError::IdentNoLtBeforeGt.into());
        }
        if lt > ident.start && self.command_buf[lt - 1] != b' ' {
            return Err(ParseError::IdentNoSpaceBeforeLt.into());
        }

        let Some(gt) = self.command_buf[lt + 1..ident.end]
            .iter()
            .position(|&b| matches!(b, b'<' | b'>'))
        else {
            return Err(ParseError::IdentNoGtAfterLt.into());
        };
        let gt = lt + 1 + gt;
        if self.command_buf[gt] != b'>' {
            return Err(ParseError::IdentNoGtAfterLt.into());
        }
        if gt + 1 < ident.end && self.command_buf[gt + 1] != b' ' {
            return Err(ParseError::IdentNoSpaceAfterGt.into());
        }

        // TODO: Parse dates

        self.bump_command()?;

        Ok(Some(PersonIdentSpan {
            name: Span::from(ident.start..(lt - 2).max(ident.start)),
            email: Span::from(lt + 1..gt - 1),
            date: Span::from((gt + 2).min(ident.end)..ident.end),
        }))
    }

    // Corresponds to part of `parse_new_commit` in fast-import.c.
    fn parse_encoding(&mut self) -> PResult<Option<Span>> {
        if self.eat_prefix(b"encoding ") {
            let encoding = self.cursor;
            self.bump_command()?;
            Ok(Some(encoding))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_and_store_blob` in fast-import.c.
    fn parse_data(&mut self) -> PResult<DataSpan> {
        if !self.eat_prefix(b"data ") {
            return Err(ParseError::ExpectedDataCommand.into());
        }
        let header = if self.eat_prefix(b"<<") {
            let delim = self.cursor;
            if delim.is_empty() {
                return Err(ParseError::EmptyDelim.into());
            } else if self.slice_cmd(delim).contains(&b'\0') {
                return Err(ParseError::DataDelimContainsNul.into());
            }
            DataSpan::Delimited { delim }
        } else {
            let len = parse_u64(self.line_remaining()).ok_or(ParseError::InvalidDataLength)?;
            DataSpan::Counted { len }
        };
        self.data_state.get_mut().set(header, &mut self.data_opened);
        self.has_optional_lf = true;
        Ok(header)
    }

    /// Returns an error when the branch name is invalid.
    ///
    // Corresponds to `lookup_branch` and `new_branch` in fast-import.c.
    #[inline]
    fn validate_branch(&self, branch: Span) -> PResult<()> {
        if self.slice_cmd(branch).contains(&b'\0') {
            return Err(ParseError::BranchContainsNul.into());
        }
        // The git-specific validation of `new_branch` is handled outside by
        // `Branch::validate_git`, so the user can use this for any VCS.
        Ok(())
    }

    /// Reads a line from input into `self.line_buf`, stripping the LF
    /// delimiter, and skipping any comment lines that start with `#`. Lines may
    /// contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `read_next_command` in fast-import.c.
    fn bump_command(&mut self) -> io::Result<()> {
        loop {
            if self.input.get_mut().eof {
                self.cursor.start = self.cursor.end;
                break;
            }
            self.cursor = self.input.get_mut().read_line(&mut self.command_buf)?;
            if !self.slice_cmd(self.cursor).starts_with(b"#") {
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
        self.line += 1;
        Ok(Span::from(start..end))
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
            StreamError::DataReader(err) => io::Error::new(io::ErrorKind::Other, err),
            StreamError::Io(err) => err,
        }
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

impl DataSpan {
    #[inline(always)]
    fn slice<'a, R: BufRead>(&self, parser: &'a Parser<R>) -> DataStream<'a, &'a [u8], R> {
        DataStream {
            header: match *self {
                DataSpan::Counted { len } => DataHeader::Counted { len },
                DataSpan::Delimited { delim } => DataHeader::Delimited {
                    delim: parser.slice_cmd(delim),
                },
            },
            parser,
        }
    }
}

impl PersonIdentSpan {
    #[inline(always)]
    fn slice<'a, R: BufRead>(&self, parser: &'a Parser<R>) -> PersonIdent<&'a [u8]> {
        PersonIdent {
            name: parser.slice_cmd(self.name),
            email: parser.slice_cmd(self.email),
            date: parser.slice_cmd(self.date),
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
