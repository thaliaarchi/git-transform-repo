// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    io::{self, BufRead},
    str,
    sync::atomic::AtomicBool,
};

use bstr::ByteSlice;
use thiserror::Error;

use crate::{
    command::{
        Blob, Branch, Command, Commit, Commitish, DataHeader, Done, Encoding, Mark, OriginalOid,
        PersonIdent, Progress,
    },
    parse::{slice, DataReaderError, DataState, Input, PResult, Sliceable, Span},
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
/// open a [`DataReader`](super::DataReader) from the returned [`Blob`](Blob)
/// with [`Blob::open`](Blob::open).
pub struct Parser<R> {
    /// The input reader being parsed.
    ///
    /// `self.input` is mutated in two separate ways: while reading a command
    /// with `Parser::next` or while reading a data stream with a `DataReader`.
    /// `Parser::next` already has exclusive access to the parser, because it
    /// requires `&mut`, and the caller cannot retain any `&`-references during
    /// it. Reading from the `DataReader` happens after `&`-slices of
    /// `self.command_buf` have been returned to the caller, so it uses
    /// `UnsafeCell` to modify `self.input` and `self.data_state`. That is
    /// safely performed by ensuring only a single instance of `DataReader` can
    /// be constructed at a time by guarding its construction with
    /// `Parser::data_opened`.
    pub(super) input: UnsafeCell<Input<R>>,

    /// A buffer containing all of the current command and its sub-commands.
    pub(super) command_buf: Vec<u8>,
    /// The current selection in `self.command_buf`, which is being processed.
    cursor: Span,

    /// A buffer containing the current commit or tag message.
    message_buf: Vec<u8>,

    /// Whether a `DataReader` has been opened for reading. This guards
    /// `Blob::open`, to ensure that only one `DataReader` can be opened per
    /// call to `Parser::next`.
    pub(super) data_opened: AtomicBool,
    /// The state for reading a data stream.
    ///
    /// It may only be mutated under `&` within the `DataReader`.
    pub(super) data_state: UnsafeCell<DataState>,
}

// SAFETY: All `UnsafeCell` fields are guaranteed only be modified by a single
// thread. When mutation occurs under an `&`-reference, it is atomically guarded
// by `Parser::data_opened` to ensure it can only happen by one thread. See the
// invariants of `Parser::input`.
unsafe impl<R> Sync for Parser<R> {}

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

    /// A mark must start with `:`.
    #[error("mark does not start with ':'")]
    MarkMissingColon,
    /// The mark is not a valid integer. fast-import allows more forms of
    /// ill-formatted integers than here.
    #[error("invalid mark")]
    InvalidMark,
    /// fast-import allows `mark :0`, but it 0 is used for when no mark has been
    /// set.
    // TODO: Revisit this after parsing fast-export streams from git fast-export
    // and other tools.
    #[error("cannot use ':0' as a mark")]
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

impl<R: BufRead> Parser<R> {
    /// Creates a new `Parser` for reading the given input.
    #[inline]
    pub fn new(input: R) -> Self {
        Parser {
            input: UnsafeCell::new(Input::new(input)),
            command_buf: Vec::new(),
            cursor: Span::from(0..0),
            message_buf: Vec::new(),
            data_opened: AtomicBool::new(false),
            data_state: UnsafeCell::new(DataState::new()),
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
            self.input
                .get_mut()
                .skip_data(self.data_state.get_mut(), &self.command_buf)?;
        }

        self.command_buf.clear();
        self.bump_command()?;

        if self.input.get_mut().eof() {
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
        let data_header = self.parse_data_header()?;

        let blob = Blob {
            mark,
            original_oid,
            data_header,
            parser: self,
        };
        Ok(Command::Blob(slice(blob, self)))
    }

    // Corresponds to `parse_new_commit` in fast-import.c.
    fn parse_commit(&mut self) -> PResult<Command<'_, &[u8], R>> {
        let branch = self.parse_branch()?;
        let mark = self.parse_mark()?;
        let original_oid = self.parse_original_oid()?;
        let author = self.parse_person_ident(b"author ")?;
        let committer = self
            .parse_person_ident(b"committer ")?
            .ok_or(ParseError::ExpectedCommitter)?;
        let encoding = self.parse_encoding()?;
        self.parse_data_small()?;
        self.bump_command()?;
        let from = self.parse_from()?;
        let merge = self.parse_merge()?;

        let commit = Commit {
            branch,
            mark,
            original_oid,
            author,
            committer,
            encoding,
            message: Span::from(0..0),
            from,
            merge,
            // TODO
        };
        let mut commit = slice(commit, self);
        commit.message = &self.message_buf;
        Ok(Command::Commit(commit))
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
        self.skip_optional_lf();
        Ok(Command::Checkpoint)
    }

    // Corresponds to `parse_alias` in fast-import.c.
    fn parse_alias(&mut self) -> PResult<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_progress` in fast-import.c.
    fn parse_progress(&mut self) -> PResult<Command<'_, &[u8], R>> {
        self.skip_optional_lf();
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

    /// Returns an error when the branch name is invalid.
    ///
    // Corresponds to `lookup_branch` and `new_branch` in fast-import.c.
    fn parse_branch(&mut self) -> PResult<Branch<Span>> {
        let branch = self.cursor;
        if branch.slice(self).contains(&b'\0') {
            return Err(ParseError::BranchContainsNul.into());
        }
        // The git-specific validation of `new_branch` is handled outside by
        // `Branch::validate_git`, so the user can use this for any VCS.
        self.bump_command()?;
        Ok(Branch { branch })
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
        if self.eat_prefix(b"mark ") {
            let mark = self.cursor;
            self.bump_command()?;
            Mark::parse(mark.slice(self)).map(Some)
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_original_identifier` in fast-import.c.
    fn parse_original_oid(&mut self) -> PResult<Option<OriginalOid<Span>>> {
        if self.eat_prefix(b"original-oid ") {
            let original_oid = self.cursor;
            self.bump_command()?;
            Ok(Some(OriginalOid { oid: original_oid }))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_from` in fast-import.c.
    fn parse_from(&mut self) -> PResult<Option<Commitish<Span>>> {
        if self.eat_prefix(b"from ") {
            let commitish = self.cursor;
            self.bump_command()?;
            Commitish::parse(commitish, self).map(Some)
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_merge` in fast-import.c.
    fn parse_merge(&mut self) -> PResult<Vec<Commitish<Span>>> {
        let mut merge = Vec::new();
        while self.eat_prefix(b"merge ") {
            let commitish = self.cursor;
            self.bump_command()?;
            merge.push(Commitish::parse(commitish, self)?);
        }
        Ok(merge)
    }

    // Corresponds to `parse_ident` in fast-import.c.
    fn parse_person_ident(&mut self, prefix: &[u8]) -> PResult<Option<PersonIdent<Span>>> {
        if !self.eat_prefix(prefix) {
            return Ok(None);
        }
        let cursor = self.cursor;
        let ident = cursor.slice(self);
        // NUL may not appear in the name or email due to using `strcspn`.
        if ident.contains(&b'\0') {
            return Err(ParseError::IdentContainsNul.into());
        }

        // TODO: If none of the date formats allow `<` or `>`, I can give better
        // messages like "name contains '>'".
        let Some(lt) = ident.iter().position(|&b| matches!(b, b'<' | b'>')) else {
            return Err(ParseError::IdentNoLtOrGt.into());
        };
        if ident[lt] != b'<' {
            return Err(ParseError::IdentNoLtBeforeGt.into());
        }
        if lt != 0 && ident[lt - 1] != b' ' {
            return Err(ParseError::IdentNoSpaceBeforeLt.into());
        }

        let Some(gt) = ident[lt + 1..]
            .iter()
            .position(|&b| matches!(b, b'<' | b'>'))
        else {
            return Err(ParseError::IdentNoGtAfterLt.into());
        };
        let gt = lt + 1 + gt;
        if ident[gt] != b'>' {
            return Err(ParseError::IdentNoGtAfterLt.into());
        }
        if gt + 1 >= ident.len() || ident[gt + 1] != b' ' {
            return Err(ParseError::IdentNoSpaceAfterGt.into());
        }

        // TODO: Parse dates

        self.bump_command()?;

        let lt = cursor.start + lt;
        let gt = cursor.start + gt;
        Ok(Some(PersonIdent {
            name: Span::from(cursor.start..(lt - 2).max(cursor.start)),
            email: Span::from(lt + 1..gt - 1),
            date: Span::from((gt + 2).min(cursor.end)..cursor.end),
        }))
    }

    // Corresponds to part of `parse_new_commit` in fast-import.c.
    fn parse_encoding(&mut self) -> PResult<Option<Encoding<Span>>> {
        if self.eat_prefix(b"encoding ") {
            let encoding = self.cursor;
            self.bump_command()?;
            Ok(Some(Encoding { encoding }))
        } else {
            Ok(None)
        }
    }

    /// Parses a `data` command, but does not read its contents. git fast-import
    /// reads blobs into memory or switches to streaming when they exceed
    /// `--big-file-threshold` (default 512MiB).
    ///
    // Corresponds to `parse_and_store_blob` in fast-import.c.
    fn parse_data_header(&mut self) -> PResult<DataHeader<Span>> {
        if !self.eat_prefix(b"data ") {
            return Err(ParseError::ExpectedDataCommand.into());
        }
        let header = if self.eat_prefix(b"<<") {
            let delim = self.cursor;
            if delim.is_empty() {
                return Err(ParseError::EmptyDelim.into());
            } else if delim.slice(self).contains(&b'\0') {
                return Err(ParseError::DataDelimContainsNul.into());
            }
            DataHeader::Delimited { delim }
        } else {
            let len = parse_u64(self.line_remaining()).ok_or(ParseError::InvalidDataLength)?;
            DataHeader::Counted { len }
        };
        self.data_state
            .get_mut()
            .set(header.clone(), &mut self.data_opened);
        self.skip_optional_lf();
        Ok(header)
    }

    /// Parses a `data` command and reads its contents into memory. git
    /// fast-import reads commit and tag messages into memory with no size
    /// limit.
    ///
    // Corresponds to `parse_data` in fast-import.c.
    fn parse_data_small(&mut self) -> PResult<usize> {
        let header = self.parse_data_header()?;
        self.message_buf.clear();
        self.input
            .get_mut()
            .read_data_to_end(&mut self.message_buf, header, &self.command_buf)
    }

    /// Reads a line from input into `self.line_buf`, stripping the LF delimiter
    /// and skipping any comment lines that start with `#`. Lines may contain
    /// any bytes (including NUL), except for LF.
    fn bump_command(&mut self) -> io::Result<()> {
        self.input
            .get_mut()
            .read_command(&mut self.command_buf, &mut self.cursor)
    }

    /// Returns the remainder of the line at the cursor.
    #[inline(always)]
    fn line_remaining(&self) -> &[u8] {
        self.cursor.slice(self)
    }

    /// Consumes text at the cursor on the current line, if it matches the
    /// prefix, and returns whether the cursor was bumped.
    ///
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

    /// Skips a trailing LF, if one exists, before reading the following
    /// command.
    #[inline(always)]
    fn skip_optional_lf(&mut self) {
        self.input.get_mut().skip_optional_lf();
    }
}

impl<R: Debug> Debug for Parser<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parser")
            .field("input", &self.input)
            .field("command_buf", &self.command_buf.as_bstr())
            .field("cursor", &self.cursor)
            .field("message_buf", &self.message_buf.as_bstr())
            .field("data_opened", &self.data_opened)
            .field("data_state", &self.data_state)
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
            StreamError::DataReader(err) => io::Error::new(io::ErrorKind::Other, err),
            StreamError::Io(err) => err,
        }
    }
}

impl<'a, R> Sliceable<'a> for Parser<R> {
    #[inline(always)]
    fn as_slice(&'a self) -> &'a [u8] {
        &self.command_buf
    }
}

impl Mark {
    // Corresponds to `parse_mark_ref` in fast-import.c.
    #[inline]
    fn parse(mark: &[u8]) -> PResult<Self> {
        let [b':', mark @ ..] = mark else {
            return Err(ParseError::MarkMissingColon.into());
        };
        let mark = parse_u64(mark).ok_or(ParseError::InvalidMark)?;
        let mark = Mark::new(mark).ok_or(ParseError::ZeroMark)?;
        Ok(mark)
    }
}

impl Commitish<Span> {
    // Corresponds to `parse_objectish` and `parse_merge` in fast-import.c.
    fn parse<R: BufRead>(commitish: Span, parser: &Parser<R>) -> PResult<Self> {
        // TODO: How much of `parse_objectish` should be here or in the
        // front-end?
        let commitish_bytes = commitish.slice(parser);
        if commitish_bytes.starts_with(b":") {
            Mark::parse(commitish_bytes).map(Commitish::Mark)
        } else {
            Ok(Commitish::BranchOrOid(commitish))
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
    // SAFETY: `from_str_radix` operates on byes and accepts only ASCII.
    u64::from_str_radix(unsafe { str::from_utf8_unchecked(b) }, 10).ok()
}
