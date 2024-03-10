// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    io::{self, BufRead, Read},
    sync::atomic::{AtomicBool, Ordering},
};

use thiserror::Error;

use crate::{
    command::DataHeader,
    parse::{BufInput, PResult, Parser},
};

/// An exclusive handle for reading the current data stream.
pub struct DataReader<'a, R> {
    input: &'a BufInput<R>,
    data_state: &'a mut DataState,
}

/// The state for reading a data stream. `Parser::data_opened` ensures only one
/// `DataReader` is ever created for this parser at a time. The header is
/// stored, instead of using the one in `Blob::data_header`, so that the data
/// stream can be skipped when the caller does not finish reading it.
#[derive(Debug)]
pub(super) struct DataState {
    /// Whether the data stream has been read to completion.
    pub(super) finished: bool,
    /// Whether the data reader has been closed.
    pub(super) closed: bool,
    /// Whether the data stream is counted or delimited.
    pub(super) is_counted: bool,
    /// The number of bytes read from the data stream.
    pub(super) len_read: u64,

    /// The total number of bytes for counted data.
    pub(super) len: u64,

    /// The delimiter for delimited data.
    pub(super) delim: Vec<u8>,
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
            // SAFETY: We have exclusive access now, because we are in the
            // single instance of `DataReader`. See the invariants in
            // `Parser::input`.
            let data_state = unsafe { &mut *parser.data_state.get() };
            Ok(DataReader {
                input: &parser.input,
                data_state,
            })
        } else {
            Err(DataReaderError::AlreadyOpened.into())
        }
    }

    /// Reads from the data stream into the given buffer. Identical to
    /// [`DataReader::read`], but returns [`ParseError`](super::ParseError).
    #[inline]
    pub fn read_next(&mut self, buf: &mut [u8]) -> PResult<usize> {
        self.input.read_data(buf, self.data_state)
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
        self.input.skip_data(self.data_state)
    }

    /// Closes the data stream and returns an error when it was not read to
    /// completion.
    #[inline]
    pub fn close(&mut self) -> PResult<()> {
        if self.data_state.closed {
            Err(DataReaderError::Closed.into())
        } else if self.data_state.finished {
            self.data_state.closed = true;
            Ok(())
        } else {
            Err(DataReaderError::Unfinished.into())
        }
    }

    /// Returns the number of bytes read from the data stream.
    #[inline]
    pub fn len_read(&self) -> u64 {
        self.data_state.len_read
    }

    /// Returns whether the data stream has been read to completion.
    #[inline]
    pub fn finished(&self) -> bool {
        self.data_state.finished
    }
}

impl<R: BufRead> Read for DataReader<'_, R> {
    /// Identical to [`DataReader::read_next`], but converts [`ParseError`](super::ParseError)
    /// to [`io::Error`].
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_next(buf).map_err(|err| err.into())
    }

    #[inline]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let n = self
            .input
            .read_data_to_end(self.data_state.as_header(), buf)?;
        self.data_state.finished = true;
        self.data_state.len_read += n as u64;
        Ok(n)
    }
}

impl DataState {
    #[inline(always)]
    pub fn new() -> Self {
        DataState {
            finished: true,
            closed: true,
            is_counted: false,
            len_read: 0,
            len: 0,
            delim: Vec::new(),
            line_buf: Vec::new(),
            line_offset: 0,
        }
    }

    #[inline(always)]
    pub fn init(&mut self, header: &DataHeader<&[u8]>, data_opened: &AtomicBool) {
        data_opened.store(false, Ordering::Release);
        self.finished = false;
        self.closed = false;
        self.len_read = 0;
        match *header {
            DataHeader::Counted { len } => {
                self.is_counted = true;
                self.len = len;
            }
            DataHeader::Delimited { delim } => {
                self.is_counted = false;
                delim.clone_into(&mut self.delim);
                self.line_buf.clear();
                self.line_offset = 0;
            }
        }
    }

    #[inline(always)]
    pub fn as_header(&self) -> DataHeader<&[u8]> {
        if self.is_counted {
            DataHeader::Counted { len: self.len }
        } else {
            DataHeader::Delimited { delim: &self.delim }
        }
    }

    #[inline(always)]
    pub fn finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use bstr::ByteSlice;
    use paste::paste;

    use crate::{
        command::{Command, DataHeader, Done, Mark, OriginalOid},
        parse::{DataReaderError, Parser, StreamError},
    };

    enum Mode {
        NoOpen,
        ReadOnce,
        ReadOnceSkip,
        SkipAll,
        ReadToEnd,
    }

    macro_rules! test_parse_blob(($data_kind:ident, [$($ModeVariant:ident $mode:ident),+ $(,)?]) => {
        paste! {
            $(
                #[test]
                fn [<parse_ $data_kind _blob_ $mode>]() {
                    [<parse_ $data_kind _blob>](Mode::$ModeVariant, false);
                }

                #[test]
                fn [<parse_ $data_kind _blob_ $mode _optional_lf>]() {
                    [<parse_ $data_kind _blob>](Mode::$ModeVariant, true);
                }
            )+
        }
    });

    test_parse_blob!(counted, [NoOpen no_open, ReadOnce read_once, ReadOnceSkip read_once_skip, SkipAll skip_all, ReadToEnd read_to_end]);
    test_parse_blob!(delimited, [NoOpen no_open, ReadOnce read_once, ReadOnceSkip read_once_skip, SkipAll skip_all, ReadToEnd read_to_end]);

    fn parse_counted_blob(mode: Mode, optional_lf: bool) {
        parse_blob(
            b"blob\nmark :42\noriginal-oid 3141592653589793238462643383279502884197\ndata 14\nHello, world!\n",
            DataHeader::Counted { len: 14 },
            mode,
            optional_lf,
        )
    }

    fn parse_delimited_blob(mode: Mode, optional_lf: bool) {
        parse_blob(
            b"blob\nmark :42\noriginal-oid 3141592653589793238462643383279502884197\ndata <<EOF\nHello, world!\nEOF\n",
            DataHeader::Delimited { delim: b"EOF" },
            mode,
            optional_lf,
        )
    }

    fn parse_blob(
        input: &'static [u8],
        header: DataHeader<&'static [u8]>,
        mode: Mode,
        optional_lf: bool,
    ) {
        let mut input = input.to_vec();
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
        assert_eq!(blob.data_header, header);

        match mode {
            Mode::NoOpen => {}
            Mode::ReadOnce => {
                let mut r = blob.open().unwrap();
                let mut b = [0; 1];
                assert_eq!(r.read(&mut b).unwrap(), 1, "read");
                assert_eq!(b, [b'H']);
                match r.close() {
                    Err(StreamError::DataReader(DataReaderError::Unfinished)) => {}
                    res => panic!("close: {res:?}"),
                }
                match parser.next() {
                    Err(StreamError::DataReader(DataReaderError::Unfinished)) => {}
                    res => panic!("next: {res:?}"),
                }
                return;
            }
            Mode::ReadOnceSkip => {
                let mut r = blob.open().unwrap();
                let mut b = [0; 1];
                assert_eq!(r.read(&mut b).unwrap(), 1, "read");
                assert_eq!(b, [b'H']);
                assert_eq!(r.skip_rest().unwrap(), 13, "skip_rest");
            }
            Mode::SkipAll => {
                let mut r = blob.open().unwrap();
                assert_eq!(r.skip_rest().unwrap(), 14, "skip_rest");
            }
            Mode::ReadToEnd => {
                let mut r = blob.open().unwrap();
                let mut buf = Vec::new();
                match r.read_to_end(&mut buf) {
                    Ok(n) => {
                        assert_eq!(n, 14);
                        assert_eq!(buf.as_bstr(), b"Hello, world!\n".as_bstr(), "buf");
                    }
                    Err(err) => panic!("read to end: {err}\nbuffer: {:?}", buf.as_bstr()),
                }
            }
        }

        assert_eq!(parser.next().unwrap(), Command::Done(Done::Eof));
    }
}
