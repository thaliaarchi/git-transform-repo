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
    parse::{PResult, Parser, Span},
};

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
    pub(super) header: DataHeader<Span>,
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

impl<R: BufRead> Read for DataReader<'_, R> {
    /// Identical to [`DataReader::read_next`], but converts [`ParseError`](super::ParseError)
    /// to [`io::Error`].
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_next(buf).map_err(|err| err.into())
    }

    #[inline]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        // SAFETY: See `DataReader::read_next`.
        let (input, data_state) = unsafe {
            (
                &mut *self.parser.input.get(),
                &mut *self.parser.data_state.get(),
            )
        };
        let n = input.read_data_to_end(buf, data_state.header.clone(), &self.parser.command_buf)?;
        data_state.finished = true;
        data_state.len_read += n as u64;
        Ok(n)
    }
}

impl DataState {
    #[inline(always)]
    pub(super) fn new() -> Self {
        DataState {
            header: DataHeader::Counted { len: 0 },
            finished: false,
            closed: false,
            len_read: 0,
            line_buf: Vec::new(),
            line_offset: 0,
        }
    }

    #[inline(always)]
    pub(super) fn set(&mut self, header: DataHeader<Span>, data_opened: &mut AtomicBool) {
        data_opened.store(false, Ordering::Release);
        self.finished = matches!(header, DataHeader::Counted { len: 0 });
        self.closed = false;
        self.len_read = 0;
        self.header = header;
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
                    [<parse_ $data_kind _blob>](Mode::$ModeVariant, true);
                    [<parse_ $data_kind _blob>](Mode::$ModeVariant, false);
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
