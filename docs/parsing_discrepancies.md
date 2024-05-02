# fast-import parsing discrepancies

## Paths

- `filemodify`: `'M' SP <mode> (<dataref> | 'inline') SP <path> LF`
- `filedelete`: `'D' SP <path> LF`
- `filerename`: `'R' SP <path> SP <path> LF`
- `filecopy`: `'C' SP <path> SP <path> LF`
- `ls`: `'ls' SP <dataref> SP <path> LF`
- `ls-commit`: `'ls' SP <path> LF`
- `ls-out`: `<mode> SP ('blob' | 'tree' | 'commit') SP <dataref> HT <path> LF`
- `ls-fail`: `'missing' SP <path> LF`

### Git

fast-import: Fixed

fast-export:

- `filemodify`, `filedelete`, `filerename-src`, `filerename-dest`,
  `filecopy-src`, `filecopy-dest`: Quotes `<path>` if it contains a control
  character (0x00–0x1F or 0x7F), SP, `"`, `\`, or (if `core.quotepath=true`) a
  non-ASCII byte (0x80–0xFF).

### BitKeeper

fast-import:

- `filemodify`, `filedelete`, `filerename-src`, `filerename-dest`,
  `filecopy-src`, `filecopy-dest`: If `<path>` starts with `"`, unquote it and
  report parse errors; otherwise, treat it as literal bytes until the next SP.
  (See [`parsePath`](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L836).)

fast-export:

- `filemodify`, `filedelete`: `<path>` is always unquoted.
  (See [`gitLine`](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-export.c#L252-L282).)

### Reposurgeon

fast-import:

- `filemodify`, `filedelete`, `filerename-src`, `filerename-dest`,
  `filecopy-src`, `filecopy-dest`: If `<path>` starts with `"`, unquote it and
  ignore parse errors; otherwise, treat it as literal bytes until the next SP.
  (See [`(*FileOp).parse`](https://gitlab.com/esr/reposurgeon/-/blob/b1739ef8b9ee6b38230d9d2fede343352dca2d6e/surgeon/inner.go#L2349).)

fast-export:

- `filemodify`, `filedelete`, `filerename-src`, `filerename-dest`,
  `filecopy-src`, `filecopy-dest`: If `<path>` contains SP, quote it.
  (See [`(*FileOp).Save`](https://gitlab.com/esr/reposurgeon/-/blob/b1739ef8b9ee6b38230d9d2fede343352dca2d6e/surgeon/inner.go#L2444).)
