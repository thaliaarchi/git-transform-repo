use memchr::memchr2;
use thiserror::Error;

use crate::parse::BufPool;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Hash)]
pub enum ParseStringError {
    #[error("string not terminated")]
    Unterminated,
    #[error("invalid escape sequence")]
    InvalidEscape,
    #[error("invalid digit in octal escape sequence")]
    InvalidOctalDigit,
    #[error("octal escape sequence overflows")]
    OctalOverflow,
}

impl BufPool {
    /// Unquotes a C-style string literal. Returns the string with escape
    /// sequences translated and the remainder of the input.
    ///
    // Corresponds to `git.git/quote.c:unquote_c_style`.
    pub fn unquote_c_style_string<'a>(
        &'a self,
        s: &'a [u8],
    ) -> Result<(&'a [u8], &'a [u8]), ParseStringError> {
        type Error = ParseStringError;
        assert!(s[0] == b'"', "not a string");
        let mut i = 1;
        let mut j = memchr2(b'"', b'\\', &s[i..]).ok_or(Error::Unterminated)?;
        if s[j] == b'"' {
            // Avoid allocating when we have no escapes.
            return Ok((&s[i..j], &s[j + 1..]));
        }
        let buf = self.new_aux_buffer();
        loop {
            buf.extend_from_slice(&s[i..j]);
            match s[j] {
                b'"' => {
                    return Ok((buf, &s[j + 1..]));
                }
                b'\\' => {
                    j += 1;
                    if j >= s.len() {
                        return Err(Error::Unterminated);
                    }
                    let ch = match s[j] {
                        b'a' => 0x07,
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'v' => 0x0b,

                        ch @ (b'\\' | b'"') => ch,

                        o1 @ (b'0' | b'1' | b'2' | b'3') => {
                            j += 2;
                            if j >= s.len() {
                                return Err(Error::Unterminated);
                            }
                            let o2 = s[j - 1];
                            let o3 = s[j];
                            if (b'0'..=b'7').contains(&o2) && (b'0'..=b'7').contains(&o3) {
                                (o1 - b'0') << 6 | (o2 - b'0') << 3 | (o3 - b'0')
                            } else {
                                return Err(Error::InvalidOctalDigit);
                            }
                        }
                        b'4' | b'5' | b'6' | b'7' => return Err(Error::OctalOverflow),

                        _ => return Err(Error::InvalidEscape),
                    };
                    buf.push(ch);
                }
                _ => unreachable!(),
            }
            i = j + 1;
            j = memchr2(b'"', b'\\', &s[i..]).ok_or(Error::Unterminated)?;
        }
    }
}
