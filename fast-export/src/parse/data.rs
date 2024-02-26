use std::{
    fmt::{self, Debug, Formatter},
    io::{self, BufRead, Read},
    marker::PhantomData,
    ptr, result,
    sync::atomic::{AtomicBool, Ordering},
};

use thiserror::Error;

use crate::{
    command::DataHeader,
    parse::{ParseErrorKind as ErrorKind, Parser, Result, Span},
};

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
pub(super) enum DataSpan {
    Counted { len: u64 },
    Delimited { delim: Span },
}

/// The state for reading a data stream. `Parser::data_opened` ensures only one
/// `DataReader` is ever created for this parser at a time. The header is
/// stored, instead of using the one in `DataStream`, so that the data stream
/// can be skipped when the caller does not finish reading it.
#[derive(Debug)]
pub(super) struct DataState {
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

impl<R: BufRead> Parser<R> {
    /// Reads from the current data stream into the given buffer. Exclusivity is
    /// not checked.
    ///
    /// # Safety
    ///
    /// The caller must guarantee exclusive mutable access to all of the
    /// `UnsafeCell` fields in `Parser` (`Parser::input` and
    /// `Parser::data_state`). See the invariants in `Parser::input`.
    unsafe fn read_data_cell(&self, buf: &mut [u8]) -> Result<usize> {
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
    #[inline(always)]
    pub(super) fn skip_data(&mut self) -> Result<u64> {
        // SAFETY: We have exclusive access from `&mut`.
        unsafe { self.skip_data_cell() }
    }

    /// Reads to the end of the data stream without consuming it. Exclusivity is
    /// not checked.
    ///
    /// # Safety
    ///
    /// Same as `Parser::read_data`.
    unsafe fn skip_data_cell(&self) -> Result<u64> {
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
    /// [`DataReader::read`], but returns [`ParseError`](super::ParseError).
    #[inline]
    pub fn read_next(&mut self, buf: &mut [u8]) -> Result<usize> {
        // SAFETY: We have exclusive mutable access to all of the `UnsafeCell`
        // fields, because we are in the single instance of `DataReader`, and
        // its construction was guarded by `DataState::reading_data`. See the
        // invariants in `Parser::input`.
        unsafe { self.parser.read_data_cell(buf) }
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
        unsafe { self.parser.skip_data_cell() }
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

/// Identical to [`DataReader::read_next`], but converts [`ParseError`](super::ParseError)
/// to [`io::Error`].
impl<R: BufRead> Read for DataReader<'_, R> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_next(buf).map_err(|err| err.into())
    }
}

impl DataSpan {
    #[inline(always)]
    pub(super) fn slice<'a, R: BufRead>(
        &self,
        parser: &'a Parser<R>,
    ) -> DataStream<'a, &'a [u8], R> {
        DataStream {
            header: match *self {
                DataSpan::Counted { len } => DataHeader::Counted { len },
                DataSpan::Delimited { delim } => DataHeader::Delimited {
                    delim: delim.slice(&parser.command_buf),
                },
            },
            parser,
        }
    }
}

impl DataState {
    #[inline(always)]
    pub(super) fn new() -> Self {
        DataState {
            header: DataSpan::Counted { len: 0 },
            finished: false,
            closed: false,
            len_read: 0,
            line_buf: Vec::new(),
            line_offset: 0,
        }
    }

    #[inline(always)]
    pub(super) fn set(&mut self, header: DataSpan, data_opened: &mut AtomicBool) {
        data_opened.store(false, Ordering::Release);
        self.header = header;
        self.finished = matches!(header, DataSpan::Counted { len: 0 });
        self.closed = false;
        self.len_read = 0;
    }

    #[inline(always)]
    pub(super) fn finished(&self) -> bool {
        self.finished
    }
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
