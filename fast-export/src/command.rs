// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    fmt::{self, Debug, Formatter},
    io::BufRead,
    num::NonZeroU64,
    ptr,
};

use thiserror::Error;

use crate::parse::{DataReader, PResult, Parser};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command<'a, B, R> {
    Blob(Blob<'a, B, R>),
    Commit(Commit<B>),
    Tag(Tag<B>),
    Reset(Reset<B>),
    Ls(Ls<B>),
    CatBlob(CatBlob<B>),
    GetMark(GetMark),
    Checkpoint,
    Done(Done),
    Alias(Alias<B>),
    Progress(Progress<B>),
    Feature(Feature<B>),
    Option(OptionCommand<B>),
}

#[derive(Clone)]
pub struct Blob<'a, B, R> {
    pub mark: Option<Mark>,
    pub original_oid: Option<OriginalOid<B>>,
    pub data_header: DataHeader<B>,
    pub(crate) parser: &'a Parser<R>,
}

impl<'a, B, R: BufRead> Blob<'a, B, R> {
    /// Opens this blob for reading. Only one instance of [`DataReader`] can
    /// exist at a time.
    #[inline(always)]
    pub fn open(&self) -> PResult<DataReader<'a, R>> {
        DataReader::open(self.parser)
    }
}

impl<B: Debug, R> Debug for Blob<'_, B, R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Blob")
            .field("mark", &self.mark)
            .field("original_oid", &self.original_oid)
            .field("data_header", &self.data_header)
            .finish()
    }
}

impl<B: PartialEq, R> PartialEq for Blob<'_, B, R> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.mark == other.mark
            && self.original_oid == other.original_oid
            && self.data_header == other.data_header
            && ptr::eq(self.parser as _, other.parser as _)
    }
}

impl<B: Eq, R> Eq for Blob<'_, B, R> {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit<B> {
    pub branch: Branch<B>,
    pub mark: Option<Mark>,
    pub original_oid: Option<OriginalOid<B>>,
    pub author: Option<PersonIdent<B>>,
    pub committer: PersonIdent<B>,
    pub encoding: Option<Encoding<B>>,
    pub message: B,
    pub from: Option<Commitish<B>>,
    pub merge: Vec<Commitish<B>>,
    // TODO
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag<B> {
    pub name: TagName<B>,
    pub mark: Option<Mark>,
    pub from: Objectish<B>,
    pub original_oid: Option<OriginalOid<B>>,
    // TODO: `tagger` is optional in fast-import.c, but required in the
    // fast-import docs.
    pub tagger: Option<PersonIdent<B>>,
    pub message: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reset<B> {
    pub branch: Branch<B>,
    pub from: Option<Commitish<B>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ls<B> {
    pub root: Treeish<B>,
    pub path: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatBlob<B> {
    pub blob: Blobish<B>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetMark {
    pub mark: Mark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Done {
    /// The stream was explicitly terminated with a `done` command.
    Explicit,
    /// The stream was terminated with EOF.
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias<B> {
    pub mark: Mark,
    pub to: Commitish<B>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress<B> {
    pub message: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Feature<B> {
    DateFormat {
        format: DateFormat,
    },
    ImportMarks {
        path: FastImportPath<B>,
        ignore_missing: bool,
    },
    ExportMarks {
        path: FastImportPath<B>,
    },
    Alias,
    RewriteSubmodulesTo {
        submodule_name: B,
        marks_path: B,
    },
    RewriteSubmodulesFrom {
        submodule_name: B,
        marks_path: B,
    },
    GetMark,
    CatBlob,
    RelativeMarks {
        relative: bool,
    },
    Done,
    Force,
    Notes,
    Ls,
    Other {
        feature: B,
    },
}

impl<B> Feature<B> {
    /// Whether this feature requires `--allow-unsafe-features`, when requested
    /// in a stream.
    pub fn is_unsafe(&self) -> bool {
        matches!(
            self,
            Feature::ImportMarks { .. } | Feature::ExportMarks { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateFormat {
    Raw,
    RawPermissive,
    Rfc2822,
    Now,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FastImportPath<B> {
    pub path: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionCommand<B> {
    Git(OptionGit<B>),
    Other(OptionOther<B>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionGit<B> {
    MaxPackSize { size: FileSize },
    BigFileThreshold { size: FileSize },
    Depth { depth: u32 },
    ActiveBranches { count: u32 },
    ExportPackEdges { path: B },
    Quiet,
    Stats,
    AllowUnsafeFeatures,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionOther<B> {
    pub option: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch<B> {
    pub branch: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagName<B> {
    pub name: B,
}

/// An error from validating that a branch has a valid format for Git.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitBranchNameError {}

impl<B> Branch<B> {
    /// Validates a branch name according to the Git format.
    ///
    // Corresponds to `git.git/refs.c:check_refname_format` (called by
    // `git.git/builtin/fast-import.c:new_branch`).
    pub fn validate_git(&self) -> Result<(), GitBranchNameError> {
        todo!()
    }
}

/// A reference to an object by an integer, which allows the front-end to recall
/// it later without knowing its hash. The value `:0` is reserved and cannot be
/// used as a mark.
///
/// # Differences from fast-import
///
/// If `:0` is explicitly used in a mark definition, it is rejected as an error.
/// fast-import allows it and treats it as if no mark was given, even though its
/// [docs](https://git-scm.com/docs/git-fast-import#_mark) state it is reserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Mark {
    pub mark: NonZeroU64, // uintmax_t in fast-import (at least u64)
}

impl Mark {
    #[inline]
    pub fn new(mark: u64) -> Option<Mark> {
        NonZeroU64::new(mark).map(|mark| Mark { mark })
    }

    #[inline]
    pub fn get(&self) -> u64 {
        self.mark.get()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalOid<B> {
    pub oid: B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Mode {
    File = 0o100644,
    Exe = 0o100755,
    SymLink = 0o120000,
    GitLink = 0o160000,
    Dir = 0o040000,
}

impl Mode {
    // Corresponds to `git.git/object.h:canon_mode`.
    #[inline]
    pub fn canonicalize(mode: u16) -> Self {
        match mode & 0o170000 {
            0o100000 => {
                if mode & 0o100 != 0 {
                    Mode::Exe
                } else {
                    Mode::File
                }
            }
            0o120000 => Mode::SymLink,
            0o040000 => Mode::Dir,
            _ => Mode::GitLink,
        }
    }

    #[inline]
    pub fn is_canon(mode: u16) -> bool {
        Mode::canonicalize(mode) as u16 == mode
    }
}

// TODO: The distinction between Objectish, Commitish, Blobish, and Treeish is
// fuzzy.
// TODO: Parse refs like `git check-ref-format`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Objectish<B> {
    Mark(Mark),
    // TODO: Parse branches and oids
    BranchOrOid(B),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commitish<B> {
    pub commit: Objectish<B>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blobish<B> {
    Mark(Mark),
    Oid(B),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Treeish<B> {
    Mark(Mark),
    Oid(B),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonIdent<B> {
    pub name: B,
    pub email: B,
    // TODO: Parse dates
    pub date: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoding<B> {
    pub encoding: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataHeader<B> {
    Counted {
        len: u64, // uintmax_t in fast-import (at least u64)
    },
    Delimited {
        delim: B,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataBuf {
    pub data: Vec<u8>,
    pub delim: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum DelimitedError {
    /// fast-import accepts opening, but not closing, delimiters that contain
    /// NUL, so it will never close such data.
    #[error("delimiter contains NUL ('\\0')")]
    DelimContainsNul,
    /// fast-import accepts an empty delimiter, but receiving that is most
    /// likely an error, so we reject it.
    #[error("delimiter is empty")]
    EmptyDelim,
    /// A line equal to the delimiter in the data will end the data early.
    #[error("data contains delimiter")]
    DataContainsDelim,
    /// The close delimiter must appear at the start of a line, so only data
    /// ending in LF can be delimited.
    #[error("data does not end with LF ('\\n')")]
    NoFinalLf,
}

impl DataBuf {
    pub fn validate_delim(&self) -> Result<(), DelimitedError> {
        if let Some(delim) = &self.delim {
            if delim.is_empty() {
                Err(DelimitedError::EmptyDelim)
            } else if delim.contains(&b'\0') {
                Err(DelimitedError::DelimContainsNul)
            } else if !self.data.ends_with(b"\n") {
                Err(DelimitedError::NoFinalLf)
            } else if self.data.split(|&b| b == b'\n').any(|line| line == &*delim) {
                Err(DelimitedError::DataContainsDelim)
            } else {
                Ok(())
            }
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSize {
    pub value: u32,
    pub unit: UnitFactor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitFactor {
    B,
    K,
    M,
    G,
}

pub trait MapBytes<T, U> {
    type Output;

    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output;
}

impl<A, T, U> MapBytes<T, U> for Option<A>
where
    A: MapBytes<T, U>,
{
    type Output = Option<A::Output>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        self.map(|v| v.map_bytes(f))
    }
}

impl<A, T, U> MapBytes<T, U> for Vec<A>
where
    A: MapBytes<T, U>,
{
    type Output = Vec<A::Output>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        self.into_iter().map(|v| v.map_bytes(f)).collect()
    }
}

impl<'a, T, U, R> MapBytes<T, U> for Command<'a, T, R> {
    type Output = Command<'a, U, R>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        match self {
            Command::Blob(blob) => Command::Blob(blob.map_bytes(f)),
            Command::Commit(commit) => Command::Commit(commit.map_bytes(f)),
            Command::Tag(tag) => Command::Tag(tag.map_bytes(f)),
            Command::Reset(reset) => Command::Reset(reset.map_bytes(f)),
            Command::Ls(ls) => Command::Ls(ls.map_bytes(f)),
            Command::CatBlob(cat_blob) => Command::CatBlob(cat_blob.map_bytes(f)),
            Command::GetMark(get_mark) => Command::GetMark(get_mark),
            Command::Checkpoint => Command::Checkpoint,
            Command::Done(done) => Command::Done(done),
            Command::Alias(alias) => Command::Alias(alias.map_bytes(f)),
            Command::Progress(progress) => Command::Progress(progress.map_bytes(f)),
            Command::Feature(feature) => Command::Feature(feature.map_bytes(f)),
            Command::Option(option) => Command::Option(option.map_bytes(f)),
        }
    }
}

impl<'a, T, U, R> MapBytes<T, U> for Blob<'a, T, R> {
    type Output = Blob<'a, U, R>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Blob {
            mark: self.mark,
            original_oid: self.original_oid.map_bytes(f),
            data_header: self.data_header.map_bytes(f),
            parser: self.parser,
        }
    }
}

impl<T, U> MapBytes<T, U> for Commit<T> {
    type Output = Commit<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Commit {
            branch: self.branch.map_bytes(f),
            mark: self.mark,
            original_oid: self.original_oid.map_bytes(f),
            author: self.author.map_bytes(f),
            committer: self.committer.map_bytes(f),
            encoding: self.encoding.map_bytes(f),
            message: f(self.message),
            from: self.from.map_bytes(f),
            merge: self.merge.map_bytes(f),
        }
    }
}

impl<T, U> MapBytes<T, U> for Tag<T> {
    type Output = Tag<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Tag {
            name: self.name.map_bytes(f),
            mark: self.mark,
            from: self.from.map_bytes(f),
            original_oid: self.original_oid.map_bytes(f),
            tagger: self.tagger.map_bytes(f),
            message: f(self.message),
        }
    }
}

impl<T, U> MapBytes<T, U> for Reset<T> {
    type Output = Reset<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Reset {
            branch: self.branch.map_bytes(f),
            from: self.from.map_bytes(f),
        }
    }
}

impl<T, U> MapBytes<T, U> for Ls<T> {
    type Output = Ls<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Ls {
            root: self.root.map_bytes(f),
            path: f(self.path),
        }
    }
}

impl<T, U> MapBytes<T, U> for CatBlob<T> {
    type Output = CatBlob<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        CatBlob {
            blob: self.blob.map_bytes(f),
        }
    }
}

impl<T, U> MapBytes<T, U> for Alias<T> {
    type Output = Alias<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Alias {
            mark: self.mark,
            to: self.to.map_bytes(f),
        }
    }
}

impl<T, U> MapBytes<T, U> for Progress<T> {
    type Output = Progress<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Progress {
            message: f(self.message),
        }
    }
}

impl<T, U> MapBytes<T, U> for Feature<T> {
    type Output = Feature<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        match self {
            Feature::DateFormat { format } => Feature::DateFormat { format },
            Feature::ImportMarks {
                path,
                ignore_missing,
            } => Feature::ImportMarks {
                path: path.map_bytes(f),
                ignore_missing,
            },
            Feature::ExportMarks { path } => Feature::ExportMarks {
                path: path.map_bytes(f),
            },
            Feature::Alias => Feature::Alias,
            Feature::RewriteSubmodulesTo {
                submodule_name,
                marks_path,
            } => Feature::RewriteSubmodulesTo {
                submodule_name: f(submodule_name),
                marks_path: f(marks_path),
            },
            Feature::RewriteSubmodulesFrom {
                submodule_name,
                marks_path,
            } => Feature::RewriteSubmodulesFrom {
                submodule_name: f(submodule_name),
                marks_path: f(marks_path),
            },
            Feature::GetMark => Feature::GetMark,
            Feature::CatBlob => Feature::CatBlob,
            Feature::RelativeMarks { relative } => Feature::RelativeMarks { relative },
            Feature::Done => Feature::Done,
            Feature::Force => Feature::Force,
            Feature::Notes => Feature::Notes,
            Feature::Ls => Feature::Ls,
            Feature::Other { feature } => Feature::Other {
                feature: f(feature),
            },
        }
    }
}

impl<T, U> MapBytes<T, U> for FastImportPath<T> {
    type Output = FastImportPath<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        FastImportPath { path: f(self.path) }
    }
}

impl<T, U> MapBytes<T, U> for OptionCommand<T> {
    type Output = OptionCommand<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        match self {
            OptionCommand::Git(option) => OptionCommand::Git(option.map_bytes(f)),
            OptionCommand::Other(option) => OptionCommand::Other(option.map_bytes(f)),
        }
    }
}

impl<T, U> MapBytes<T, U> for OptionGit<T> {
    type Output = OptionGit<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        match self {
            OptionGit::MaxPackSize { size } => OptionGit::MaxPackSize { size },
            OptionGit::BigFileThreshold { size } => OptionGit::BigFileThreshold { size },
            OptionGit::Depth { depth } => OptionGit::Depth { depth },
            OptionGit::ActiveBranches { count } => OptionGit::ActiveBranches { count },
            OptionGit::ExportPackEdges { path } => OptionGit::ExportPackEdges { path: f(path) },
            OptionGit::Quiet => OptionGit::Quiet,
            OptionGit::Stats => OptionGit::Stats,
            OptionGit::AllowUnsafeFeatures => OptionGit::AllowUnsafeFeatures,
        }
    }
}

impl<T, U> MapBytes<T, U> for OptionOther<T> {
    type Output = OptionOther<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        OptionOther {
            option: f(self.option),
        }
    }
}

impl<T, U> MapBytes<T, U> for Branch<T> {
    type Output = Branch<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Branch {
            branch: f(self.branch),
        }
    }
}

impl<T, U> MapBytes<T, U> for TagName<T> {
    type Output = TagName<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        TagName { name: f(self.name) }
    }
}

impl<T, U> MapBytes<T, U> for OriginalOid<T> {
    type Output = OriginalOid<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        OriginalOid { oid: f(self.oid) }
    }
}

impl<T, U> MapBytes<T, U> for Objectish<T> {
    type Output = Objectish<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        match self {
            Objectish::Mark(mark) => Objectish::Mark(mark),
            Objectish::BranchOrOid(commit) => Objectish::BranchOrOid(f(commit)),
        }
    }
}

impl<T, U> MapBytes<T, U> for Commitish<T> {
    type Output = Commitish<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Commitish {
            commit: self.commit.map_bytes(f),
        }
    }
}

impl<T, U> MapBytes<T, U> for Blobish<T> {
    type Output = Blobish<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        match self {
            Blobish::Mark(mark) => Blobish::Mark(mark),
            Blobish::Oid(oid) => Blobish::Oid(f(oid)),
        }
    }
}

impl<T, U> MapBytes<T, U> for Treeish<T> {
    type Output = Treeish<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        match self {
            Treeish::Mark(mark) => Treeish::Mark(mark),
            Treeish::Oid(oid) => Treeish::Oid(f(oid)),
        }
    }
}

impl<T, U> MapBytes<T, U> for PersonIdent<T> {
    type Output = PersonIdent<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        PersonIdent {
            name: f(self.name),
            email: f(self.email),
            date: f(self.date),
        }
    }
}

impl<T, U> MapBytes<T, U> for Encoding<T> {
    type Output = Encoding<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Encoding {
            encoding: f(self.encoding),
        }
    }
}

impl<T, U> MapBytes<T, U> for DataHeader<T> {
    type Output = DataHeader<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        match self {
            DataHeader::Counted { len } => DataHeader::Counted { len },
            DataHeader::Delimited { delim } => DataHeader::Delimited { delim: f(delim) },
        }
    }
}

impl<'a, B, R> From<Blob<'a, B, R>> for Command<'a, B, R> {
    #[inline(always)]
    fn from(blob: Blob<'a, B, R>) -> Self {
        Command::Blob(blob)
    }
}

impl<B, R> From<Commit<B>> for Command<'_, B, R> {
    #[inline(always)]
    fn from(commit: Commit<B>) -> Self {
        Command::Commit(commit)
    }
}

impl<B, R> From<Tag<B>> for Command<'_, B, R> {
    #[inline(always)]
    fn from(tag: Tag<B>) -> Self {
        Command::Tag(tag)
    }
}

impl<B, R> From<Reset<B>> for Command<'_, B, R> {
    #[inline(always)]
    fn from(reset: Reset<B>) -> Self {
        Command::Reset(reset)
    }
}

impl<B, R> From<Ls<B>> for Command<'_, B, R> {
    #[inline(always)]
    fn from(ls: Ls<B>) -> Self {
        Command::Ls(ls)
    }
}

impl<B, R> From<CatBlob<B>> for Command<'_, B, R> {
    #[inline(always)]
    fn from(cat_blob: CatBlob<B>) -> Self {
        Command::CatBlob(cat_blob)
    }
}

impl<B, R> From<GetMark> for Command<'_, B, R> {
    #[inline(always)]
    fn from(get_mark: GetMark) -> Self {
        Command::GetMark(get_mark)
    }
}

impl<B, R> From<Done> for Command<'_, B, R> {
    #[inline(always)]
    fn from(done: Done) -> Self {
        Command::Done(done)
    }
}

impl<B, R> From<Alias<B>> for Command<'_, B, R> {
    #[inline(always)]
    fn from(alias: Alias<B>) -> Self {
        Command::Alias(alias)
    }
}

impl<B, R> From<Progress<B>> for Command<'_, B, R> {
    #[inline(always)]
    fn from(progress: Progress<B>) -> Self {
        Command::Progress(progress)
    }
}

impl<B, R> From<Feature<B>> for Command<'_, B, R> {
    #[inline(always)]
    fn from(feature: Feature<B>) -> Self {
        Command::Feature(feature)
    }
}

impl<B, R> From<OptionCommand<B>> for Command<'_, B, R> {
    #[inline(always)]
    fn from(option: OptionCommand<B>) -> Self {
        Command::Option(option)
    }
}
