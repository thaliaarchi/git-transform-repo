use std::num::NonZeroU64;

use thiserror::Error;

use crate::parse::DataStream;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command<'a, B, R> {
    Blob(Blob<'a, B, R>),
    Commit(Commit<'a, B, R>),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob<'a, B, R> {
    pub mark: Option<Mark>,
    pub original_oid: Option<OriginalOid<B>>,
    pub data: DataStream<'a, B, R>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit<'a, B, R> {
    pub branch: Branch<B>,
    pub mark: Option<Mark>,
    pub original_oid: Option<OriginalOid<B>>,
    pub author: Option<PersonIdent<B>>,
    pub committer: PersonIdent<B>,
    pub encoding: Option<Encoding<B>>,
    pub message: DataStream<'a, B, R>,
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
