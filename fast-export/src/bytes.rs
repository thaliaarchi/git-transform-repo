use std::borrow::Cow;

use bstr::{BStr, BString};

pub trait FromBytes<'a> {
    fn from_bytes(bytes: &'a [u8]) -> Self;
}

impl<'a> FromBytes<'a> for &'a [u8] {
    #[inline(always)]
    fn from_bytes(bytes: &'a [u8]) -> Self {
        bytes
    }
}

impl FromBytes<'_> for Vec<u8> {
    #[inline]
    fn from_bytes(bytes: &[u8]) -> Self {
        bytes.to_vec()
    }
}

impl<'a> FromBytes<'a> for Cow<'a, [u8]> {
    #[inline(always)]
    fn from_bytes(bytes: &'a [u8]) -> Self {
        Cow::Borrowed(bytes)
    }
}

impl<'a> FromBytes<'a> for &'a BStr {
    #[inline(always)]
    fn from_bytes(bytes: &'a [u8]) -> Self {
        BStr::new(bytes)
    }
}

impl FromBytes<'_> for BString {
    #[inline]
    fn from_bytes(bytes: &[u8]) -> Self {
        BString::new(bytes.to_vec())
    }
}

impl<'a> FromBytes<'a> for Cow<'a, BStr> {
    #[inline(always)]
    fn from_bytes(bytes: &'a [u8]) -> Self {
        Cow::Borrowed(BStr::new(bytes))
    }
}
