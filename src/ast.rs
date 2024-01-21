use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Blob(Commented<Blob>),
    Commit(Commented<Commit>),
    Tag(Commented<Tag>),
    Reset(Commented<Reset>),
    Ls(Commented<Ls>),
    CatBlob(Commented<CatBlob>),
    GetMark(Commented<GetMark>),
    Checkpoint(Commented<Checkpoint>),
    Done(Commented<Done>),
    Alias(Commented<Alias>),
    Progress(Commented<Progress>),
    Feature(Commented<Feature>),
    OptionGit(Commented<OptionGit>),
    OptionOther(Commented<OptionOther>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    pub mark: Option<Commented<Mark>>,
    pub original_oid: Option<Commented<OriginalOid>>,
    pub data: Commented<Data>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feature;

// Signs (`+`/none) are not 1-to-1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionGit {
    MaxPackSize(FileSize),
    BigFileThreshold(FileSize),
    Depth(u32),
    ActiveBranches(u32),
    ExportPackEdges(InlineString),
    Quiet,
    Stats,
    AllowUnsafeFeatures,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionOther(pub InlineString);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Mark {
    pub mark: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalOid {
    pub oid: InlineString,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Data {
    Counted(CountedData),
    Delimited(DelimitedData),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountedData {
    pub data: Vec<u8>,
    pub optional_lf: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelimitedData {
    data: Box<[u8]>,
    delim: Box<[u8]>,
    pub optional_lf: bool,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum DelimitedError {
    #[error("data contains delimiter")]
    ContainsDelim,
    #[error("data contains NUL ('\\0')")]
    ContainsNul,
    #[error("data does not end with LF ('\\n')")]
    NoFinalLf,
}

impl CountedData {
    #[inline]
    #[must_use]
    pub fn new<T: Into<Vec<u8>>>(data: T) -> Self {
        CountedData {
            data: data.into(),
            optional_lf: true,
        }
    }
}

impl DelimitedData {
    #[inline]
    pub fn new<T: Into<Vec<u8>>>(data: T, delim: InlineString) -> Result<Self, DelimitedError> {
        DelimitedData::_new(data.into().into_boxed_slice(), delim.bytes)
    }

    fn _new(data: Box<[u8]>, delim: Box<[u8]>) -> Result<Self, DelimitedError> {
        if data.last().is_some_and(|&b| b != b'\n') {
            Err(DelimitedError::NoFinalLf)
        } else if data.contains(&b'\0') {
            Err(DelimitedError::ContainsNul)
        } else if data.split(|&b| b == b'\n').any(|line| line == &*delim) {
            Err(DelimitedError::ContainsDelim)
        } else {
            Ok(DelimitedData {
                data,
                delim,
                optional_lf: true,
            })
        }
    }

    #[inline]
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    #[inline]
    #[must_use]
    pub fn into_data(self) -> Vec<u8> {
        self.data.into_vec()
    }

    #[inline]
    #[must_use]
    pub fn delim(&self) -> &[u8] {
        &self.delim
    }
}

impl From<CountedData> for Data {
    #[inline]
    fn from(data: CountedData) -> Self {
        Data::Counted(data)
    }
}

impl From<DelimitedData> for Data {
    #[inline]
    fn from(data: DelimitedData) -> Self {
        Data::Delimited(data)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSize {
    pub value: u32,
    pub unit: UnitFactor,
}

// Case is not 1-to-1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitFactor {
    B,
    K,
    M,
    G,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineString {
    bytes: Box<[u8]>,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum InlineStringError {
    #[error("inline string contains NUL ('\\0')")]
    ContainsNul,
    #[error("inline string contains LF ('\\n')")]
    ContainsLf,
}

impl InlineString {
    #[inline]
    pub fn new<T: Into<Vec<u8>>>(bytes: T) -> Result<Self, InlineStringError> {
        InlineString::_new(bytes.into().into_boxed_slice())
    }

    fn _new(bytes: Box<[u8]>) -> Result<Self, InlineStringError> {
        if let Some(&b) = bytes.iter().find(|&&b| b == b'\0' || b == b'\n') {
            Err(if b == b'\0' {
                InlineStringError::ContainsNul
            } else {
                InlineStringError::ContainsLf
            })
        } else {
            Ok(InlineString { bytes })
        }
    }

    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[inline]
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes.into_vec()
    }
}

impl TryFrom<Vec<u8>> for InlineString {
    type Error = InlineStringError;

    #[inline]
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        InlineString::new(bytes)
    }
}

impl PartialEq<[u8]> for InlineString {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }
}

impl PartialEq<InlineString> for [u8] {
    #[inline]
    fn eq(&self, other: &InlineString) -> bool {
        self == other.as_bytes()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comments {
    text: Box<[u8]>,
}

impl Comments {
    #[inline]
    #[must_use]
    pub fn new<T: Into<Vec<u8>>>(text: T) -> Self {
        Comments {
            text: text.into().into_boxed_slice(),
        }
    }

    #[inline]
    #[must_use]
    pub fn text(&self) -> &[u8] {
        &self.text
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commented<T> {
    pub comments: Comments,
    pub value: T,
}

impl<T> Commented<T> {
    #[inline]
    #[must_use]
    pub fn new(comments: Comments, value: T) -> Self {
        Commented { comments, value }
    }

    #[inline]
    #[must_use]
    pub fn wrap(value: T) -> Self {
        Commented::new(Comments::default(), value)
    }
}

impl<T> From<T> for Commented<T> {
    #[inline]
    fn from(value: T) -> Self {
        Commented::wrap(value)
    }
}

impl<T: Default> Default for Commented<T> {
    #[inline]
    fn default() -> Self {
        Commented {
            comments: Comments::default(),
            value: T::default(),
        }
    }
}
