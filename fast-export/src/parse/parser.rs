// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    cell::UnsafeCell,
    io::{self, BufRead},
    str::{self, FromStr},
    sync::atomic::{AtomicBool, Ordering},
};

use memchr::memchr;
use thiserror::Error;

use crate::{
    command::{
        Alias, Blob, Blobish, Branch, CatBlob, Command, Commit, Commitish, DataHeader, DateFormat,
        Done, Encoding, FastImportPath, Feature, FileSize, GetMark, Ls, Mark, Objectish,
        OptionCommand, OptionGit, OptionOther, OriginalOid, PersonIdent, Progress, Reset, Tag,
        TagName, Treeish, UnitFactor,
    },
    parse::{BufInput, DataReaderError, DataState, DirectiveParser, PResult, ParseStringError},
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

    // Fields that are silently truncated by fast-import when they contain NUL.
    #[error("branch name contains NUL")]
    BranchContainsNul,
    #[error("person identifier contains NUL")]
    IdentContainsNul,
    #[error("encoding specifier contains NUL")]
    EncodingContainsNul,
    /// A data delimiter containing NUL is truncated and will never match with a
    /// closing delimiter, always leaving its data unterminated.
    #[error("data delimiter contains NUL")]
    DataDelimContainsNul,
    #[error("tag name contains NUL")]
    TagContainsNul,
    #[error("path contains NUL")]
    PathContainsNul,
    #[error("rewrite submodules feature contains NUL")]
    RewriteSubmodulesContainsNul,

    #[error("invalid mode integer")]
    InvalidModeInt,
    #[error("invalid mode")]
    InvalidMode,
    #[error("no space after mode")]
    NoSpaceAfterMode,
    #[error("no space after data ref")]
    NoSpaceAfterDataRef,
    #[error("junk after path in commit 'M'")]
    JunkAfterFileModifyPath,
    #[error("junk after path in commit 'D'")]
    JunkAfterFileDeletePath,
    #[error("missing space after source path")]
    NoSpaceAfterSource,
    #[error("missing destination path")]
    MissingDest,
    #[error("junk after destination path")]
    JunkAfterDest,

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

    /// A mark must start with `:`.
    #[error("mark does not start with ':'")]
    MarkMissingColon,
    /// The mark is not a valid integer. fast-import allows more forms of
    /// ill-formatted integers than here.
    #[error("invalid mark integer")]
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
    /// fast-import accepts an empty delimiter, but receiving that is most
    /// likely an error, so we reject it.
    #[error("data delimiter is empty")]
    EmptyDelim,
    /// EOF was reached before encountering the data delimiter.
    #[error("unterminated delimited data stream")]
    UnterminatedData,

    #[error("expected root before path in 'ls'")]
    MissingLsRoot,
    #[error("expected path after root in 'ls'")]
    MissingLsPath,
    #[error("invalid path in 'ls': {0}")]
    LsPathString(#[source] ParseStringError),
    #[error("junk after path in 'ls'")]
    JunkAfterLsPath,

    #[error("invalid date format")]
    InvalidDateFormat,
    /// Expected format `name:filename`` for rewrite submodules feature.
    #[error("expected ':' in submodule rewrite")]
    RewriteSubmodulesNoColon,

    #[error("unrecognized 'option git' option")]
    UnsupportedGitOption,
    #[error("invalid file size argument in option")]
    InvalidOptionFileSize,
    #[error("invalid integer argument in option")]
    InvalidOptionInt,

    /// The command is not recognized.
    #[error("unrecognized command")]
    UnrecognizedCommand,
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
            return Ok(Command::from(Done::Eof));
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
            Ok(Command::from(Done::Explicit))
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
            Err(ParseError::UnrecognizedCommand.into())
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

        Ok(Command::from(Blob {
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

        Ok(Command::from(Commit {
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

        Ok(Command::from(Tag {
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

        Ok(Command::from(Reset { branch, from }))
    }

    // Corresponds to `parse_ls(p, NULL)` in fast-import.c.
    fn parse_ls<'a>(&'a self, args: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        let (root, path) = parse_ls(self, args, false)?;
        Ok(Command::from(Ls {
            root: root.unwrap(),
            path,
        }))
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob<'a>(&'a self, data_ref: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        let blob = Blobish::parse(data_ref)?;
        Ok(Command::from(CatBlob { blob }))
    }

    // Corresponds to `parse_get_mark` in fast-import.c.
    fn parse_get_mark<'a>(&'a self, mark: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        // TODO: :0 is not forbidden by fast-import. How would it behave?
        let mark = Mark::parse(mark)?;
        Ok(Command::from(GetMark { mark }))
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

        Ok(Command::from(Alias { mark, to }))
    }

    // Corresponds to `parse_progress` in fast-import.c.
    fn parse_progress<'a>(&'a self, message: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        self.input.skip_optional_lf()?;
        Ok(Command::from(Progress { message }))
    }

    // Corresponds to `parse_feature` in fast-import.c.
    fn parse_feature<'a>(&'a self, feature: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        Ok(Command::from(Feature::parse(feature)?))
    }

    // Corresponds to `parse_option` in fast-import.c.
    fn parse_option<'a>(&'a self, option: &'a [u8]) -> PResult<Command<'a, &'a [u8], R>> {
        let option = if let Some(option) = option.strip_prefix(b"git ") {
            OptionCommand::Git(OptionGit::parse(option)?)
        } else {
            OptionCommand::Other(OptionOther { option })
        };
        Ok(Command::from(option))
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
        let message_buf = self.new_aux_buffer();
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
    pub(super) fn parse(mark: &[u8]) -> PResult<Self> {
        let [b':', mark @ ..] = mark else {
            return Err(ParseError::MarkMissingColon.into());
        };
        let mark = parse_int(mark).ok_or(ParseError::InvalidMark)?;
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
    pub(super) fn parse(commitish: &'a [u8]) -> PResult<Self> {
        // TODO: How much of `parse_objectish` should be here or in the
        // front-end?
        // Only commits are allowed.
        Objectish::parse(commitish).map(|objectish| Commitish { commit: objectish })
    }
}

impl<'a> Blobish<&'a [u8]> {
    // Corresponds to part of `parse_cat_blob` in fast-import.c.
    pub(super) fn parse(blobish: &'a [u8]) -> PResult<Self> {
        // TODO: Parse oids.
        if blobish.starts_with(b":") {
            Mark::parse(blobish).map(Blobish::Mark)
        } else {
            Ok(Blobish::Oid(blobish))
        }
    }
}

impl<'a> Treeish<&'a [u8]> {
    // Corresponds to `parse_treeish_dataref` in fast-import.c.
    fn parse(treeish: &'a [u8]) -> PResult<Self> {
        // TODO: Parse oids.
        if treeish.starts_with(b":") {
            Mark::parse(treeish).map(Treeish::Mark)
        } else {
            Ok(Treeish::Oid(treeish))
        }
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
        if encoding.contains(&b'\0') {
            return Err(ParseError::EncodingContainsNul.into());
        }
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
            let len = parse_int(arg).ok_or(ParseError::InvalidDataLength)?;
            Ok(DataHeader::Counted { len })
        }
    }
}

impl<'a> Feature<&'a [u8]> {
    // Corresponds to `parse_one_feature` in fast-import.
    fn parse(feature: &'a [u8]) -> PResult<Self> {
        if let Some(format) = feature.strip_prefix(b"date-format=") {
            Ok(Feature::DateFormat {
                format: DateFormat::parse(format)?,
            })
        } else if let Some(path) = feature.strip_prefix(b"import-marks=") {
            Ok(Feature::ImportMarks {
                path: FastImportPath::parse(path)?,
                ignore_missing: false,
            })
        } else if let Some(path) = feature.strip_prefix(b"import-marks-if-exists=") {
            Ok(Feature::ImportMarks {
                path: FastImportPath::parse(path)?,
                ignore_missing: true,
            })
        } else if let Some(path) = feature.strip_prefix(b"export-marks=") {
            Ok(Feature::ExportMarks {
                path: FastImportPath::parse(path)?,
            })
        } else if feature == b"alias" {
            Ok(Feature::Alias)
        } else if let Some(args) = feature.strip_prefix(b"rewrite-submodules-to=") {
            let (submodule_name, marks_path) = parse_rewrite_submodules(args)?;
            Ok(Feature::RewriteSubmodulesTo {
                submodule_name,
                marks_path,
            })
        } else if let Some(args) = feature.strip_prefix(b"rewrite-submodules-from=") {
            let (submodule_name, marks_path) = parse_rewrite_submodules(args)?;
            Ok(Feature::RewriteSubmodulesFrom {
                submodule_name,
                marks_path,
            })
        } else if feature == b"get-mark" {
            Ok(Feature::GetMark)
        } else if feature == b"cat-blob" {
            Ok(Feature::CatBlob)
        } else if feature == b"relative-marks" {
            Ok(Feature::RelativeMarks { relative: true })
        } else if feature == b"no-relative-marks" {
            Ok(Feature::RelativeMarks { relative: false })
        } else if feature == b"done" {
            Ok(Feature::Done)
        } else if feature == b"force" {
            Ok(Feature::Force)
        } else if feature == b"notes" {
            Ok(Feature::Notes)
        } else if feature == b"ls" {
            Ok(Feature::Ls)
        } else {
            Ok(Feature::Other { feature })
        }
    }
}

impl DateFormat {
    // Corresponds to `option_date_format` in fast-import.c.
    fn parse(format: &[u8]) -> PResult<Self> {
        match format {
            b"raw" => Ok(DateFormat::Raw),
            b"raw-permissive" => Ok(DateFormat::RawPermissive),
            b"rfc2822" => Ok(DateFormat::Rfc2822),
            b"now" => Ok(DateFormat::Now),
            _ => Err(ParseError::InvalidDateFormat.into()),
        }
    }
}

impl<'a> FastImportPath<&'a [u8]> {
    // Corresponds to part of `make_fast_import_path` in fast-import.c.
    fn parse(path: &'a [u8]) -> PResult<Self> {
        if path.contains(&b'\0') {
            return Err(ParseError::PathContainsNul.into());
        }
        // TODO: Make method to resolve the full path.
        Ok(FastImportPath { path })
    }
}

// Corresponds to `parse_ls` in fast-import.c.
pub(super) fn parse_ls<'a, P: DirectiveParser<R>, R: BufRead + 'a>(
    parser: &'a P,
    args: &'a [u8],
    in_commit: bool,
) -> PResult<(Option<Treeish<&'a [u8]>>, &'a [u8])> {
    if args.is_empty() {
        return Err(ParseError::MissingLsPath.into());
    }
    let (root, mut path) = if args[0] == b'"' {
        if !in_commit {
            return Err(ParseError::MissingLsRoot.into());
        }
        (None, args)
    } else {
        let i = memchr(b' ', args).ok_or(ParseError::MissingLsPath)?;
        let (root, path) = args.split_at(i);
        (Some(Treeish::parse(root)?), path)
    };
    if path.is_empty() {
        return Err(ParseError::MissingLsPath.into());
    }
    if path[0] == b'"' {
        let rest;
        (path, rest) = parser
            .unquote_c_style_string(path)
            .map_err(ParseError::LsPathString)?;
        if !rest.is_empty() {
            return Err(ParseError::JunkAfterLsPath.into());
        }
    }
    Ok((root, path))
}

// Corresponds to `option_rewrite_submodules` in fast-import.c.
fn parse_rewrite_submodules(args: &[u8]) -> PResult<(&[u8], &[u8])> {
    if args.contains(&b'\0') {
        return Err(ParseError::RewriteSubmodulesContainsNul.into());
    }
    let Some(colon) = args.iter().position(|&b| b == b':') else {
        return Err(ParseError::RewriteSubmodulesNoColon.into());
    };
    // TODO: Make method to resolve the full path.
    let (submodule_name, marks_path) = args.split_at(colon);
    Ok((submodule_name, marks_path))
}

impl<'a> OptionGit<&'a [u8]> {
    // Corresponds to `parse_one_option` in fast-import.c.
    fn parse(option: &'a [u8]) -> PResult<Self> {
        if let Some(size) = option.strip_prefix(b"max-pack-size=") {
            Ok(OptionGit::MaxPackSize {
                size: FileSize::parse(size)?,
            })
        } else if let Some(size) = option.strip_prefix(b"big-file-threshold=") {
            Ok(OptionGit::BigFileThreshold {
                size: FileSize::parse(size)?,
            })
        } else if let Some(depth) = option.strip_prefix(b"depth=") {
            Ok(OptionGit::Depth {
                depth: parse_int(depth).ok_or(ParseError::InvalidOptionInt)?,
            })
        } else if let Some(count) = option.strip_prefix(b"active-branches=") {
            Ok(OptionGit::ActiveBranches {
                count: parse_int(count).ok_or(ParseError::InvalidOptionInt)?,
            })
        } else if let Some(path) = option.strip_prefix(b"export-pack-edges=") {
            Ok(OptionGit::ExportPackEdges { path })
        } else if option == b"quiet" {
            Ok(OptionGit::Quiet)
        } else if option == b"stats" {
            Ok(OptionGit::Stats)
        } else if option == b"allow-unsafe-features" {
            Ok(OptionGit::AllowUnsafeFeatures)
        } else {
            Err(ParseError::UnrecognizedCommand.into())
        }
    }
}

impl FileSize {
    fn parse(size: &[u8]) -> PResult<Self> {
        let (value, unit) = match size {
            [value @ .., b'k' | b'K'] => (value, UnitFactor::K),
            [value @ .., b'm' | b'M'] => (value, UnitFactor::M),
            [value @ .., b'g' | b'G'] => (value, UnitFactor::G),
            _ => (size, UnitFactor::B),
        };
        Ok(FileSize {
            value: parse_int(value).ok_or(ParseError::InvalidOptionFileSize)?,
            unit,
        })
    }
}

// TODO: Return `PResult<T>`.
#[inline]
fn parse_int<T: FromStr>(b: &[u8]) -> Option<T> {
    // TODO: Make an integer parsing routine, to precisely control the grammar
    // and protect from upstream changes.
    if b.starts_with(b"+") {
        return None;
    }
    // SAFETY: `from_str` for integer types operates on bytes and accepts only
    // ASCII.
    T::from_str(unsafe { str::from_utf8_unchecked(b) }).ok()
}
