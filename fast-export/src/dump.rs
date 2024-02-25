use std::io::{self, Write};

use crate::command::{DataBuf, FileSize, Mark, OptionGit, OptionOther, OriginalOid, UnitFactor};

pub trait Dump {
    fn dump<W: Write>(&self, w: &mut W) -> io::Result<()>;
}

impl<B: AsRef<[u8]>> Dump for OptionGit<B> {
    fn dump<W: Write>(&self, w: &mut W) -> io::Result<()> {
        // Positive sign and leading zeros are not preserved from the source.
        w.write_all(b"option git ")?;
        match self {
            OptionGit::MaxPackSize(n) => {
                w.write_all(b"--max-pack-size=")?;
                n.dump(w)?;
                w.write_all(b"\n")
            }
            OptionGit::BigFileThreshold(n) => {
                w.write_all(b"--big-file-threshold=")?;
                n.dump(w)?;
                w.write_all(b"\n")
            }
            OptionGit::Depth(n) => write!(w, "--depth={n}\n"),
            OptionGit::ActiveBranches(n) => write!(w, "--active-branches={n}\n"),
            OptionGit::ExportPackEdges(file) => {
                write!(w, "--export-pack-edges=")?;
                w.write_all(file.as_ref())?;
                w.write_all(b"\n")
            }
            OptionGit::Quiet => w.write_all(b"--quiet\n"),
            OptionGit::Stats => w.write_all(b"--stats\n"),
            OptionGit::AllowUnsafeFeatures => w.write_all(b"--allow-unsafe-features\n"),
        }
    }
}

impl<B: AsRef<[u8]>> Dump for OptionOther<B> {
    fn dump<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"option ")?;
        w.write_all(self.option.as_ref())?;
        w.write_all(b"\n")
    }
}

impl Dump for Mark {
    fn dump<W: Write>(&self, w: &mut W) -> io::Result<()> {
        write!(w, "mark :{}\n", self.mark)
    }
}

impl<B: AsRef<[u8]>> Dump for OriginalOid<B> {
    fn dump<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"original-oid ")?;
        w.write_all(self.oid.as_ref())?;
        w.write_all(b"\n")
    }
}

impl Dump for DataBuf {
    fn dump<W: Write>(&self, w: &mut W) -> io::Result<()> {
        if let Some(delim) = &self.delim {
            // Dump it in the delimited style only if it would parse correctly
            // with the data.
            if self.validate_delim().is_ok() {
                w.write_all(b"data <<")?;
                w.write_all(delim)?;
                w.write_all(b"\n")?;
                w.write_all(&self.data)?;
                w.write_all(delim)?;
                w.write_all(b"\n\n")?; // Second LF is optional
                return Ok(());
            }
        }
        write!(w, "data {}\n", self.data.len())?;
        w.write_all(&self.data)?;
        w.write_all(b"\n") // Optional LF
    }
}

impl Dump for FileSize {
    fn dump<W: Write>(&self, w: &mut W) -> io::Result<()> {
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

impl<T: Dump> Dump for Option<T> {
    fn dump<W: Write>(&self, w: &mut W) -> io::Result<()> {
        if let Some(value) = self {
            value.dump(w)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump<T: Dump>(value: T) -> Vec<u8> {
        let mut buf = Vec::new();
        value.dump(&mut buf).unwrap();
        buf
    }

    #[test]
    fn data() {
        assert_eq!(
            dump(DataBuf {
                data: b"Hello, world!".to_vec(),
                delim: None,
            }),
            b"data 13\nHello, world!\n",
        );
        assert_eq!(
            dump(DataBuf {
                data: b"Hello, world!\n".to_vec(),
                delim: Some(b"EOF".to_vec()),
            }),
            b"data <<EOF\nHello, world!\nEOF\n\n",
        );
    }

    #[test]
    fn data_invalid_delim() {
        assert_eq!(
            dump(DataBuf {
                data: b"Hello,\nEOF\nworld!\n".to_vec(), // Contains delim
                delim: Some(b"EOF".to_vec()),
            }),
            b"data 18\nHello,\nEOF\nworld!\n\n",
        );
        assert_eq!(
            dump(DataBuf {
                data: b"Hello, world!".to_vec(), // No final LF
                delim: Some(b"EOF".to_vec()),
            }),
            b"data 13\nHello, world!\n",
        );
        assert_eq!(
            dump(DataBuf {
                data: b"Hello, world!\n".to_vec(),
                delim: Some(b"E\0F".to_vec()), // Contains NUL
            }),
            b"data 14\nHello, world!\n\n",
        );
        assert_eq!(
            dump(DataBuf {
                data: b"Hello, world!\n".to_vec(),
                delim: Some(b"".to_vec()), // Empty delim
            }),
            b"data 14\nHello, world!\n\n",
        );
    }

    #[test]
    fn option_git() {}

    #[test]
    fn option_other() {
        assert_eq!(
            dump(OptionOther {
                option: b"vcs some config",
            }),
            b"option vcs some config\n",
        );
    }
}
