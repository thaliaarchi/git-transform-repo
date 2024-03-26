use std::{io::BufRead, str};

use memchr::memchr;

use crate::{
    command::{Blobish, CatBlob, Commitish, Mark, Mode, Treeish},
    parse::{parse_ls, BufInput, DirectiveParser, PResult, ParseError},
};

pub struct ChangeIter<'a, R> {
    /// A borrow of `Parser::input`.
    input: &'a BufInput<R>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change<B> {
    FileModify(FileModifyChange<B>),
    FileDelete(FileDeleteChange<B>),
    FileRename(FileRenameChange<B>),
    FileCopy(FileCopyChange<B>),
    FileDeleteAll,
    NoteModify(NoteModifyChange<B>),
    Ls(CommitLs<B>),
    CatBlob(CatBlob<B>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileModifyChange<B> {
    data_ref: DataRef<B>,
    mode: Mode,
    path: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDeleteChange<B> {
    path: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRenameChange<B> {
    source: B,
    dest: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCopyChange<B> {
    source: B,
    dest: B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteModifyChange<B> {
    data_ref: DataRef<B>,
    commit: Commitish<B>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataRef<B> {
    Mark(Mark),
    Oid(B),
    Inline,
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
    fn parse_file_modify(&'a self, args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let (mode, rest) = split_at_space(args).ok_or(ParseError::NoSpaceAfterMode)?;
        let mode = Mode::parse(mode)?;

        let (data_ref, path) = split_at_space(rest).ok_or(ParseError::NoSpaceAfterDataRef)?;
        let data_ref = DataRef::parse(data_ref)?;
        let path = self
            .unquote_eol(path)
            .ok_or(ParseError::JunkAfterFileModifyPath)?;

        // TODO: Emit `cat-blob` commands before the modify.
        // TODO: Parse data.

        Ok(Change::from(FileModifyChange {
            data_ref,
            mode,
            path,
        }))
    }

    // Corresponds to `file_change_d` in fast-import.c.
    fn parse_file_delete(&'a self, path: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let path = self
            .unquote_eol(path)
            .ok_or(ParseError::JunkAfterFileDeletePath)?;
        Ok(Change::from(FileDeleteChange { path }))
    }

    // Corresponds to `file_change_cr(s, b, 1)` in fast-import.c.
    fn parse_file_rename(&'a self, paths: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let (source, dest) = self.parse_file_rename_copy(paths)?;
        Ok(Change::from(FileRenameChange { source, dest }))
    }

    // Corresponds to `file_change_cr(s, b, 0)` in fast-import.c.
    fn parse_file_copy(&'a self, paths: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let (source, dest) = self.parse_file_rename_copy(paths)?;
        Ok(Change::from(FileCopyChange { source, dest }))
    }

    // Corresponds to `file_change_cr` in fast-import.c.
    fn parse_file_rename_copy(&'a self, paths: &'a [u8]) -> PResult<(&'a [u8], &'a [u8])> {
        let (source, dest) = self
            .unquote_space(paths)
            .ok_or(ParseError::NoSpaceAfterSource)?;
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
    fn parse_note_modify(&'a self, args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let (data_ref, commit) = split_at_space(args).ok_or(ParseError::NoSpaceAfterDataRef)?;
        let data_ref = DataRef::parse(data_ref)?;
        let commit = Commitish::parse(commit)?;

        // TODO: Parse data.

        Ok(Change::from(NoteModifyChange { data_ref, commit }))
    }

    // Corresponds to `parse_ls(p, b)` in fast-import.c.
    fn parse_ls(&'a self, args: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let (root, path) = parse_ls(self, args, true)?;
        Ok(Change::from(CommitLs { root, path }))
    }

    // Corresponds to `parse_cat_blob` in fast-import.c.
    fn parse_cat_blob(&'a self, data_ref: &'a [u8]) -> PResult<Change<&'a [u8]>> {
        let blob = Blobish::parse(data_ref)?;
        Ok(Change::from(CatBlob { blob }))
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
            split_at_space(s)
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

impl<'a> DataRef<&'a [u8]> {
    // Corresponds to parts of `file_change_m` and `note_change_n` in
    // fast-import.c.
    fn parse(data_ref: &'a [u8]) -> PResult<Self> {
        if data_ref == b"inline" {
            Ok(DataRef::Inline)
        } else if data_ref.starts_with(b":") {
            Ok(DataRef::Mark(Mark::parse(data_ref)?))
        } else {
            Ok(DataRef::Oid(data_ref))
        }
    }
}

impl Mode {
    /// Parse a file mode string. Allows only canonical modes, with the
    /// exception that files can be shortened to just their permission bits for
    /// brevity. Leading zeros are allowed. This logic is specific to
    /// fast-import.
    ///
    // Corresponds to part of `file_change_m` in fast-import.c.
    fn parse(mode: &[u8]) -> PResult<Self> {
        // SAFETY: `from_str_radix` operates on bytes and accepts only ASCII.
        let mode = u16::from_str_radix(unsafe { str::from_utf8_unchecked(mode) }, 8)
            .map_err(|_| ParseError::InvalidModeInt)?;
        match mode {
            0o100644 | 0o644 => Ok(Mode::File),
            0o100755 | 0o755 => Ok(Mode::Exe),
            0o120000 => Ok(Mode::SymLink),
            0o160000 => Ok(Mode::GitLink),
            0o040000 => Ok(Mode::Dir),
            _ => Err(ParseError::InvalidMode.into()),
        }
    }
}

fn split_at_space(b: &[u8]) -> Option<(&[u8], &[u8])> {
    memchr(b' ', b).map(|i| {
        let (b1, b2) = b.split_at(i);
        (b1, &b2[1..])
    })
}

impl<B> From<FileModifyChange<B>> for Change<B> {
    #[inline(always)]
    fn from(change: FileModifyChange<B>) -> Self {
        Change::FileModify(change)
    }
}

impl<B> From<FileDeleteChange<B>> for Change<B> {
    #[inline(always)]
    fn from(change: FileDeleteChange<B>) -> Self {
        Change::FileDelete(change)
    }
}

impl<B> From<FileRenameChange<B>> for Change<B> {
    #[inline(always)]
    fn from(change: FileRenameChange<B>) -> Self {
        Change::FileRename(change)
    }
}

impl<B> From<FileCopyChange<B>> for Change<B> {
    #[inline(always)]
    fn from(change: FileCopyChange<B>) -> Self {
        Change::FileCopy(change)
    }
}

impl<B> From<NoteModifyChange<B>> for Change<B> {
    #[inline(always)]
    fn from(change: NoteModifyChange<B>) -> Self {
        Change::NoteModify(change)
    }
}

impl<B> From<CommitLs<B>> for Change<B> {
    #[inline(always)]
    fn from(change: CommitLs<B>) -> Self {
        Change::Ls(change)
    }
}

impl<B> From<CatBlob<B>> for Change<B> {
    #[inline(always)]
    fn from(change: CatBlob<B>) -> Self {
        Change::CatBlob(change)
    }
}
