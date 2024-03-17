# fast-import parsing discrepancies

## Paths

- Input:
  - `filemodify`: `'M' SP <mode> (<dataref> | 'inline') SP <path> LF`
  - `filedelete`: `'D' SP <path> LF`
  - `filerename`: `'R' SP <path> SP <path> LF`
  - `filecopy`: `'C' SP <path> SP <path> LF`
  - `ls`: `'ls' SP <dataref> SP <path> LF`
  - `ls-commit`: `'ls' SP <path> LF`
- Output:
  - `ls-out`: `<mode> SP ('blob' | 'tree' | 'commit') SP <dataref> HT <path> LF`
  - `ls-fail`: `'missing' SP <path> LF`

### git fast-import (implementation)

- `filemodify`, `filedelete`, `filerename-dest`, `filecopy-dest`: If `<path>` is
  a valid quoted string, unquote it; otherwise, treat it as literal bytes
  (including SP).
- `filerename-src`, `filecopy-src`: If `<path>` is a valid quoted string,
  unquote it; otherwise, treat it as literal bytes until the next SP.
- `ls`: If `<path>` starts with `"`, unquote it and report parse errors;
  otherwise, treat it as literal bytes (including SP).
- `ls-commit`: Unquote `<path>` and report parse errors (it must start with `"`
  to disambiguate from `ls`).
- `ls-out`, `ls-fail`: `<path>` is always printed quoted.

### git fast-import (docs)

General:

> A `<path>` string must use UNIX-style directory separators (forward slash
> `/`), may contain any byte other than `LF`, and must not start with double
> quote (`"`).
>
> A path can use C-style string quoting; this is accepted in all cases and
> mandatory if the filename starts with double quote or contains `LF`. In
> C-style quoting, the complete name should be surrounded with double quotes,
> and any `LF`, backslash, or double quote characters must be escaped by
> preceding them with a backslash (e.g., `"path/with\n, \\ and \" in it"`).
>
> The value of `<path>` must be in canonical form. That is it must not:
>
> contain an empty directory component (e.g. `foo//bar` is invalid),
>
> end with a directory separator (e.g. `foo/` is invalid),
>
> start with a directory separator (e.g. `/foo` is invalid),
>
> contain the special component `.` or `..` (e.g. `foo/./bar` and `foo/../bar`
> are invalid).
>
> The root of the tree can be represented by an empty string as `<path>`.
>
> It is recommended that `<path>` always be encoded using UTF-8.

`filecopy-src`, `filerename-src`:

> To use a source path that contains SP the path must be quoted.

`ls-commit`:

> The path must be quoted in this case.

`filemodify`, `filedelete`, `filerename-dest`, `filecopy-dest`, `ls`, `ls-out`,
`ls-fail`:

No particular notes.

### bk fast-import

- `filemodify`, `filedelete`, `filerename-src`, `filerename-dest`,
  `filecopy-src`, `filecopy-dest`: If `<path>` starts with `"`, unquote it and
  report parse errors; otherwise, treat it as literal bytes until the next SP.
  (See [`parsePath`](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L836).)
- `ls`, `ls-commit`, `ls-out`, `ls-fail`: `ls` command is not supported.
