use std::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    io::{self, BufRead, Read},
    marker::PhantomData,
    ops::Range,
    ptr, result, str,
    sync::atomic::{AtomicBool, Ordering},
};

use bstr::ByteSlice;
use thiserror::Error;

use crate::command::{Blob, Command, DataHeader, Done, Mark, OriginalOid, Progress};

type Result<T> = std::result::Result<T, StreamError>;

/// A zero-copy pull parser for fast-export streams.
///
/// It uses only as much memory as the single largest command command in the
/// stream. Any references to parsed bytes returned by the parser are
/// invalidated when [`Parser::next`] is called and must be first copied in
/// order to retain them. Returned references can safely be used by multiple
/// threads, to be processed in parallel.
///
/// Commands are parsed separately from data streams. To read a data stream,
/// open a [`DataReader`] from the returned [`DataStream`] with
/// [`DataStream::open`].
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
    input: UnsafeCell<Input<R>>,

    /// A buffer containing all of the current command and its sub-commands.
    command_buf: Vec<u8>,
    /// The current selection in `command_buf`, which is being processed.
    cursor: Span,

    /// Whether a `DataReader` has been opened for reading. This guards
    /// `DataStream::open`, to ensure that only one `DataReader` can be opened
    /// per call to `Parser::next`.
    data_opened: AtomicBool,
    /// The state for reading a data stream.
    ///
    /// It may only be mutated under `&` within the `DataReader`.
    data_state: UnsafeCell<DataState>,

    /// Whether the previous command ended with an optional LF.
    has_optional_lf: bool,
}

// SAFETY: All `UnsafeCell` fields are guaranteed only be modified by a single
// thread. When mutation occurs under an `&`-reference, it is atomically guarded
// by `Parser::data_opened` to ensure it can only happen by one thread. See the
// invariants of `Parser::input`.
unsafe impl<R> Sync for Parser<R> {}

/// A range of bytes within `command_buf`.
///
/// This is used instead of directly slicing `command_buf` so that ranges can be
/// safely saved while the buffer is still being grown. After the full command
/// has been read (except for a data stream, which is read separately),
/// `command_buf` will not change until the next call to `next`, and slices can
/// be made and returned to the caller.
#[derive(Copy, Clone, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

/// Input for a fast-export stream.
struct Input<R> {
    /// Reader for the fast-export stream.
    r: R,
    /// Whether the reader has reached EOF.
    eof: bool,
}

/// Metadata for the current data stream. It can be opened for reading with
/// [`DataStream::open`].
#[derive(Clone)]
pub struct DataStream<'a, B, R> {
    header: DataHeader<B>,
    parser: &'a Parser<R>,
}

/// An exclusive handle for reading the current data stream.
pub struct DataReader<'a, R> {
    parser: &'a Parser<R>,
    marker: PhantomData<&'a mut Parser<R>>,
}

/// Spanned version of [`DataHeader`].
#[derive(Clone, Copy, Debug)]
enum DataSpan {
    Counted { len: u64 },
    Delimited { delim: Span },
}

/// The state for reading a data stream. `Parser::data_opened` ensures only one
/// `DataReader` is ever created for this parser at a time. The header is
/// stored, instead of using the one in `DataStream`, so that the data stream
/// can be skipped when the caller does not finish reading it.
#[derive(Debug)]
struct DataState {
    /// The header information for the data stream.
    header: DataSpan,
    /// Whether the data stream has been read to completion.
    finished: bool,
    /// Whether the data reader has been closed.
    closed: bool,
    /// The number of bytes read from the data stream.
    len_read: u64,
    /// A buffer for reading lines in delimited data.
    line_buf: Vec<u8>,
    /// The offset into `line_buf`, at which reading begins.
    line_offset: usize,
}

/// An error from opening a [`DataReader`].
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Hash)]
#[error("data stream already opened for reading")]
pub struct DataReaderError;

/// An error from parsing a fast-export stream, including IO errors.
#[derive(Debug, Error)]
#[error(transparent)]
pub enum StreamError {
    Parse(#[from] ParseError),
    Io(#[from] io::Error),
}

/// An error from parsing a fast-export stream.
#[derive(Clone, Error, PartialEq, Eq, Hash)]
#[error("{kind}: {:?}", line.as_bstr())]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub line: Vec<u8>,
}

/// A kind of error from parsing a command in a fast-export stream.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Hash)]
pub enum ParseErrorKind {
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
    /// The data stream was not read to completion by [`DataReader`] before the
    /// next command was parsed. If you want to close it early, call
    /// [`DataReader::skip_rest`].
    #[error("data stream was not read to the end")]
    UnfinishedData,
    /// The data reader has already been closed by [`DataReader::close`].
    #[error("data reader is closed")]
    ClosedData,
    /// The length for a counted `data` command is not a valid integer.
    #[error("invalid data length")]
    InvalidDataLength,
    /// EOF was reached before reading the complete counted data stream.
    #[error("unexpected EOF in data stream")]
    DataUnexpectedEof,
    /// fast-import accepts opening, but not closing, delimiters that contain
    /// NUL, so it will never terminate such data. This error detects that
    /// early.
    #[error("data delimiter contains NUL ('\\0')")]
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

use ParseErrorKind as ErrorKind;

impl<R: BufRead> Parser<R> {
    /// Creates a new `Parser` for reading the given input.
    #[inline]
    pub fn new(input: R) -> Self {
        Parser {
            input: UnsafeCell::new(Input {
                r: input,
                eof: false,
            }),
            command_buf: Vec::new(),
            cursor: Span::from(0..0),
            data_opened: AtomicBool::new(false),
            data_state: UnsafeCell::new(DataState {
                header: DataSpan::Counted { len: 0 },
                finished: false,
                closed: false,
                len_read: 0,
                line_buf: Vec::new(),
                line_offset: 0,
            }),
            has_optional_lf: false,
        }
    }

    /// Parses the next command in the fast-export stream.
    ///
    /// The parsed commands borrow from the parser's buffer, so need to be
    /// copied before calling `next` again to retain them.
    ///
    // Corresponds to the loop in `cmd_fast_import` in fast-import.c.
    pub fn next(&mut self) -> Result<Command<'_, &[u8], R>> {
        // Finish reading the previous data stream, if the user didn't.
        if !self.data_state.get_mut().finished {
            // SAFETY: We have `&mut`-access to all of `Parser`.
            unsafe { self.skip_data()? };
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
            Err(self.err(ErrorKind::UnexpectedBlank))
        } else {
            Err(self.err(ErrorKind::UnsupportedCommand))
        }
    }

    // Corresponds to `parse_new_blob` in fast-import.c.
    fn parse_blob(&mut self) -> Result<Command<'_, &[u8], R>> {
        self.bump_command()?;
        let mark = self.parse_mark()?;
        let original_oid = self.parse_original_oid()?;
        let data = self.parse_data()?;

        Ok(Command::Blob(Blob {
            mark,
            original_oid: original_oid.map(|oid| OriginalOid {
                oid: self.slice_cmd(oid),
            }),
            data: DataStream {
                header: data.slice(self),
                parser: self,
            },
        }))
    }

    // Corresponds to `parse_new_commit` in fast-import.c.
    fn parse_commit(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_new_tag` in fast-import.c.
    fn parse_tag(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_reset_branch` in fast-import.c.
    fn parse_reset(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_ls` in fast-import.c.
    fn parse_ls(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_get_mark` in fast-import.c.
    fn parse_get_mark(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_checkpoint` in fast-import.c.
    fn parse_checkpoint(&mut self) -> Result<Command<'_, &[u8], R>> {
        self.has_optional_lf = true;
        Ok(Command::Checkpoint)
    }

    // Corresponds to `parse_alias` in fast-import.c.
    fn parse_alias(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_progress` in fast-import.c.
    fn parse_progress(&mut self) -> Result<Command<'_, &[u8], R>> {
        self.has_optional_lf = true;
        Ok(Command::Progress(Progress {
            message: self.line_remaining(),
        }))
    }

    // Corresponds to `parse_feature` in fast-import.c.
    fn parse_feature(&mut self) -> Result<Command<'_, &[u8], R>> {
        todo!()
    }

    // Corresponds to `parse_option` in fast-import.c.
    fn parse_option(&mut self) -> Result<Command<'_, &[u8], R>> {
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
    fn parse_mark(&mut self) -> Result<Option<Mark>> {
        if self.eat_prefix(b"mark :") {
            let mark =
                parse_u64(self.line_remaining()).ok_or_else(|| self.err(ErrorKind::InvalidMark))?;
            self.bump_command()?;
            let mark = Mark::new(mark).ok_or_else(|| self.err(ErrorKind::ZeroMark))?;
            Ok(Some(mark))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_original_identifier` in fast-import.c.
    fn parse_original_oid(&mut self) -> Result<Option<Span>> {
        if self.eat_prefix(b"original-oid ") {
            let original_oid = self.cursor;
            self.bump_command()?;
            Ok(Some(original_oid))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_and_store_blob` in fast-import.c.
    fn parse_data(&mut self) -> Result<DataSpan> {
        if !self.eat_prefix(b"data ") {
            return Err(self.err(ErrorKind::ExpectedDataCommand));
        }
        let header = if self.eat_prefix(b"<<") {
            let delim = self.cursor;
            if delim.is_empty() {
                return Err(self.err(ErrorKind::EmptyDelim));
            } else if self.slice_cmd(delim).contains(&b'\0') {
                return Err(self.err(ErrorKind::DataDelimContainsNul));
            }
            DataSpan::Delimited { delim }
        } else {
            let len = parse_u64(self.line_remaining())
                .ok_or_else(|| self.err(ErrorKind::InvalidDataLength))?;
            DataSpan::Counted { len }
        };
        self.data_opened.store(false, Ordering::Release);
        let data_state = self.data_state.get_mut();
        data_state.header = header;
        data_state.finished = matches!(data_state.header, DataSpan::Counted { len: 0 });
        data_state.closed = false;
        data_state.len_read = 0;
        self.has_optional_lf = true;
        Ok(header)
    }

    /// Reads from the current data stream into the given buffer.
    ///
    /// # Safety
    ///
    /// The caller must guarantee exclusive mutable access to all of the
    /// `UnsafeCell` fields in `Parser` (`Parser::input` and
    /// `Parser::data_state`). See the invariants in `Parser::input`.
    unsafe fn read_data(&self, buf: &mut [u8]) -> Result<usize> {
        // SAFETY: Guaranteed by caller.
        let (input, s) = unsafe { (&mut *self.input.get(), &mut *self.data_state.get()) };
        if s.closed {
            return Err(self.err(ErrorKind::ClosedData));
        }
        if buf.is_empty() || s.finished {
            return Ok(0);
        }
        match s.header {
            DataSpan::Counted { len } => {
                if input.eof {
                    return Err(self.err(ErrorKind::DataUnexpectedEof));
                }
                let end = usize::try_from(len - s.len_read)
                    .unwrap_or(usize::MAX)
                    .min(buf.len());
                let n = input.r.read(&mut buf[..end])?;
                debug_assert!(n <= end, "misbehaving BufRead implementation");
                s.len_read += n as u64;
                if s.len_read >= len {
                    debug_assert!(s.len_read == len, "read too many bytes");
                    s.finished = true;
                }
                Ok(n)
            }
            DataSpan::Delimited { delim } => {
                if s.line_offset >= s.line_buf.len() {
                    if input.eof {
                        return Err(self.err(ErrorKind::UnterminatedData));
                    }
                    s.line_buf.clear();
                    s.line_offset = 0;
                    let line = input.read_line(&mut s.line_buf)?;
                    if line.slice(&s.line_buf) == delim.slice(&self.command_buf) {
                        s.finished = true;
                        return Ok(0);
                    }
                }
                let off = s.line_offset;
                let n = (s.line_buf.len() - off).min(buf.len());
                buf[..n].copy_from_slice(&s.line_buf[off..off + n]);
                s.line_offset += n;
                s.len_read += n as u64;
                Ok(n)
            }
        }
    }

    /// Reads to the end of the data stream without consuming it.
    ///
    /// # Safety
    ///
    /// Same as `Parser::read_data`.
    unsafe fn skip_data(&self) -> Result<u64> {
        // SAFETY: Guaranteed by caller.
        let (input, s) = unsafe { (&mut *self.input.get(), &mut *self.data_state.get()) };
        if s.closed {
            return Err(self.err(ErrorKind::ClosedData));
        }
        if s.finished {
            return Ok(0);
        }
        let start_len = s.len_read;
        match s.header {
            DataSpan::Counted { len } => {
                while s.len_read < len {
                    let buf = input.r.fill_buf()?;
                    if buf.is_empty() {
                        input.eof = true;
                        return Err(self.err(ErrorKind::DataUnexpectedEof));
                    }
                    let n = usize::try_from(len - s.len_read)
                        .unwrap_or(usize::MAX)
                        .min(buf.len());
                    input.r.consume(n);
                    s.len_read += n as u64;
                }
            }
            DataSpan::Delimited { delim } => {
                let delim = delim.slice(&self.command_buf);
                loop {
                    if input.eof {
                        return Err(self.err(ErrorKind::UnterminatedData));
                    }
                    s.line_buf.clear();
                    let line = input.read_line(&mut s.line_buf)?;
                    if line.slice(&s.line_buf) == delim {
                        break;
                    }
                    s.len_read += s.line_buf.len() as u64;
                }
            }
        }
        s.finished = true;
        Ok(s.len_read - start_len)
    }

    /// Reads a line from input into `self.line_buf`, stripping the LF
    /// delimiter, and skipping any comment lines that start with `#`. Lines may
    /// contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `read_next_command` in fast-import.c.
    fn bump_command(&mut self) -> io::Result<()> {
        if self.input.get_mut().eof {
            self.cursor.start = self.cursor.end;
            return Ok(());
        }
        loop {
            self.cursor = self.input.get_mut().read_line(&mut self.command_buf)?;
            if !self.slice_cmd(self.cursor).starts_with(b"#") || self.input.get_mut().eof {
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

    /// Creates a parse error at the cursor.
    #[inline(never)]
    fn err(&self, kind: ParseErrorKind) -> StreamError {
        // TODO: Improve error reporting:
        // - Use an error reporting library like miette or Ariadne.
        // - Track line in data stream.
        // - Would it be useful to include the error range for `io::Error`s from
        //   `self.input.r`? Such errors seem unlikely to be related to the
        //   syntax or semantics of the data. It could possibly be tracked by
        //   recording the size of the buffer with `fill_buf` before calling
        //   `read`.
        let line = if self.data_opened.load(Ordering::Acquire) {
            b"<<parsing data stream>>".to_vec()
        } else {
            self.line_remaining().to_owned()
        };
        StreamError::Parse(ParseError { kind, line })
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

impl Span {
    #[inline(always)]
    fn slice<'a, B: AsRef<[u8]>>(&self, bytes: &'a B) -> &'a [u8] {
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

impl<R: BufRead> Input<R> {
    /// Reads a line from `input` into `buf`, stripping the LF delimiter. Lines
    /// may contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `strbuf_getline_lf` in strbuf.c.
    #[inline(always)]
    fn read_line(&mut self, buf: &mut Vec<u8>) -> io::Result<Span> {
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
        Ok(Span::from(start..end))
    }
}

impl<'a, B, R: BufRead> DataStream<'a, B, R> {
    /// Opens this data stream for reading. Only one instance of [`DataReader`]
    /// can exist at a time.
    #[inline]
    pub fn open(&self) -> result::Result<DataReader<'a, R>, DataReaderError> {
        // Check that `data_opened` was previously false and set it to true.
        if !self.parser.data_opened.swap(true, Ordering::Acquire) {
            Ok(DataReader {
                parser: self.parser,
                marker: PhantomData,
            })
        } else {
            Err(DataReaderError)
        }
    }

    /// Gets the header for this data stream.
    #[inline(always)]
    pub fn header(&self) -> &DataHeader<B> {
        &self.header
    }
}

impl<B: Debug, R> Debug for DataStream<'_, B, R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataStream")
            .field("header", &self.header)
            .finish()
    }
}

impl<B: PartialEq, R> PartialEq for DataStream<'_, B, R> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.header == other.header && ptr::eq(self.parser as _, other.parser as _)
    }
}

impl<B: Eq, R> Eq for DataStream<'_, B, R> {}

impl<R: BufRead> DataReader<'_, R> {
    /// Reads from the data stream into the given buffer. Identical to
    /// [`DataReader::read`], but returns [`ParseError`].
    #[inline]
    pub fn read_next(&mut self, buf: &mut [u8]) -> Result<usize> {
        // SAFETY: We have exclusive mutable access to all of the `UnsafeCell`
        // fields, because we are in the single instance of `DataReader`, and
        // its construction was guarded by `DataState::reading_data`. See the
        // invariants in `Parser::input`.
        unsafe { self.parser.read_data(buf) }
    }

    /// Skips reading the rest of the data stream and returns the number of
    /// bytes skipped.
    ///
    /// Use this when only reading some of the data stream, otherwise the next
    /// call to [`Parser::next`] will return an error. It is not recommended to
    /// use this when you intend to read the whole stream.
    ///
    /// Unlike [`DataReader::read_next`], this returns a `u64`, because the
    /// length skipped can be larger than `usize` on 32-bit platforms, as it
    /// does not need to all fit in memory at once.
    #[inline]
    pub fn skip_rest(&mut self) -> Result<u64> {
        // SAFETY: See `DataReader::read_next`.
        unsafe { self.parser.skip_data() }
    }

    /// Closes the data stream and returns an error when it was not read to
    /// completion.
    #[inline]
    pub fn close(&mut self) -> Result<()> {
        // SAFETY: See `DataReader::read_next`.
        let data_state = unsafe { &mut *self.parser.data_state.get() };
        if data_state.closed {
            Err(self.parser.err(ErrorKind::ClosedData))
        } else if data_state.finished {
            data_state.closed = true;
            Ok(())
        } else {
            Err(self.parser.err(ErrorKind::UnfinishedData))
        }
    }

    /// Returns the number of bytes read from the data stream.
    #[inline]
    pub fn len_read(&self) -> u64 {
        // SAFETY: See `DataReader::read_next`.
        let data_state = unsafe { &*self.parser.data_state.get() };
        data_state.len_read
    }

    /// Returns whether the data stream has been read to completion.
    #[inline]
    pub fn finished(&self) -> bool {
        // SAFETY: See `DataReader::read_next`.
        let data_state = unsafe { &*self.parser.data_state.get() };
        data_state.finished
    }
}

/// Identical to [`DataReader::read_next`], but converts [`ParseError`] to
/// [`io::Error`].
impl<R: BufRead> Read for DataReader<'_, R> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_next(buf).map_err(|err| err.into())
    }
}

impl DataSpan {
    #[inline(always)]
    fn slice<'a, R: BufRead>(&self, parser: &'a Parser<R>) -> DataHeader<&'a [u8]> {
        match *self {
            DataSpan::Counted { len } => DataHeader::Counted { len },
            DataSpan::Delimited { delim } => DataHeader::Delimited {
                delim: parser.slice_cmd(delim),
            },
        }
    }
}

impl Debug for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParseError")
            .field("kind", &self.kind)
            .field("line", &self.line.as_bstr())
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
            StreamError::Io(err) => err,
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

#[cfg(test)]
mod tests {
    use std::io::Read;

    use bstr::ByteSlice;

    use crate::{
        command::{Command, DataHeader, Done, Mark, OriginalOid},
        parse::Parser,
    };

    #[test]
    fn parse_counted_blob_read_stream() {
        parse_counted_blob(true, true);
        parse_counted_blob(true, false);
    }

    #[test]
    fn parse_counted_blob_skip_stream() {
        parse_counted_blob(false, true);
        parse_counted_blob(false, false);
    }

    #[test]
    fn parse_delimited_blob_read_stream() {
        parse_delimited_blob(true, true);
        parse_delimited_blob(true, false);
    }

    #[test]
    fn parse_delimited_blob_skip_stream() {
        parse_delimited_blob(false, true);
        parse_delimited_blob(false, false);
    }

    fn parse_counted_blob(read_all: bool, optional_lf: bool) {
        let mut input = b"blob\nmark :42\noriginal-oid 3141592653589793238462643383279502884197\ndata 14\nHello, world!\n".to_vec();
        if optional_lf {
            input.push(b'\n');
        }
        let mut input = &input[..];
        let mut parser = Parser::new(&mut input);

        let command = parser.next().unwrap();
        let Command::Blob(blob) = command else {
            panic!("not a blob: {command:?}");
        };
        assert_eq!(blob.mark, Some(Mark::new(42).unwrap()));
        assert_eq!(
            blob.original_oid,
            Some(OriginalOid {
                oid: &b"3141592653589793238462643383279502884197"[..],
            }),
        );
        assert_eq!(blob.data.header, DataHeader::Counted { len: 14 });

        if read_all {
            let mut r = blob.data.open().unwrap();
            let mut buf = Vec::new();
            if let Err(err) = r.read_to_end(&mut buf) {
                panic!("read to end: {err}\nbuffer: {:?}", buf.as_bstr());
            }
            assert_eq!(buf.as_bstr(), b"Hello, world!\n".as_bstr(), "data stream");
        }

        assert_eq!(parser.next().unwrap(), Command::Done(Done::Eof));
        assert!(parser.input.get_mut().r.is_empty());
    }

    fn parse_delimited_blob(read_all: bool, optional_lf: bool) {
        let mut input = b"blob\nmark :42\noriginal-oid 3141592653589793238462643383279502884197\ndata <<EOF\nHello, world!\nEOF\n".to_vec();
        if optional_lf {
            input.push(b'\n');
        }
        let mut input = &input[..];
        let mut parser = Parser::new(&mut input);

        let command = parser.next().unwrap();
        let Command::Blob(blob) = command else {
            panic!("not a blob: {command:?}");
        };
        assert_eq!(blob.mark, Some(Mark::new(42).unwrap()));
        assert_eq!(
            blob.original_oid,
            Some(OriginalOid {
                oid: &b"3141592653589793238462643383279502884197"[..],
            }),
        );
        assert_eq!(
            blob.data.header,
            DataHeader::Delimited { delim: &b"EOF"[..] },
        );

        if read_all {
            let mut r = blob.data.open().unwrap();
            let mut buf = Vec::new();
            if let Err(err) = r.read_to_end(&mut buf) {
                panic!("read to end: {err}\nbuffer: {:?}", buf.as_bstr());
            }
            assert_eq!(buf.as_bstr(), b"Hello, world!\n".as_bstr(), "data stream");
        }

        assert_eq!(parser.next().unwrap(), Command::Done(Done::Eof));
        assert!(parser.input.get_mut().r.is_empty());
    }
}
