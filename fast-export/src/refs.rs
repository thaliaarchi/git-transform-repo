//! Validation for Git refnames.
//!
//! Up to date with Git as of [8f7582d995](https://git.kernel.org/pub/scm/git/git.git/commit/?id=8f7582d995682f785e80e344197cc715e6bc7d8e)
//! (The eighteenth batch, 2024-04-12).

use std::fmt::{self, Debug, Formatter};

use bstr::ByteSlice;
use enumflags2::{bitflags, BitFlag, BitFlags};
use thiserror::Error;

/// A Git reference name.
#[repr(transparent)]
#[derive(PartialEq, Eq)]
pub struct Refname {
    refname: [u8],
}

/// Flags for checking refnames.
#[bitflags]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefnameFlag {
    /// One-level refnames are accepted.
    ///
    // Corresponds to `git.git/refs.h:REFNAME_ALLOW_ONELEVEL`.
    AllowOneLevel,
    /// Interpret the refname as a pattern, which can contain a single asterisk
    /// `*`.
    ///
    // Corresponds to `git.git/refs.h:REFNAME_REFSPEC_PATTERN`.
    RefspecPattern,
}

/// A violation of how references are named in Git. See the documentation of
/// [`git check-ref-format`](https://git-scm.com/docs/git-check-ref-format) for
/// more information.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum RefnameError {
    #[error("refname is empty")]
    Empty,
    #[error("refname starts with slash `/`")]
    StartsWithSlash,
    #[error("refname ends with slash `/`")]
    EndsWithSlash,
    #[error("refname contains consecutive slashes `//`")]
    SlashSlash,
    #[error("refname has only one level (must contain slash `/`)")]
    OnlyOneLevel,

    #[error("refname component is dot `.`")]
    ComponentIsDot,
    #[error("refname component starts with dot `.`")]
    ComponentStartsWithDot,
    #[error("refname ends with dot `.`")]
    EndsWithDot,
    #[error("refname contains consecutive dots `..`")]
    DotDot,

    #[error("refname contains asterisk `*`")]
    Asterisk,
    #[error("refname pattern contains multiple asterisks `*`")]
    MultipleAsterisks,

    #[error("refname contains ASCII control character")]
    ControlChar,
    #[error("refname contains space ` `")]
    Space,
    #[error("refname contains colon `:`")]
    Colon,
    #[error("refname contains question mark `?`")]
    Question,
    #[error("refname contains open bracket `[`")]
    OpenBracket,
    #[error("refname contains backslash `\\`")]
    Backslash,
    #[error("refname contains caret `^`")]
    Caret,
    #[error("refname contains tilde `~`")]
    Tilde,

    #[error("refname is the single character `@`")]
    IsAt,
    #[error("refname contains the sequence `@{{`")]
    AtBrace,

    #[error("refname component ends with the sequence `.lock`")]
    ComponentEndsWithDotLock,
}

impl Refname {
    /// Create a new `Refname` and check that it has a valid format.
    pub fn new<B: AsRef<[u8]> + ?Sized>(
        refname: &B,
        flags: BitFlags<RefnameFlag>,
    ) -> Result<&Self, RefnameError> {
        let refname = refname.as_ref();
        Refname::check_format(refname, flags)?;
        Ok(Refname::new_(refname))
    }

    /// Create a new `Refname` without checking that it has a valid format.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the refname has a valid format.
    pub unsafe fn new_unchecked<B: AsRef<[u8]> + ?Sized>(refname: &B) -> &Self {
        debug_assert!(Refname::check_format(refname.as_ref(), RefnameFlag::all()).is_ok());
        Refname::new_(refname.as_ref())
    }

    #[inline(always)]
    fn new_(refname: &[u8]) -> &Self {
        // SAFETY: Refname is repr(transparent).
        unsafe { &*(refname as *const [u8] as *const Refname) }
    }

    /// Check that the refname has a valid format according to the rules of
    /// [`git check-ref-format`](https://git-scm.com/docs/git-check-ref-format).
    /// If [`RefnameFlag::AllowOneLevel`] is set in `flags`, then accept
    /// one-level reference names. If [`RefnameFlag::RefspecPattern`] is set in
    /// `flags`, then allow a single `*` wildcard character in the refspec.
    ///
    // Corresponds to `git.git/refs.c:check_refname_format`.
    pub fn check_format(refname: &[u8], flags: BitFlags<RefnameFlag>) -> Result<(), RefnameError> {
        let mut component_count = 0;
        let mut allow_asterisk = flags.contains(RefnameFlag::RefspecPattern);
        if refname == b"@" {
            return Err(RefnameError::IsAt);
        }

        let mut rest = refname;
        loop {
            let component_len = check_refname_component(rest, flags, &mut allow_asterisk)?;
            if component_len == 0 {
                if refname.is_empty() {
                    return Err(RefnameError::Empty);
                } else if component_count == 0 {
                    return Err(RefnameError::StartsWithSlash);
                } else if rest.is_empty() {
                    return Err(RefnameError::EndsWithSlash);
                } else {
                    return Err(RefnameError::SlashSlash);
                }
            }
            component_count += 1;
            if component_len == rest.len() {
                break;
            }
            rest = &rest[component_len + 1..];
        }

        if refname.ends_with(b".") {
            return Err(RefnameError::EndsWithDot);
        }
        if !flags.contains(RefnameFlag::AllowOneLevel) && component_count < 2 {
            return Err(RefnameError::OnlyOneLevel);
        }
        Ok(())
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8] {
        &self.refname
    }
}

/// Try to read one path component from the front of `refname`. Return the
/// length of the component, or an error if the component is not legal.
///
// Corresponds to `git.git/refs.c:check_refname_component`.
#[inline]
fn check_refname_component(
    refname: &[u8],
    flags: BitFlags<RefnameFlag>,
    allow_asterisk: &mut bool,
) -> Result<usize, RefnameError> {
    let mut component_len = refname.len();
    let mut last = 0;
    for (i, &ch) in refname.iter().enumerate() {
        match ch {
            // Forbidden characters.
            b'\0'..=b'\x1f' | b'\x7f' => return Err(RefnameError::ControlChar),
            b' ' => return Err(RefnameError::Space),
            b':' => return Err(RefnameError::Colon),
            b'?' => return Err(RefnameError::Question),
            b'[' => return Err(RefnameError::OpenBracket),
            b'\\' => return Err(RefnameError::Backslash),
            b'^' => return Err(RefnameError::Caret),
            b'~' => return Err(RefnameError::Tilde),

            b'*' => {
                // Only accept a single asterisk if it is a refspec pattern,
                // and none otherwise.
                if *allow_asterisk {
                    *allow_asterisk = false;
                } else if flags.contains(RefnameFlag::RefspecPattern) {
                    return Err(RefnameError::MultipleAsterisks);
                } else {
                    return Err(RefnameError::Asterisk);
                }
            }

            // Forbidden sequences: `..` and `@{`.
            b'.' if last == b'.' => return Err(RefnameError::DotDot),
            b'{' if last == b'@' => return Err(RefnameError::AtBrace),

            // End of the component.
            b'/' => {
                component_len = i;
                break;
            }

            // Valid characters.
            _ => {}
        }
        last = ch;
    }

    if component_len != 0 {
        let component = &refname[..component_len];
        if component[0] == b'.' {
            if component_len == 1 {
                return Err(RefnameError::ComponentIsDot);
            } else {
                return Err(RefnameError::ComponentStartsWithDot);
            }
        }
        if component.ends_with(b".lock") {
            return Err(RefnameError::ComponentEndsWithDotLock);
        }
    }
    // Handle empty component errors with more context in caller.
    Ok(component_len)
}

impl Debug for Refname {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // TODO: Match how Git escapes bytes.
        f.debug_tuple("Refname")
            .field(&self.refname.as_bstr())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use enumflags2::BitFlag;

    use super::*;
    use RefnameFlag::*;

    macro_rules! flags(
        () => { RefnameFlag::empty() };
        ($flags:expr) => { $flags.into() };
    );
    macro_rules! valid_ref(($refname:literal $(, $flags:expr)?) => {{
        let flags = flags!($($flags)?);
        assert_eq!(
            Refname::check_format($refname, flags),
            Ok(()),
            "{:?}, {flags}", $refname.as_bstr(),
        );
    }});
    macro_rules! invalid_ref(($refname:literal $(, $flags:expr)? => $err:ident) => {{
        let flags = flags!($($flags)?);
        assert_eq!(
            Refname::check_format($refname, flags),
            Err(RefnameError::$err),
            "{:?}, {flags}", $refname.as_bstr(),
        );
    }});

    // # Differences from Git
    //
    // Tests with `--normalize` or `--branch` are excluded and the `!MINGW`
    // prerequisite, which appears on refnames starting with `/`, is omitted.
    //
    // Corresponds to `git.git/t/t1402-check-ref-format.sh`.
    #[test]
    fn t1402_check_ref_format() {
        invalid_ref!(b"" => Empty);
        invalid_ref!(b"/" => StartsWithSlash);
        invalid_ref!(b"/", AllowOneLevel => StartsWithSlash);
        valid_ref!(b"foo/bar/baz");
        invalid_ref!(b"refs///heads/foo" => SlashSlash);
        invalid_ref!(b"heads/foo/" => EndsWithSlash);
        invalid_ref!(b"/heads/foo" => StartsWithSlash);
        invalid_ref!(b"///heads/foo" => StartsWithSlash);
        invalid_ref!(b"./foo" => ComponentIsDot);
        invalid_ref!(b"./foo/bar" => ComponentIsDot);
        invalid_ref!(b"foo/./bar" => ComponentIsDot);
        invalid_ref!(b"foo/bar/." => ComponentIsDot);
        invalid_ref!(b".refs/foo" => ComponentStartsWithDot);
        invalid_ref!(b"refs/heads/foo." => EndsWithDot);
        invalid_ref!(b"heads/foo..bar" => DotDot);
        invalid_ref!(b"heads/foo?bar" => Question);
        valid_ref!(b"foo./bar");
        invalid_ref!(b"heads/foo.lock" => ComponentEndsWithDotLock);
        invalid_ref!(b"heads///foo.lock" => SlashSlash);
        invalid_ref!(b"foo.lock/bar" => ComponentEndsWithDotLock);
        invalid_ref!(b"foo.lock///bar" => ComponentEndsWithDotLock);
        valid_ref!(b"heads/foo@bar");
        invalid_ref!(b"heads/v@{ation" => AtBrace);
        invalid_ref!(b"heads/foo\\bar" => Backslash);
        invalid_ref!(b"heads/foo\t" => ControlChar);
        invalid_ref!(b"heads/foo\x7f" => ControlChar);
        valid_ref!(b"heads/fu\xc3\x9f");
        valid_ref!(b"heads/*foo/bar", RefspecPattern);
        valid_ref!(b"heads/foo*/bar", RefspecPattern);
        valid_ref!(b"heads/f*o/bar", RefspecPattern);
        invalid_ref!(b"heads/f*o*/bar", RefspecPattern => MultipleAsterisks);
        invalid_ref!(b"heads/foo*/bar*", RefspecPattern => MultipleAsterisks);

        invalid_ref!(b"foo" => OnlyOneLevel);
        valid_ref!(b"foo", AllowOneLevel);
        invalid_ref!(b"foo", RefspecPattern => OnlyOneLevel);
        valid_ref!(b"foo", RefspecPattern | AllowOneLevel);

        valid_ref!(b"foo/bar");
        valid_ref!(b"foo/bar", AllowOneLevel);
        valid_ref!(b"foo/bar", RefspecPattern);
        valid_ref!(b"foo/bar", RefspecPattern | AllowOneLevel);

        invalid_ref!(b"foo/*" => Asterisk);
        invalid_ref!(b"foo/*", AllowOneLevel => Asterisk);
        valid_ref!(b"foo/*", RefspecPattern);
        valid_ref!(b"foo/*", RefspecPattern | AllowOneLevel);

        invalid_ref!(b"*/foo" => Asterisk);
        invalid_ref!(b"*/foo", AllowOneLevel => Asterisk);
        valid_ref!(b"*/foo", RefspecPattern);
        valid_ref!(b"*/foo", RefspecPattern | AllowOneLevel);

        invalid_ref!(b"foo/*/bar" => Asterisk);
        invalid_ref!(b"foo/*/bar", AllowOneLevel => Asterisk);
        valid_ref!(b"foo/*/bar", RefspecPattern);
        valid_ref!(b"foo/*/bar", RefspecPattern | AllowOneLevel);

        invalid_ref!(b"*" => Asterisk);
        invalid_ref!(b"*", AllowOneLevel => Asterisk);
        invalid_ref!(b"*", RefspecPattern => OnlyOneLevel);
        valid_ref!(b"*", RefspecPattern | AllowOneLevel);

        invalid_ref!(b"foo/*/*", RefspecPattern => MultipleAsterisks);
        invalid_ref!(b"foo/*/*", RefspecPattern | AllowOneLevel => MultipleAsterisks);

        invalid_ref!(b"*/foo/*", RefspecPattern => MultipleAsterisks);
        invalid_ref!(b"*/foo/*", RefspecPattern | AllowOneLevel => MultipleAsterisks);

        invalid_ref!(b"*/*/foo", RefspecPattern => MultipleAsterisks);
        invalid_ref!(b"*/*/foo", RefspecPattern | AllowOneLevel => MultipleAsterisks);

        invalid_ref!(b"/foo" => StartsWithSlash);
        invalid_ref!(b"/foo", AllowOneLevel => StartsWithSlash);
        invalid_ref!(b"/foo", RefspecPattern => StartsWithSlash);
        invalid_ref!(b"/foo", RefspecPattern | AllowOneLevel => StartsWithSlash);
    }

    /// Cases not covered by t1402.
    #[test]
    fn additional_cases() {
        invalid_ref!(b"foo bar" => Space);
        invalid_ref!(b"foo:bar" => Colon);
        invalid_ref!(b"foo[bar" => OpenBracket);
        invalid_ref!(b"foo^bar" => Caret);
        invalid_ref!(b"foo~bar" => Tilde);
        invalid_ref!(b"@" => IsAt);
    }
}
