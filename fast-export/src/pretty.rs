use std::io::{self, Write};

use crate::ast::{Blob, Data, FileSize, Mark, OptionGit, OptionOther, OriginalOid, UnitFactor};

pub trait Pretty {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()>;
}

impl Pretty for Blob<'_> {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"blob\n")?;
        self.mark.pretty(w)?;
        self.original_oid.pretty(w)?;
        self.data.pretty(w)
    }
}

impl Pretty for OptionGit<'_> {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        // Positive sign and leading zeros are not preserved from the source.
        w.write_all(b"option git ")?;
        match *self {
            OptionGit::MaxPackSize(n) => {
                w.write_all(b"--max-pack-size=")?;
                n.pretty(w)?;
                w.write_all(b"\n")
            }
            OptionGit::BigFileThreshold(n) => {
                w.write_all(b"--big-file-threshold=")?;
                n.pretty(w)?;
                w.write_all(b"\n")
            }
            OptionGit::Depth(n) => write!(w, "--depth={n}\n"),
            OptionGit::ActiveBranches(n) => write!(w, "--active-branches={n}\n"),
            OptionGit::ExportPackEdges(file) => {
                write!(w, "--export-pack-edges=")?;
                w.write_all(file)?;
                w.write_all(b"\n")
            }
            OptionGit::Quiet => w.write_all(b"--quiet\n"),
            OptionGit::Stats => w.write_all(b"--stats\n"),
            OptionGit::AllowUnsafeFeatures => w.write_all(b"--allow-unsafe-features\n"),
        }
    }
}

impl Pretty for OptionOther<'_> {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"option ")?;
        w.write_all(self.option)?;
        w.write_all(b"\n")
    }
}

impl Pretty for Mark {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        write!(w, "mark :{}\n", self.mark)
    }
}

impl Pretty for OriginalOid<'_> {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"original-oid ")?;
        w.write_all(self.oid)?;
        w.write_all(b"\n")
    }
}

impl Pretty for Data<'_> {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        if let Some(delim) = self.delim {
            // Dump it in the delimited style only if it would parse correctly
            // with the data.
            if self.validate_delim().is_ok() {
                w.write_all(b"data <<")?;
                w.write_all(delim)?;
                w.write_all(b"\n")?;
                w.write_all(self.data)?;
                w.write_all(delim)?;
                w.write_all(b"\n\n")?; // Second LF is optional
                return Ok(());
            }
        }
        write!(w, "data {}\n", self.data.len())?;
        w.write_all(self.data)?;
        w.write_all(b"\n") // Optional LF
    }
}

impl Pretty for FileSize {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        // Case is not preserved from the source.
        write!(w, "{}", self.value)?;
        match self.unit {
            UnitFactor::B => Ok(()),
            UnitFactor::K => w.write_all(b"k"),
            UnitFactor::M => w.write_all(b"m"),
            UnitFactor::G => w.write_all(b"g"),
        }
    }
}

impl<T: Pretty> Pretty for Option<T> {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        if let Some(value) = self {
            value.pretty(w)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pretty<T: Pretty>(value: T) -> Vec<u8> {
        let mut buf = Vec::new();
        value.pretty(&mut buf).unwrap();
        buf
    }

    #[test]
    fn data() {
        assert_eq!(
            pretty(Data {
                data: b"Hello, world!",
                delim: None,
            }),
            b"data 13\nHello, world!\n",
        );
        assert_eq!(
            pretty(Data {
                data: b"Hello, world!\n",
                delim: Some(b"EOF"),
            }),
            b"data <<EOF\nHello, world!\nEOF\n\n",
        );
    }

    #[test]
    fn data_invalid_delim() {
        assert_eq!(
            pretty(Data {
                data: b"Hello,\nEOF\nworld!\n", // Contains delim
                delim: Some(b"EOF"),
            }),
            b"data 18\nHello,\nEOF\nworld!\n\n",
        );
        assert_eq!(
            pretty(Data {
                data: b"Hello, world!", // No final LF
                delim: Some(b"EOF"),
            }),
            b"data 13\nHello, world!\n",
        );
        assert_eq!(
            pretty(Data {
                data: b"Hello,\0world!\n", // Contains NUL
                delim: Some(b"EOF"),
            }),
            b"data 14\nHello,\0world!\n\n",
        );
        assert_eq!(
            pretty(Data {
                data: b"Hello, world!\n",
                delim: Some(b""), // Empty delim
            }),
            b"data 14\nHello, world!\n\n",
        );
    }

    #[test]
    fn option_git() {}

    #[test]
    fn option_other() {
        assert_eq!(
            pretty(OptionOther {
                option: b"vcs some config",
            }),
            b"option vcs some config\n",
        );
    }
}
