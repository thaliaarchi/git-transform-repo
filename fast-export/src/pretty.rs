use std::io::{self, Write};

use crate::{
    Blob, CountedData, Data, DelimitedData, FileSize, InlineString, Mark, OptionGit, OptionOther,
    OriginalOid, UnitFactor,
};

pub trait Pretty {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()>;
}

impl Pretty for Blob {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"blob\n")?;
        self.mark.pretty(w)?;
        self.original_oid.pretty(w)?;
        self.data.pretty(w)
    }
}

impl Pretty for OptionGit {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        // Positive sign and leading zeros are not preserved from the source.
        w.write_all(b"option git ")?;
        match self {
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
                file.pretty(w)?;
                w.write_all(b"\n")
            }
            OptionGit::Quiet => w.write_all(b"--quiet\n"),
            OptionGit::Stats => w.write_all(b"--stats\n"),
            OptionGit::AllowUnsafeFeatures => w.write_all(b"--allow-unsafe-features\n"),
        }
    }
}

impl Pretty for OptionOther {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"option ")?;
        self.0.pretty(w)
    }
}

impl Pretty for Mark {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        write!(w, "mark :{}\n", self.mark)
    }
}

impl Pretty for OriginalOid {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"original-oid ")?;
        w.write_all(self.oid.as_bytes())?;
        w.write_all(b"\n")
    }
}

impl Pretty for Data {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        match self {
            Data::Counted(data) => data.pretty(w),
            Data::Delimited(data) => data.pretty(w),
        }
    }
}

impl Pretty for CountedData {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        write!(w, "data {}\n", self.data.len())?;
        w.write_all(&self.data)?;
        if self.optional_lf {
            w.write_all(b"\n")?;
        }
        Ok(())
    }
}

impl Pretty for DelimitedData {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"data <<")?;
        w.write_all(self.delim())?;
        w.write_all(b"\n")?;
        w.write_all(self.data())?;
        w.write_all(self.delim())?;
        w.write_all(b"\n")?;
        if self.optional_lf {
            w.write_all(b"\n")?;
        }
        Ok(())
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

impl Pretty for InlineString {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(self.as_bytes())
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
    fn counted_data() {
        assert_eq!(
            pretty(CountedData::new(b"Hello, world!")),
            b"data 13\nHello, world!\n",
        );
    }

    #[test]
    fn delimited_data() {
        // Empty delimiter is allowed by git fast-import:
        // assert_eq!(
        //     pretty(DelimitedData::new("Hello, world!\n", InlineString::new(b"").unwrap()).unwrap()),
        //     b"data <<\nHello, world!\n\n\n",
        // );
        assert_eq!(
            pretty(
                DelimitedData::new("Hello, world!\n", InlineString::new(b"EOF").unwrap()).unwrap(),
            ),
            b"data <<EOF\nHello, world!\nEOF\n\n",
        );
    }

    #[test]
    fn option_git() {}

    #[test]
    fn option_other() {
        assert_eq!(
            pretty(OptionOther(InlineString::new(b"vcs some config").unwrap())),
            b"option vcs some config",
        );
    }
}
