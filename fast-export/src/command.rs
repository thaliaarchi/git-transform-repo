use std::{
    fmt::{self, Debug, Formatter},
    io::BufRead,
    num::NonZeroU64,
};

use bstr::ByteSlice;
use thiserror::Error;

use crate::parse::DataStream;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command<'a, R: BufRead> {
    Blob(Blob<'a, R>),
    Commit(Commit<'a>),
    Tag(Tag<'a>),
    Reset(Reset<'a>),
    Ls(Ls<'a>),
    CatBlob(CatBlob<'a>),
    GetMark(GetMark<'a>),
    Checkpoint,
    Done(Done),
    Alias(Alias<'a>),
    Progress(Progress<'a>),
    Feature(Feature<'a>),
    Option(OptionCommand<'a>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob<'a, R: BufRead> {
    pub mark: Option<Mark>,
    pub original_oid: Option<OriginalOid<'a>>,
    pub data: DataStream<'a, R>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit<'a>(&'a ());

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag<'a>(&'a ());

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reset<'a>(&'a ());

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ls<'a>(&'a ());

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatBlob<'a>(&'a ());

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetMark<'a>(&'a ());

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Done {
    /// The stream was explicitly terminated with a `done` command.
    Explicit,
    /// The stream was terminated with EOF.
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias<'a>(&'a ());

#[derive(Clone, PartialEq, Eq)]
pub struct Progress<'a> {
    pub message: &'a [u8],
}

impl Debug for Progress<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Progress")
            .field(&self.message.as_bstr())
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feature<'a>(&'a ());

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionCommand<'a> {
    Git(OptionGit<'a>),
    Other(OptionOther<'a>),
}

#[derive(Clone, PartialEq, Eq)]
pub enum OptionGit<'a> {
    MaxPackSize(FileSize),
    BigFileThreshold(FileSize),
    Depth(u32),
    ActiveBranches(u32),
    ExportPackEdges(&'a [u8]),
    Quiet,
    Stats,
    AllowUnsafeFeatures,
}

#[derive(Clone, PartialEq, Eq)]
pub struct OptionOther<'a> {
    pub option: &'a [u8],
}

impl Debug for OptionGit<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            OptionGit::MaxPackSize(n) => f.debug_tuple("MaxPackSize").field(n).finish(),
            OptionGit::BigFileThreshold(n) => f.debug_tuple("BigFileThreshold").field(n).finish(),
            OptionGit::Depth(n) => f.debug_tuple("Depth").field(n).finish(),
            OptionGit::ActiveBranches(n) => f.debug_tuple("ActiveBranches").field(n).finish(),
            OptionGit::ExportPackEdges(file) => f
                .debug_tuple("ExportPackEdges")
                .field(&file.as_bstr())
                .finish(),
            OptionGit::Quiet => write!(f, "Quiet"),
            OptionGit::Stats => write!(f, "Stats"),
            OptionGit::AllowUnsafeFeatures => write!(f, "AllowUnsafeFeatures"),
        }
    }
}

impl Debug for OptionOther<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OptionOther")
            .field(&self.option.as_bstr())
            .finish()
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
#[derive(Clone, Copy, PartialEq, Eq)]
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

impl Debug for Mark {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Mark").field(&self.mark).finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct OriginalOid<'a> {
    pub oid: &'a [u8],
}

impl Debug for OriginalOid<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OriginalOid")
            .field(&self.oid.as_bstr())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum DataHeader<'a> {
    Counted {
        len: u64, // uintmax_t in fast-import (at least u64)
    },
    Delimited {
        delim: &'a [u8],
    },
}

impl Debug for DataHeader<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DataHeader::Counted { len } => f.debug_struct("Counted").field("len", len).finish(),
            DataHeader::Delimited { delim } => f
                .debug_struct("Delimited")
                .field("delim", &delim.as_bstr())
                .finish(),
        }
    }
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
