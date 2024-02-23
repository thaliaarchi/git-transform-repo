use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command<'a> {
    Blob(Blob<'a>),
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
pub struct Blob<'a> {
    pub mark: Option<Mark>,
    pub original_oid: Option<OriginalOid<'a>>,
    pub data: Data<'a>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress<'a> {
    pub message: &'a [u8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feature<'a>(&'a ());

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionCommand<'a> {
    Git(OptionGit<'a>),
    Other(OptionOther<'a>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionOther<'a> {
    pub option: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Mark {
    pub mark: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalOid<'a> {
    pub oid: &'a [u8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Data<'a> {
    pub data: &'a [u8],
    pub delim: Option<&'a [u8]>,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum DelimitedError {
    #[error("data contains delimiter")]
    ContainsDelim,
    #[error("data does not end with LF ('\\n')")]
    NoFinalLf,
    // TODO: Verify this case.
    #[error("data contains NUL ('\\0')")]
    ContainsNul,
    /// fast-import accepts an empty delimiter, but receiving that is most
    /// likely an error, so it is forbidden here.
    #[error("delimiter is empty")]
    EmptyDelim,
}

impl Data<'_> {
    pub fn validate_delim(&self) -> Result<(), DelimitedError> {
        if let Some(delim) = self.delim {
            if delim.is_empty() {
                Err(DelimitedError::EmptyDelim)
            } else if !matches!(self.data, [.., b'\n']) {
                Err(DelimitedError::NoFinalLf)
            } else if self.data.contains(&b'\0') {
                Err(DelimitedError::ContainsNul)
            } else if self.data.split(|&b| b == b'\n').any(|line| line == &*delim) {
                Err(DelimitedError::ContainsDelim)
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
