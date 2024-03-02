// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    fmt::{self, Debug, Formatter},
    ops::Range,
};

use crate::command::MapBytes;

/// Converts the type to a byte slice for slicing with a `Span`.
pub(super) trait Sliceable<'a> {
    fn as_slice(&'a self) -> &'a [u8];
}

/// A range of bytes within `Parser::command_buf`.
///
/// This is used instead of directly slicing `Parser::command_buf` so that
/// ranges can be safely saved while the buffer is still being grown. After the
/// full command has been read (except for a data stream, which is read
/// separately), `Parser::command_buf` will not change until the next call to
/// `Parser::next`, and slices can be made and returned to the caller.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) struct Span {
    pub(super) start: usize,
    pub(super) end: usize,
}

#[inline(always)]
pub(super) fn slice<'a, T, S>(command: T, bytes: &'a S) -> T::Output
where
    T: MapBytes<Span, &'a [u8]>,
    S: Sliceable<'a> + ?Sized,
{
    command.map_bytes(&mut |field| field.slice(bytes))
}

impl<'a> Sliceable<'a> for [u8] {
    #[inline(always)]
    fn as_slice(&'a self) -> &'a [u8] {
        self
    }
}

impl<'a> Sliceable<'a> for Vec<u8> {
    #[inline(always)]
    fn as_slice(&'a self) -> &'a [u8] {
        self
    }
}

impl Span {
    #[cfg(debug_assertions)]
    #[inline(always)]
    pub(super) fn slice<'a, S: Sliceable<'a> + ?Sized>(&self, bytes: &'a S) -> &'a [u8] {
        &bytes.as_slice()[Range::from(*self)]
    }

    #[cfg(not(debug_assertions))]
    #[inline(always)]
    pub(super) fn slice<'a, S: Sliceable<'a> + ?Sized>(&self, bytes: &'a S) -> &'a [u8] {
        // SAFETY: It is up to the caller to ensure that spans are in bounds.
        //
        // Most spans are for `Parser::command_buf`. Since its length
        // monotonically increases during a call to `Parser::next`, as long as a
        // span is used in the same call, only its construction is relevant.
        // Spans used for other buffers, such as when reading data, have
        // different considerations. Since spans do not leak into the public
        // API, the surface area is manageable.
        unsafe { bytes.as_slice().get_unchecked(Range::from(*self)) }
    }

    #[inline(always)]
    pub(super) fn is_empty(&self) -> bool {
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
