use std::io::BufRead;

use crate::{
    command::{Blobish, CatBlob, Treeish},
    parse::{parse_ls, BufInput, DirectiveParser, PResult},
};

pub struct ChangeIter<'a, R> {
    /// A borrow of `Parser::input`.
    input: &'a BufInput<R>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change<B> {
    FileModify,
    FileDelete,
    FileRename,
    FileCopy,
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
        } else if let Some(args) = line.strip_prefix(b"D ") {
            self.parse_file_delete(args)
        } else if let Some(args) = line.strip_prefix(b"R ") {
            self.parse_file_rename(args)
        } else if let Some(args) = line.strip_prefix(b"C ") {
            self.parse_file_copy(args)
        } else if line == b"deleteall" {
            self.parse_file_delete_all()
        } else if let Some(args) = line.strip_prefix(b"N ") {
            self.parse_note_modify(args)
        } else if let Some(args) = line.strip_prefix(b"ls ") {
            self.parse_ls(args)
        } else if let Some(data_ref) = line.strip_prefix(b"cat-blob ") {
            self.parse_cat_blob(data_ref)
        } else {
            // TODO: Unread
            return Ok(None);
        };
        change.map(Some)
    }

    // Corresponds to `file_change_m` in fast-import.c.
    fn parse_file_modify(&'a self, _args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        todo!()
    }

    // Corresponds to `file_change_d` in fast-import.c.
    fn parse_file_delete(&'a self, _args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        todo!()
    }

    // Corresponds to `file_change_cr(s, b, 1)` in fast-import.c.
    fn parse_file_rename(&'a self, _args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        todo!()
    }

    // Corresponds to `file_change_cr(s, b, 0)` in fast-import.c.
    fn parse_file_copy(&'a self, _args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        todo!()
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
}

impl<R: BufRead> DirectiveParser<R> for ChangeIter<'_, R> {
    #[inline(always)]
    fn input(&self) -> &BufInput<R> {
        &self.input
    }
}
