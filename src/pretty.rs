use std::io::{self, Write};

use crate::{
    BString, Blob, Commented, Comments, CountedData, Data, DelimitedData, Mark, OriginalOid,
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

impl Pretty for BString {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(self.as_bytes())
    }
}

impl Pretty for Comments {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        for line in self.text().split_inclusive(|&b| b == b'\n') {
            w.write_all(b"#")?;
            w.write_all(line)?;
        }
        if self.text().last().is_some_and(|&b| b != b'\n') {
            w.write_all(b"\n")?;
        }
        Ok(())
    }
}

impl<T: Pretty> Pretty for Commented<T> {
    fn pretty<W: Write>(&self, w: &mut W) -> io::Result<()> {
        self.comments.pretty(w)?;
        self.value.pretty(w)
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
        //     pretty(DelimitedData::new("Hello, world!\n", BString::new(b"").unwrap()).unwrap()),
        //     b"data <<\nHello, world!\n\n\n",
        // );
        assert_eq!(
            pretty(DelimitedData::new("Hello, world!\n", BString::new(b"EOF").unwrap()).unwrap()),
            b"data <<EOF\nHello, world!\nEOF\n\n",
        );
    }

    #[test]
    fn comments() {
        assert_eq!(pretty(Comments::new(b"")), b"");
        assert_eq!(pretty(Comments::new(b"\n")), b"#\n");
        assert_eq!(pretty(Comments::new(b"\n\n")), b"#\n#\n");
        assert_eq!(pretty(Comments::new(b"a\nb")), b"#a\n#b\n");
        assert_eq!(pretty(Comments::new(b"a\nb\n")), b"#a\n#b\n");
    }
}
