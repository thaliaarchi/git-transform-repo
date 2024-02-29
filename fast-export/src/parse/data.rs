// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    io::{self, BufRead, Read},
    sync::atomic::{AtomicBool, Ordering},
};

use thiserror::Error;

use crate::parse::{DataSpan, PResult, Parser};

/// An exclusive handle for reading the current data stream.
pub struct DataReader<'a, R> {
    parser: &'a Parser<R>,
}

/// The state for reading a data stream. `Parser::data_opened` ensures only one
/// `DataReader` is ever created for this parser at a time. The header is
/// stored, instead of using the one in `Blob::data_header`, so that the data
/// stream can be skipped when the caller does not finish reading it.
#[derive(Debug)]
pub(super) struct DataState {
    /// The header information for the data stream.
    pub(super) header: DataSpan,
    /// Whether the data stream has been read to completion.
    pub(super) finished: bool,
    /// Whether the data reader has been closed.
    pub(super) closed: bool,
    /// The number of bytes read from the data stream.
    pub(super) len_read: u64,
    /// A buffer for reading lines in delimited data.
    pub(super) line_buf: Vec<u8>,
    /// The offset into `line_buf`, at which reading begins.
    pub(super) line_offset: usize,
}

/// An error from opening a [`DataReader`].
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Hash)]
pub enum DataReaderError {
    /// A data stream can only be opened once.
    #[error("data stream already opened for reading")]
    AlreadyOpened,
    /// The data stream was not read to completion by [`DataReader`] before the
    /// next command was parsed. If you want to close it early, call
    /// [`DataReader::skip_rest`].
    #[error("data stream was not read to the end")]
    Unfinished,
    /// The data reader has already been closed by [`DataReader::close`].
    #[error("data reader is closed")]
    Closed,
}

impl<'a, R: BufRead> DataReader<'a, R> {
    /// Opens the current data stream for reading. Only one instance of
    /// [`DataReader`] can exist at a time.
    #[inline]
    pub(crate) fn open(parser: &'a Parser<R>) -> PResult<DataReader<'a, R>> {
        // Check that `data_opened` was previously false and set it to true.
        if !parser.data_opened.swap(true, Ordering::Acquire) {
            Ok(DataReader { parser })
        } else {
            Err(DataReaderError::AlreadyOpened.into())
        }
    }

    /// Reads from the data stream into the given buffer. Identical to
    /// [`DataReader::read`], but returns [`ParseError`](super::ParseError).
    #[inline]
    pub fn read_next(&mut self, buf: &mut [u8]) -> PResult<usize> {
        // SAFETY: We have exclusive mutable access to all of the `UnsafeCell`
        // fields, because we are in the single instance of `DataReader`, and
        // its construction was guarded by `DataState::reading_data`. See the
        // invariants in `Parser::input`.
        let (input, data_state) = unsafe {
            (
                &mut *self.parser.input.get(),
                &mut *self.parser.data_state.get(),
            )
        };
        input.read_data(buf, data_state, &self.parser.command_buf)
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
    pub fn skip_rest(&mut self) -> PResult<u64> {
        // SAFETY: See `DataReader::read_next`.
        let (input, data_state) = unsafe {
            (
                &mut *self.parser.input.get(),
                &mut *self.parser.data_state.get(),
            )
        };
        input.skip_data(data_state, &self.parser.command_buf)
    }

    /// Closes the data stream and returns an error when it was not read to
    /// completion.
    #[inline]
    pub fn close(&mut self) -> PResult<()> {
        // SAFETY: See `DataReader::read_next`.
        let data_state = unsafe { &mut *self.parser.data_state.get() };
        if data_state.closed {
            Err(DataReaderError::Closed.into())
        } else if data_state.finished {
            data_state.closed = true;
            Ok(())
        } else {
            Err(DataReaderError::Unfinished.into())
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
        assert_eq!(blob.data_header, DataHeader::Counted { len: 14 });

        if read_all {
            let mut r = blob.open().unwrap();
            let mut buf = Vec::new();
            if let Err(err) = r.read_to_end(&mut buf) {
                panic!("read to end: {err}\nbuffer: {:?}", buf.as_bstr());
            }
            assert_eq!(buf.as_bstr(), b"Hello, world!\n".as_bstr(), "data stream");
        }

        assert_eq!(parser.next().unwrap(), Command::Done(Done::Eof));
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
            blob.data_header,
            DataHeader::Delimited { delim: &b"EOF"[..] },
        );

        if read_all {
            let mut r = blob.open().unwrap();
            let mut buf = Vec::new();
            if let Err(err) = r.read_to_end(&mut buf) {
                panic!("read to end: {err}\nbuffer: {:?}", buf.as_bstr());
            }
            assert_eq!(buf.as_bstr(), b"Hello, world!\n".as_bstr(), "data stream");
        }

        assert_eq!(parser.next().unwrap(), Command::Done(Done::Eof));
    }
}
