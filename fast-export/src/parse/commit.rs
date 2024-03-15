use std::io::BufRead;

use memchr::memchr;

use crate::{
    command::{Blobish, CatBlob, Treeish},
    parse::{parse_ls, BufInput, DirectiveParser, PResult, ParseError},
};

pub struct ChangeIter<'a, R> {
    /// A borrow of `Parser::input`.
    input: &'a BufInput<R>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change<B> {
    FileModify,
    FileDelete { path: B },
    FileRename { source: B, dest: B },
    FileCopy { source: B, dest: B },
    FileDeleteAll,
    NoteModify,
    Ls(CommitLs<B>),
    CatBlob(CatBlob<B>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitLs<B> {
    pub root: Option<Treeish<B>>,
    pub path: B,
}

impl<'a, R: BufRead> ChangeIter<'a, R> {
    pub fn next(&'a self) -> PResult<Option<Change<&'a [u8]>>> {
        let Some(line) = self.input.next_directive()? else {
            return Ok(None);
        };

        let change = if let Some(args) = line.strip_prefix(b"M ") {
            self.parse_file_modify(args)
        } else if let Some(path) = line.strip_prefix(b"D ") {
            self.parse_file_delete(path)
        } else if let Some(paths) = line.strip_prefix(b"R ") {
            self.parse_file_rename(paths)
        } else if let Some(paths) = line.strip_prefix(b"C ") {
            self.parse_file_copy(paths)
        } else if line == b"deleteall" {
            self.parse_file_delete_all()
        } else if let Some(args) = line.strip_prefix(b"N ") {
            self.parse_note_modify(args)
        } else if let Some(args) = line.strip_prefix(b"ls ") {
            self.parse_ls(args)
        } else if let Some(data_ref) = line.strip_prefix(b"cat-blob ") {
            self.parse_cat_blob(data_ref)
        } else {
            self.input.unread_directive();
            return Ok(None);
        };
        change.map(Some)
    }

    // Corresponds to `file_change_m` in fast-import.c.
    fn parse_file_modify(&'a self, _args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        todo!()
    }

    // Corresponds to `file_change_d` in fast-import.c.
    fn parse_file_delete(&'a self, path: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let path = self
            .unquote_eol(path)
            .ok_or(ParseError::JunkAfterFileDeletePath)?;
        Ok(Change::FileDelete { path })
    }

    // Corresponds to `file_change_cr(s, b, 1)` in fast-import.c.
    fn parse_file_rename(&'a self, paths: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let (source, dest) = self.parse_file_rename_copy(paths)?;
        Ok(Change::FileRename { source, dest })
    }

    // Corresponds to `file_change_cr(s, b, 0)` in fast-import.c.
    fn parse_file_copy(&'a self, paths: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let (source, dest) = self.parse_file_rename_copy(paths)?;
        Ok(Change::FileCopy { source, dest })
    }

    // Corresponds to `file_change_cr` in fast-import.c.
    fn parse_file_rename_copy(&'a self, paths: &'a [u8]) -> PResult<(&'a [u8], &'a [u8])> {
        let (source, dest) = self
            .unquote_space(paths)
            .ok_or(ParseError::MissingSpaceAfterSource)?;
        if dest.is_empty() {
            return Err(ParseError::MissingDest.into());
        }
        let dest = self.unquote_eol(dest).ok_or(ParseError::JunkAfterDest)?;
        Ok((source, dest))
    }

    // Corresponds to `file_change_deleteall` in fast-import.c.
    fn parse_file_delete_all(&'a self) -> PResult<Change<&'a [u8]>> {
        Ok(Change::FileDeleteAll)
    }

    // Corresponds to `note_change_n` in fast-import.c.
    fn parse_note_modify(&'a self, _args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        todo!()
    }

    // Corresponds to `parse_ls(p, b)` in fast-import.c.
    fn parse_ls(&'a self, args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let (root, path) = parse_ls(self, args, true)?;
        Ok(Change::Ls(CommitLs { root, path }))
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob(&'a self, data_ref: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let blob = Blobish::parse(data_ref)?;
        Ok(Change::CatBlob(CatBlob { blob }))
    }

    /// Returns `None` when the string is not followed by a space.
    fn unquote_space(&'a self, s: &'a [u8]) -> Option<(&'a [u8], &'a [u8])> {
        // BUG-COMPAT: fast-import only treats this path as a quoted string when
        // it parses successfully, in contrast to `ls`.
        if let Ok((unquoted, rest)) = self.unquote_c_style_string(s) {
            if !rest.starts_with(b" ") {
                return None;
            }
            Some((unquoted, &rest[1..]))
        } else {
            let Some(i) = memchr(b' ', s) else {
                return None;
            };
            let (s, rest) = s.split_at(i);
            Some((s, &rest[1..]))
        }
    }

    /// Returns `None` when the string is followed by junk.
    fn unquote_eol(&'a self, s: &'a [u8]) -> Option<&'a [u8]> {
        // BUG-COMPAT: fast-import only treats this path as a quoted string when
        // it parses successfully, in contrast to `ls`.
        if let Ok((unquoted, rest)) = self.unquote_c_style_string(s) {
            if !rest.is_empty() {
                return None;
            }
            Some(unquoted)
        } else {
            // BUG-COMPAT: Allows spaces when unquoted.
            Some(s)
        }
    }
}

impl<R: BufRead> DirectiveParser<R> for ChangeIter<'_, R> {
    #[inline(always)]
    fn input(&self) -> &BufInput<R> {
        &self.input
    }
}
