use std::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    io::{self, BufRead, Read},
    ops::Range,
    ptr, result, str,
    sync::atomic::{AtomicBool, Ordering},
};

use bstr::ByteSlice;
use thiserror::Error;

use crate::command::{Blob, Command, DataHeader, Done, Mark, OriginalOid, Progress};

type Result<T> = std::result::Result<T, ParseError>;

/// A zero-copy pull parser for fast-export streams.
///
/// It uses only as much memory as the single largest command command in the
/// stream. Any references to parsed data returned by the parser are invalidated
/// when [`Parser::next`] is called and must be first copied in order to retain
/// them. References to parsed data can safely be used by multiple threads, so
/// it can be processed in parallel.
pub struct Parser<R: BufRead> {
    /// The input reader being parsed. Mutation under `&` is guarded by
    /// `DataState::reading_data`.
    ///
    /// `input` is mutated in two separate ways: while reading a command with
    /// `Parser::next` or while reading the stream of a `data` command with
    /// `DataReader::read`. During `Parser::next`, no `&`-references to it are
    /// live, so it uses the usual notion of `&mut`. During `DataReader::read`,
    /// `&`-slices of `command_buf` have been returned to the caller, so
    /// `&mut`-access for the relevant fields is obtained via `UnsafeCell`. That
    /// is safely performed by ensuring only a single instance of `DataReader`
    /// can be constructed at a time by guarding its construction with
    /// `DataState::reading_data`.
    input: UnsafeCell<Input<R>>,

    /// A buffer containing all of the current command.
    command_buf: Vec<u8>,
    /// The current selection in `command_buf`, which is being processed.
    cursor: Span,

    /// The state for reading a `data` command stream.
    ///
    /// When it is `None`, mutation of other fields occurs as usual with `&mut`.
    /// When it is `Some`, only a single `DataReader` may active at a time, and
    /// through it, mutation of `UnsafeCell` fields occurs under `&`.
    data_state: Option<DataState>,
    /// A buffer for reading lines in delimited data. Mutation under `&` is
    /// guarded by `DataState::reading_data`. This is under `Parser`, instead of
    /// `DataState`, so it can be reused between `data` commands.
    delim_line_buf: UnsafeCell<Vec<u8>>,
}

// SAFETY: All `UnsafeCell` fields are guaranteed only be modified by a single
// thread. When mutation occurs under an `&`-reference, it is atomically guarded
// by `DataState::reading_data`. See the invariants of `Parser::input`.
unsafe impl<R: BufRead + Sync> Sync for Parser<R> {}

/// A range of bytes within `command_buf`.
///
/// This is used instead of directly slicing `command_buf` so that ranges can be
/// safely saved while the buffer is still being grown. After the full command
/// has been read (except for a `data` command stream, which is read
/// separately), `command_buf` will not change until the next call to `next`,
/// and slices can be made and returned to the caller.
#[derive(Copy, Clone, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

/// Input for a fast-export stream.
struct Input<R: BufRead> {
    /// Reader for the fast-export stream.
    r: R,
    /// Whether the reader has reached EOF.
    eof: bool,
}

#[derive(Clone)]
pub struct DataStream<'a, R: BufRead> {
    header: DataHeader<'a>,
    parser: &'a Parser<R>,
}

/// An exclusive handle for reading the current `data` command stream.
pub struct DataReader<'a, R: BufRead> {
    parser: &'a Parser<R>,
}

/// Spanned version of [`DataHeader`].
#[derive(Clone, Copy, Debug)]
enum DataSpan {
    Counted { len: u64 },
    Delimited { delim: Span },
}

/// The state for reading a `data` command stream. The header is stored, instead
/// of using the one in `DataReader`, to ensure the data is fully read, even
/// when the caller does not use it. `reading_data` ensures only one
/// `DataReader` is ever created for this parser at a time.
#[derive(Debug)]
struct DataState {
    /// Whether a `DataReader` has been opened. It guards mutation under `&` of
    /// `Parser::input`, `Parser::delim_line_buf`, `DataState::len_remaining`,
    /// and `DataState::delim_line_offset`,
    reading_data: AtomicBool,
    /// The header information for the current data stream. For counted data,
    /// the length is of the unread portion of the stream. Mutation under `&` is
    /// guarded by `reading_data`.
    header: UnsafeCell<DataSpan>,
    /// The offset into `Parse::delim_line_buf`, at which reading begins.
    /// Mutation under `&` is guarded by `DataState::reading_data`.
    delim_line_offset: UnsafeCell<usize>,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Hash)]
pub enum DataStreamError {
    #[error("data stream already opened for reading")]
    AlreadyOpened,
    #[error("data stream closed")]
    Closed,
}

/// An error from parsing a fast-export stream.
#[derive(Debug, Error)]
#[error(transparent)]
pub enum ParseError {
    Command(#[from] CommandError),
    Io(#[from] io::Error),
}

/// An error from parsing a command in a fast-export stream.
#[derive(Clone, Error, PartialEq, Eq, Hash)]
#[error("{kind}: {:?}", line.as_bstr())]
pub struct CommandError {
    pub kind: CommandErrorKind,
    pub line: Vec<u8>,
}

/// A kind of error from parsing a command in a fast-export stream.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Hash)]
pub enum CommandErrorKind {
    /// The mark is not a valid integer. fast-import allows more forms of
    /// ill-formatted integers.
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
    /// The data reader has already been closed by [`DataReader::skip_rest`].
    #[error("data reader has already been closed")]
    ClosedData,
    /// The length for a counted `data` command is not a valid integer.
    #[error("invalid data length")]
    InvalidDataLength,
    /// EOF was reached before reading the complete counted `data` stream.
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
}

use CommandErrorKind as ErrorKind;

impl<R: BufRead> Parser<R> {
    #[inline]
    pub fn new(input: R) -> Self {
        Parser {
            input: UnsafeCell::new(Input {
                r: input,
                eof: false,
            }),
            command_buf: Vec::new(),
            cursor: Span::from(0..0),
            data_state: None,
            delim_line_buf: UnsafeCell::new(Vec::new()),
        }
    }

    /// Parses the next command in the fast-export stream.
    ///
    /// The parsed commands borrow from the parser's buffer, so need to be
    /// copied if they are retained.
    ///
    // Corresponds to the loop in `cmd_fast_import` in fast-import.c.
    pub fn next(&mut self) -> Result<Command<'_, R>> {
        // Finish reading the previous data stream, if the user didn't.
        if self.data_state.is_some() {
            self.skip_data()?;
        }

        self.command_buf.clear();
        self.bump_line()?;
        if self.input.get_mut().eof {
            return Ok(Command::Done(Done::Eof));
        }
        if self.eat_all(b"blob") {
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
        } else if self.eat_all(b"checkpoint") {
            self.parse_checkpoint()
        } else if self.eat_all(b"done") {
            Ok(Command::Done(Done::Explicit))
        } else if self.eat_all(b"alias") {
            self.parse_alias()
        } else if self.eat_prefix(b"progress ") {
            self.parse_progress()
        } else if self.eat_prefix(b"feature ") {
            self.parse_feature()
        } else if self.eat_prefix(b"option ") {
            self.parse_option()
        } else {
            Err(self.err(ErrorKind::UnsupportedCommand))
        }
    }

    // Corresponds to `parse_new_blob` in fast-import.c.
    fn parse_blob(&mut self) -> Result<Command<'_, R>> {
        self.bump_line()?;
        let mark = self.parse_mark()?;
        let original_oid_span = self.parse_original_oid()?;
        let data_span = self.parse_data()?;

        Ok(Command::Blob(Blob {
            mark,
            original_oid: original_oid_span.map(|sp| OriginalOid { oid: self.get(sp) }),
            data: DataStream {
                header: data_span.expand(self),
                parser: self,
            },
        }))
    }

    // Corresponds to `parse_new_commit` in fast-import.c.
    fn parse_commit(&mut self) -> Result<Command<'_, R>> {
        todo!()
    }

    // Corresponds to `parse_new_tag` in fast-import.c.
    fn parse_tag(&mut self) -> Result<Command<'_, R>> {
        todo!()
    }

    // Corresponds to `parse_reset_branch` in fast-import.c.
    fn parse_reset(&mut self) -> Result<Command<'_, R>> {
        todo!()
    }

    // Corresponds to `parse_ls` in fast-import.c.
    fn parse_ls(&mut self) -> Result<Command<'_, R>> {
        todo!()
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob(&mut self) -> Result<Command<'_, R>> {
        todo!()
    }

    // Corresponds to `parse_get_mark` in fast-import.c.
    fn parse_get_mark(&mut self) -> Result<Command<'_, R>> {
        todo!()
    }

    // Corresponds to `parse_checkpoint` in fast-import.c.
    fn parse_checkpoint(&mut self) -> Result<Command<'_, R>> {
        todo!()
    }

    // Corresponds to `parse_alias` in fast-import.c.
    fn parse_alias(&mut self) -> Result<Command<'_, R>> {
        todo!()
    }

    // Corresponds to `parse_progress` in fast-import.c.
    fn parse_progress(&mut self) -> Result<Command<'_, R>> {
        let message_span = self.cursor;
        self.skip_optional_lf()?;
        Ok(Command::Progress(Progress {
            message: self.get(message_span),
        }))
    }

    // Corresponds to `parse_feature` in fast-import.c.
    fn parse_feature(&mut self) -> Result<Command<'_, R>> {
        todo!()
    }

    // Corresponds to `parse_option` in fast-import.c.
    fn parse_option(&mut self) -> Result<Command<'_, R>> {
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
            self.bump_line()?;
            let mark = Mark::new(mark).ok_or_else(|| self.err(ErrorKind::ZeroMark))?;
            Ok(Some(mark))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_original_identifier` in fast-import.c.
    fn parse_original_oid(&mut self) -> Result<Option<Span>> {
        if self.eat_prefix(b"original-oid ") {
            let original_oid_span = self.cursor;
            self.bump_line()?;
            Ok(Some(original_oid_span))
        } else {
            Ok(None)
        }
    }

    // Corresponds to `parse_and_store_blob` in fast-import.c.
    fn parse_data(&mut self) -> Result<DataSpan> {
        if !self.eat_prefix(b"data ") {
            return Err(self.err(ErrorKind::ExpectedDataCommand));
        }
        debug_assert!(self.data_state.is_none(), "unaccounted 'data' command");
        if self.eat_prefix(b"<<") {
            let delim_span = self.cursor;
            if delim_span.is_empty() {
                Err(self.err(ErrorKind::EmptyDelim))
            } else if self.get(delim_span).contains(&b'\0') {
                Err(self.err(ErrorKind::DataDelimContainsNul))
            } else {
                let header = DataSpan::Delimited { delim: delim_span };
                self.data_state = Some(DataState {
                    reading_data: AtomicBool::new(false),
                    header: UnsafeCell::new(header),
                    delim_line_offset: UnsafeCell::new(0),
                });
                Ok(header)
            }
        } else {
            let len = parse_u64(self.line_remaining())
                .ok_or_else(|| self.err(ErrorKind::InvalidDataLength))?;
            let header = DataSpan::Counted { len };
            self.data_state = Some(DataState {
                reading_data: AtomicBool::new(false),
                header: UnsafeCell::new(header),
                delim_line_offset: UnsafeCell::new(0), // Not applicable here
            });
            Ok(header)
        }
    }

    /// Reads from the current data stream into the given buffer.
    ///
    /// # Safety
    ///
    /// The caller must have exclusive mutable access to all of the `UnsafeCell`
    /// fields in `Parser` (`Parser::input`, `DataState::header`,
    /// `DataState::delim_line_offset`, and `Parser::delim_line_buf`). See the
    /// invariants in `Parser::input`.
    unsafe fn read_data(&self, buf: &mut [u8]) -> Result<usize> {
        let Some(data_state) = &self.data_state else {
            panic!("invalid data state");
        };
        // TODO: Evaluate this ordering.
        if !data_state.reading_data.load(Ordering::SeqCst) {
            return Err(self.err(ErrorKind::ClosedData));
        }
        // SAFETY: The caller must guarantee exclusive mutable access to these.
        let (input, header, delim_line_buf, delim_line_offset) = unsafe {
            (
                &mut *self.input.get(),
                &mut *data_state.header.get(),
                &mut *self.delim_line_buf.get(),
                &mut *data_state.delim_line_offset.get(),
            )
        };
        if buf.is_empty() {
            return Ok(0);
        }
        match header {
            DataSpan::Counted { len: len_remaining } => {
                if *len_remaining == 0 {
                    return Ok(0);
                }
                if input.eof {
                    return Err(self.err(ErrorKind::DataUnexpectedEof));
                }
                let n = usize::try_from(*len_remaining)
                    .unwrap_or(usize::MAX)
                    .min(buf.len());
                let n = input.r.read(&mut buf[..n])?;
                *len_remaining -= n as u64;
                Ok(n)
            }
            DataSpan::Delimited { delim: delim_span } => {
                if delim_span.is_empty() {
                    // An empty delimiter is repurposed to mean EOF has been
                    // reached, since empty delimiters are forbidden (see
                    // `ErrorKind::EmptyDelim`).
                    return Ok(0);
                }
                if *delim_line_offset >= delim_line_buf.len() && !input.eof {
                    delim_line_buf.clear();
                    *delim_line_offset = 0;
                    Parser::bump_line_raw(input, delim_line_buf)?;
                    if delim_line_buf == self.get(*delim_span) {
                        // Mark the delimiter as done.
                        *delim_span = Span::from(0..0);
                        return Ok(0);
                    }
                }
                if input.eof {
                    return Err(self.err(ErrorKind::UnterminatedData));
                }
                let offset = *delim_line_offset;
                let n = (delim_line_buf.len() - offset).min(buf.len());
                buf[..n].copy_from_slice(&delim_line_buf[offset..offset + n]);
                *delim_line_offset += n;
                Ok(n)
            }
        }
    }

    /// Reads to the end of the data stream without consuming it.
    fn skip_data(&mut self) -> Result<()> {
        let Some(data_state) = &mut self.data_state else {
            panic!("invalid data state");
        };
        // TODO: Evaluate this ordering.
        if data_state.reading_data.load(Ordering::SeqCst) {
            return Err(self.err(ErrorKind::UnfinishedData));
        }
        let input = self.input.get_mut();
        match *data_state.header.get_mut() {
            DataSpan::Counted {
                len: mut len_remaining,
            } => {
                while len_remaining > 0 {
                    let buf = input.r.fill_buf()?;
                    if buf.is_empty() {
                        input.eof = true;
                        return Err(self.err(ErrorKind::DataUnexpectedEof));
                    }
                    let n = usize::try_from(len_remaining)
                        .unwrap_or(usize::MAX)
                        .min(buf.len());
                    input.r.consume(n);
                    len_remaining -= n as u64;
                }
            }
            DataSpan::Delimited { delim: delim_span } => {
                let delim = &self.command_buf[Range::from(delim_span)];
                loop {
                    if input.eof {
                        return Err(self.err(ErrorKind::UnterminatedData));
                    }
                    let delim_line_buf = self.delim_line_buf.get_mut();
                    delim_line_buf.clear();
                    let line_span = Parser::bump_line_raw(input, delim_line_buf)?;
                    if &delim_line_buf[Range::from(line_span)] == delim {
                        break;
                    }
                }
            }
        }
        self.data_state = None;
        Ok(())
    }

    /// Reads a line from input into `self.line_buf`, stripping the LF
    /// delimiter, and skipping any comment lines that start with `#`. Lines may
    /// contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `read_next_command` in fast-import.c.
    fn bump_line(&mut self) -> io::Result<()> {
        while !self.input.get_mut().eof {
            self.cursor = Parser::bump_line_raw(self.input.get_mut(), &mut self.command_buf)?;
            match self.get(self.cursor) {
                [b'#', ..] => continue,
                _ => break,
            }
        }
        Ok(())
    }

    /// Reads a line from `input` into `buf`, stripping the LF delimiter. Lines
    /// may contain any bytes (including NUL), except for LF.
    ///
    // Corresponds to `strbuf_getline_lf` in strbuf.c.
    #[inline(always)]
    fn bump_line_raw(input: &mut Input<R>, buf: &mut Vec<u8>) -> io::Result<Span> {
        debug_assert!(!input.eof, "already at EOF");
        let start = buf.len();
        input.r.read_until(b'\n', buf)?;
        let mut end = buf.len();
        if let [.., b'\n'] = &buf[start..] {
            end -= 1;
        } else {
            // EOF is reached in `read_until` iff the delimiter is not included.
            input.eof = true;
        }
        Ok(Span::from(start..end))
    }

    // Corresponds to `skip_optional_lf` in fast-import.c.
    fn skip_optional_lf(&mut self) -> io::Result<()> {
        todo!()
    }

    /// Returns the text in the command at the cursor.
    #[inline(always)]
    fn get(&self, range: Span) -> &[u8] {
        &self.command_buf[Range::from(range)]
    }

    /// Returns the remainder of the line at the cursor.
    #[inline(always)]
    fn line_remaining(&self) -> &[u8] {
        self.get(self.cursor)
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
    fn eat_all(&mut self, b: &[u8]) -> bool {
        if self.line_remaining() == b {
            self.cursor.start = self.cursor.end;
            true
        } else {
            false
        }
    }

    /// Creates a parse error at the cursor.
    #[inline(never)]
    fn err(&self, kind: CommandErrorKind) -> ParseError {
        // TODO: Improve error reporting:
        // - Use an error reporting library like miette or Ariadne.
        // - Track line in data stream.
        // - Would it be useful to include the error range for `io::Error`s from
        //   `self.input.r`? Such errors seem unlikely to be related to the
        //   syntax or semantics of the data. It could possibly be tracked by
        //   recording the size of the buffer with `fill_buf` before calling
        //   `read`.
        let line = if self.data_state.is_none() {
            self.line_remaining().to_owned()
        } else {
            b"<<parsing data stream>>".to_vec()
        };
        ParseError::Command(CommandError { kind, line })
    }
}

impl<R: BufRead + Debug> Debug for Parser<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parser")
            .field("input", &self.input)
            .field("command_buf", &self.command_buf.as_bstr())
            .field("cursor", &self.cursor)
            .field("data_state", &self.data_state)
            .field("delim_line_buf", &self.delim_line_buf)
            .finish()
    }
}

impl Span {
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

impl<'a, R: BufRead> DataStream<'a, R> {
    pub fn open(&self) -> result::Result<DataReader<'a, R>, DataStreamError> {
        let Some(state) = &self.parser.data_state else {
            return Err(DataStreamError::Closed);
        };
        // TODO: Evaluate this ordering.
        match state
            .reading_data
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => Ok(DataReader {
                parser: self.parser,
            }),
            Err(_) => Err(DataStreamError::AlreadyOpened),
        }
    }

    #[inline(always)]
    pub fn header(&self) -> &DataHeader<'a> {
        &self.header
    }
}

impl<R: BufRead> Debug for DataStream<'_, R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataStream")
            .field("header", &self.header)
            .finish()
    }
}

impl<R: BufRead> PartialEq for DataStream<'_, R> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.header == other.header && ptr::eq(self.parser as _, other.parser as _)
    }
}

impl<R: BufRead> Eq for DataStream<'_, R> {}

impl<'a, R: BufRead> DataReader<'a, R> {
    /// Reads from this data reader into the given buffer. Identical to
    /// [`DataReader::read`], but returns [`CommandError`].
    pub fn read_next(&mut self, buf: &mut [u8]) -> Result<usize> {
        // SAFETY: We have exclusive mutable access to all of the `UnsafeCell`
        // fields, because we are in the single instance of `DataReader`, and
        // its construction was guarded by `DataState::reading_data`. See the
        // invariants in `Parser::input`.
        unsafe { self.parser.read_data(buf) }
    }

    /// Skip reading the rest of the data stream and close the reader. The data
    /// stream must be read to completion before the next call to
    /// [`Parser::next`], otherwise an error will be returned there. It is not
    /// recommended to use this when you intend to read the whole stream.
    pub fn skip_rest(&mut self) -> Result<()> {
        let Some(data_state) = &self.parser.data_state else {
            panic!("invalid data state");
        };
        // TODO: Evaluate this ordering.
        match data_state.reading_data.compare_exchange(
            true,
            false,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => Ok(()),
            Err(_) => Err(self.parser.err(ErrorKind::ClosedData)),
        }
    }
}

/// Identical to [`DataReader::read_next`], but converts [`CommandError`] to
/// [`io::Error`].
impl<'a, R: BufRead> Read for DataReader<'a, R> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_next(buf).map_err(|err| err.into())
    }
}

impl DataSpan {
    #[inline(always)]
    fn expand<'a, R: BufRead>(&self, parser: &'a Parser<R>) -> DataHeader<'a> {
        match *self {
            DataSpan::Counted { len } => DataHeader::Counted { len },
            DataSpan::Delimited { delim } => DataHeader::Delimited {
                delim: parser.get(delim),
            },
        }
    }
}

impl Debug for CommandError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandError")
            .field("kind", &self.kind)
            .field("line", &self.line.as_bstr())
            .finish()
    }
}

impl From<io::ErrorKind> for ParseError {
    #[inline]
    fn from(kind: io::ErrorKind) -> Self {
        ParseError::Io(kind.into())
    }
}

impl From<ParseError> for io::Error {
    #[inline]
    fn from(err: ParseError) -> Self {
        match err {
            ParseError::Command(err) => io::Error::new(io::ErrorKind::InvalidData, err),
            ParseError::Io(err) => err,
        }
    }
}

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
        parse_counted_blob(true);
    }

    #[test]
    fn parse_counted_blob_skip_stream() {
        parse_counted_blob(false);
    }

    #[test]
    fn parse_delimited_blob_read_stream() {
        parse_delimited_blob(true);
    }

    #[test]
    fn parse_delimited_blob_skip_stream() {
        parse_delimited_blob(false);
    }

    fn parse_counted_blob(read_all: bool) {
        let input = &mut &b"blob\nmark :42\noriginal-oid 3141592653589793238462643383279502884197\ndata 14\nHello, world!\n"[..];
        let mut parser = Parser::new(input);

        let command = parser.next().unwrap();
        let Command::Blob(blob) = command else {
            panic!("not a blob: {command:?}");
        };
        assert_eq!(blob.mark, Some(Mark::new(42).unwrap()));
        assert_eq!(
            blob.original_oid,
            Some(OriginalOid {
                oid: b"3141592653589793238462643383279502884197",
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
    }

    fn parse_delimited_blob(read_all: bool) {
        let input = &mut &b"blob\nmark :42\noriginal-oid 3141592653589793238462643383279502884197\ndata <<EOF\nHello, world!\nEOF\n"[..];
        let mut parser = Parser::new(input);

        let command = parser.next().unwrap();
        let Command::Blob(blob) = command else {
            panic!("not a blob: {command:?}");
        };
        assert_eq!(blob.mark, Some(Mark::new(42).unwrap()));
        assert_eq!(
            blob.original_oid,
            Some(OriginalOid {
                oid: b"3141592653589793238462643383279502884197",
            }),
        );
        assert_eq!(blob.data.header, DataHeader::Delimited { delim: b"EOF" });

        if read_all {
            let mut r = blob.data.open().unwrap();
            let mut buf = Vec::new();
            if let Err(err) = r.read_to_end(&mut buf) {
                panic!("read to end: {err}\nbuffer: {:?}", buf.as_bstr());
            }
            assert_eq!(buf.as_bstr(), b"Hello, world!\n".as_bstr(), "data stream");
        }

        assert_eq!(parser.next().unwrap(), Command::Done(Done::Eof));
    }
}
