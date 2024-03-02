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
    Tag(Tag),
    Reset(Reset),
    Ls(Ls),
    CatBlob(CatBlob),
    GetMark(GetMark),
    Checkpoint,
    Done(Done),
    Alias(Alias),
    Progress(Progress<B>),
    Feature(Feature),
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
pub struct Tag;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reset;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ls;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatBlob;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetMark;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Done {
    /// The stream was explicitly terminated with a `done` command.
    Explicit,
    /// The stream was terminated with EOF.
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress<B> {
    pub message: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feature;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionCommand<B> {
    Git(OptionGit<B>),
    Other(OptionOther<B>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionGit<B> {
    MaxPackSize(FileSize),
    BigFileThreshold(FileSize),
    Depth(u32),
    ActiveBranches(u32),
    ExportPackEdges(B),
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

/// An error from validating that a branch has a valid format for git.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitBranchNameError {}

impl<B> Branch<B> {
    /// Validates a branch name according to the git format.
    ///
    // Corresponds to `check_refname_format` in refs.c (called by `new_branch`
    // in fast-import.c).
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Commitish<B> {
    Mark(Mark),
    // TODO: Parse branches and oids
    BranchOrOid(B),
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
            Command::Tag(tag) => Command::Tag(tag),
            Command::Reset(reset) => Command::Reset(reset),
            Command::Ls(ls) => Command::Ls(ls),
            Command::CatBlob(cat_blob) => Command::CatBlob(cat_blob),
            Command::GetMark(get_mark) => Command::GetMark(get_mark),
            Command::Checkpoint => Command::Checkpoint,
            Command::Done(done) => Command::Done(done),
            Command::Alias(alias) => Command::Alias(alias),
            Command::Progress(progress) => Command::Progress(progress.map_bytes(f)),
            Command::Feature(feature) => Command::Feature(feature),
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

impl<T, U> MapBytes<T, U> for Progress<T> {
    type Output = Progress<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        Progress {
            message: f(self.message),
        }
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
            OptionGit::MaxPackSize(n) => OptionGit::MaxPackSize(n),
            OptionGit::BigFileThreshold(n) => OptionGit::BigFileThreshold(n),
            OptionGit::Depth(n) => OptionGit::Depth(n),
            OptionGit::ActiveBranches(n) => OptionGit::ActiveBranches(n),
            OptionGit::ExportPackEdges(file) => OptionGit::ExportPackEdges(f(file)),
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

impl<T, U> MapBytes<T, U> for OriginalOid<T> {
    type Output = OriginalOid<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        OriginalOid { oid: f(self.oid) }
    }
}

impl<T, U> MapBytes<T, U> for Commitish<T> {
    type Output = Commitish<U>;

    #[inline(always)]
    fn map_bytes<F: FnMut(T) -> U>(self, f: &mut F) -> Self::Output {
        match self {
            Commitish::Mark(mark) => Commitish::Mark(mark),
            Commitish::BranchOrOid(commit) => Commitish::BranchOrOid(f(commit)),
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
