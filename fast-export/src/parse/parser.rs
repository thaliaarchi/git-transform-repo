// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    cell::UnsafeCell,
    io::{self, BufRead},
    str,
    sync::atomic::{AtomicBool, Ordering},
};

use thiserror::Error;

use crate::{
    command::{
        Alias, Blob, Branch, Command, Commit, Commitish, DataHeader, Done, Encoding, Mark,
        Objectish, OriginalOid, PersonIdent, Progress, Reset, Tag, TagName,
    },
    parse::{BufInput, DataReaderError, DataState, DirectiveParser, PResult},
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
    pub(super) input: BufInput<R>,

    /// A buffer containing the current commit or tag message.
    message_buf: UnsafeCell<Vec<u8>>,

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

    #[error("tag name contains NUL")]
    TagContainsNul,

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

    /// The length for a counted `data` directive is not a valid integer.
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

    #[error("expected 'data' directive in blob")]
    ExpectedBlobData,
    #[error("expected committer in commit")]
    ExpectedCommitCommitter,
    #[error("expected message in commit")]
    ExpectedCommitMessage,
    #[error("expected 'from' directive in tag")]
    ExpectedTagFrom,
    #[error("expected message in tag")]
    ExpectedTagMessage,
    #[error("expected 'mark' directive in alias")]
    ExpectedAliasMark,
    #[error("expected 'to' directive in alias")]
    ExpectedAliasTo,

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
            input: BufInput::new(input),
            message_buf: UnsafeCell::new(Vec::new()),
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
        // Read the previous data stream, if the user didn't. Error if the user
        // only partially read the data stream.
        if !self.data_state.get_mut().finished() {
            if self.data_opened.load(Ordering::Acquire) {
                return Err(DataReaderError::Unfinished.into());
            }
            self.input.skip_data(self.data_state.get_mut())?;
        }

        self.input.truncate_context();
        let Some(line) = self.input.next_directive()? else {
            return Ok(Command::Done(Done::Eof));
        };

        if line == b"blob" {
            self.parse_blob()
        } else if let Some(branch) = line.strip_prefix(b"commit ") {
            self.parse_commit(branch)
        } else if let Some(name) = line.strip_prefix(b"tag ") {
            self.parse_tag(name)
        } else if let Some(branch) = line.strip_prefix(b"reset ") {
            self.parse_reset(branch)
        } else if let Some(args) = line.strip_prefix(b"ls ") {
            self.parse_ls(args)
        } else if let Some(data_ref) = line.strip_prefix(b"cat-blob ") {
            self.parse_cat_blob(data_ref)
        } else if let Some(mark) = line.strip_prefix(b"get-mark ") {
            self.parse_get_mark(mark)
        } else if line == b"checkpoint" {
            self.parse_checkpoint()
        } else if line == b"done" {
            Ok(Command::Done(Done::Explicit))
        } else if line == b"alias" {
            self.parse_alias()
        } else if let Some(message) = line.strip_prefix(b"progress ") {
            self.parse_progress(message)
        } else if let Some(feature) = line.strip_prefix(b"feature ") {
            self.parse_feature(feature)
        } else if let Some(option) = line.strip_prefix(b"option ") {
            self.parse_option(option)
        } else if line == b"" {
            Err(ParseError::UnexpectedBlank.into())
        } else {
            Err(ParseError::UnsupportedCommand.into())
        }
    }

    // Corresponds to `parse_new_blob` in fast-import.c.
    fn parse_blob(&self) -> PResult<Command<'_, &[u8], R>> {
        let mark = self.parse_directive(b"mark ", Mark::parse)?;
        let original_oid = self.parse_directive(b"original-oid ", OriginalOid::parse)?;
        let data_header = self
            .parse_directive(b"data ", DataHeader::parse)?
            .ok_or(ParseError::ExpectedBlobData)?;

        let data_state = unsafe { &mut *self.data_state.get() };
        data_state.init(&data_header, &self.data_opened);

        Ok(Command::Blob(Blob {
            mark,
            original_oid,
            data_header,
            parser: self,
        }))
    }

    // Corresponds to `parse_new_commit` in fast-import.c.
    fn parse_commit<'a>(&'a self, branch: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        let branch = Branch::parse(branch)?;
        let mark = self.parse_directive(b"mark ", Mark::parse)?;
        let original_oid = self.parse_directive(b"original-oid ", OriginalOid::parse)?;
        let author = self.parse_directive(b"author ", PersonIdent::parse)?;
        let committer = self
            .parse_directive(b"committer ", PersonIdent::parse)?
            .ok_or(ParseError::ExpectedCommitCommitter)?;
        let encoding = self.parse_directive(b"encoding ", Encoding::parse)?;
        let message = self
            .parse_data_small()?
            .ok_or(ParseError::ExpectedCommitMessage)?;
        let from = self.parse_directive(b"from ", Commitish::parse)?;
        let merge = self.parse_directive_many(b"merge ", Commitish::parse)?;

        Ok(Command::Commit(Commit {
            branch,
            mark,
            original_oid,
            author,
            committer,
            encoding,
            message,
            from,
            merge,
            // TODO
        }))
    }

    // Corresponds to `parse_new_tag` in fast-import.c.
    fn parse_tag<'a>(&'a self, name: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        let name = TagName::parse(name)?;
        let mark = self.parse_directive(b"mark ", Mark::parse)?;
        let from = self
            .parse_directive(b"from ", Objectish::parse)?
            .ok_or(ParseError::ExpectedTagFrom)?;
        let original_oid = self.parse_directive(b"original-oid ", OriginalOid::parse)?;
        // TODO: `tagger` is optional in fast-import.c, but required in the
        // fast-import docs.
        let tagger = self.parse_directive(b"tagger ", PersonIdent::parse)?;
        let message = self
            .parse_data_small()?
            .ok_or(ParseError::ExpectedTagMessage)?;

        Ok(Command::Tag(Tag {
            name,
            mark,
            from,
            original_oid,
            tagger,
            message,
        }))
    }

    // Corresponds to `parse_reset_branch` in fast-import.c.
    fn parse_reset<'a>(&'a self, branch: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        // TODO: Handle deletions and ref namespaces.
        let branch = Branch::parse(branch)?;
        let from = self.parse_directive(b"from ", Commitish::parse)?;
        // TODO: fast-import docs include an optional LF, but fast-import.c
        // doesn't seem to.

        Ok(Command::Reset(Reset { branch, from }))
    }

    // Corresponds to `parse_ls` in fast-import.c.
    fn parse_ls<'a>(&'a self, _args: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        todo!()
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob<'a>(&'a self, _data_ref: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        todo!()
    }

    // Corresponds to `parse_get_mark` in fast-import.c.
    fn parse_get_mark<'a>(&'a self, _mark: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        todo!()
    }

    // Corresponds to `parse_checkpoint` in fast-import.c.
    fn parse_checkpoint(&self) -> PResult<Command<'_, &[u8], R>> {
        self.input.skip_optional_lf()?;
        Ok(Command::Checkpoint)
    }

    // Corresponds to `parse_alias` in fast-import.c.
    fn parse_alias(&self) -> PResult<Command<'_, &[u8], R>> {
        // TODO: This optional LF is at the start of the command in
        // fast-import.c, but at the end in the fast-import docs.
        self.input.skip_optional_lf()?;
        let mark = self
            .parse_directive(b"mark ", Mark::parse)?
            .ok_or(ParseError::ExpectedAliasMark)?;
        let to = self
            .parse_directive(b"to ", Commitish::parse)?
            .ok_or(ParseError::ExpectedAliasTo)?;

        Ok(Command::Alias(Alias { mark, to }))
    }

    // Corresponds to `parse_progress` in fast-import.c.
    fn parse_progress<'a>(&'a self, message: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        self.input.skip_optional_lf()?;
        Ok(Command::Progress(Progress { message }))
    }

    // Corresponds to `parse_feature` in fast-import.c.
    fn parse_feature<'a>(&'a self, _feature: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        todo!()
    }

    // Corresponds to `parse_option` in fast-import.c.
    fn parse_option<'a>(&'a self, _option: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        todo!()
    }

    /// Parses a `data` directive and reads its contents into memory. git
    /// fast-import reads commit and tag messages into memory with no size
    /// limit.
    ///
    // Corresponds to `parse_data` in fast-import.c.
    fn parse_data_small(&self) -> PResult<Option<&[u8]>> {
        let Some(header) = self.parse_directive(b"data ", DataHeader::parse)? else {
            return Ok(None);
        };
        let message_buf = unsafe { &mut *self.message_buf.get() };
        message_buf.clear();
        self.input.read_data_to_end(header, message_buf)?;
        Ok(Some(message_buf))
    }
}

impl<R: BufRead> DirectiveParser<R> for Parser<R> {
    #[inline(always)]
    fn input(&self) -> &BufInput<R> {
        &self.input
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

impl<'a> Branch<&'a [u8]> {
    // Corresponds to `lookup_branch` and `new_branch` in fast-import.c.
    fn parse(branch: &'a [u8]) -> PResult<Self> {
        if branch.contains(&b'\0') {
            return Err(ParseError::BranchContainsNul.into());
        }
        // The git-specific validation of `new_branch` is handled outside by
        // `Branch::validate_git`, so the user can use this for any VCS.
        Ok(Branch { branch })
    }
}

impl<'a> TagName<&'a [u8]> {
    // Corresponds to part of `parse_new_tag` in fast-import.c.
    fn parse(name: &'a [u8]) -> PResult<Self> {
        if name.contains(&b'\0') {
            return Err(ParseError::TagContainsNul.into());
        }
        Ok(TagName { name })
    }
}

impl Mark {
    /// # Differences from fast-import
    ///
    /// `mark :0` is rejected here, but not by fast-import.
    ///
    /// filter-repo does not check any errors for this integer. It allows `+`
    /// sign, parse errors, empty digits, and junk after the integer.
    ///
    // Corresponds to `parse_mark` and `parse_mark_ref` in fast-import.c.
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

impl<'a> OriginalOid<&'a [u8]> {
    // Corresponds to `parse_original_identifier` in fast-import.c.
    #[inline]
    fn parse(original_oid: &'a [u8]) -> PResult<Self> {
        Ok(OriginalOid { oid: original_oid })
    }
}

impl<'a> Objectish<&'a [u8]> {
    // Corresponds to `from` in `parse_new_tag` in fast-import.c.
    fn parse(objectish: &'a [u8]) -> PResult<Self> {
        // Non-commits are allowed.
        if objectish.starts_with(b":") {
            Mark::parse(objectish).map(Objectish::Mark)
        } else {
            Ok(Objectish::BranchOrOid(objectish))
        }
    }
}

impl<'a> Commitish<&'a [u8]> {
    // Corresponds to `parse_objectish` and `parse_merge` in fast-import.c.
    fn parse(commitish: &'a [u8]) -> PResult<Self> {
        // TODO: How much of `parse_objectish` should be here or in the
        // front-end?
        // Only commits are allowed.
        Objectish::parse(commitish).map(|objectish| Commitish { commit: objectish })
    }
}

impl<'a> PersonIdent<&'a [u8]> {
    // Corresponds to `parse_ident` in fast-import.c.
    fn parse(ident: &'a [u8]) -> PResult<Self> {
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

        Ok(PersonIdent {
            name: &ident[..lt.saturating_sub(2)],
            email: &ident[lt + 1..gt - 1],
            date: &ident[(gt + 2).min(ident.len())..],
        })
    }
}

impl<'a> Encoding<&'a [u8]> {
    // Corresponds to part of `parse_new_commit` in fast-import.c.
    #[inline(always)]
    fn parse(encoding: &'a [u8]) -> PResult<Self> {
        Ok(Encoding { encoding })
    }
}

impl<'a> DataHeader<&'a [u8]> {
    /// Parses a `data` directive, but does not read its contents. git
    /// fast-import reads blobs into memory or switches to streaming when they
    /// exceed `--big-file-threshold` (default 512MiB).
    ///
    // Corresponds to `parse_and_store_blob` in fast-import.c.
    fn parse(arg: &'a [u8]) -> PResult<Self> {
        if let Some(delim) = arg.strip_prefix(b"<<") {
            if delim == b"" {
                return Err(ParseError::EmptyDelim.into());
            } else if delim.contains(&b'\0') {
                return Err(ParseError::DataDelimContainsNul.into());
            }
            Ok(DataHeader::Delimited { delim })
        } else {
            let len = parse_u64(arg).ok_or(ParseError::InvalidDataLength)?;
            Ok(DataHeader::Counted { len })
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
