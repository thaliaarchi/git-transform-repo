# Strange and invalid Git object formats

## `git fsck` reports

A list of all error messages reported by `git fsck` and known violations of them
in the wild. The messages are sourced from what fsck [prints](https://git.kernel.org/pub/scm/git/git.git/tree/fsck.c?id=3bd955d26919e149552f34aacf8a4e6368c26cec),
rather than from [its documentation](https://git-scm.com/docs/git-fsck#_fsck_messages).

- `badDate`: invalid author/committer line - bad date (ERROR, commit or tag)
- `badDateOverflow`: invalid author/committer line - date causes integer overflow (ERROR, commit or tag)
- `badEmail`: invalid author/committer line - bad email (ERROR, commit or tag)
- `badFilemode`: contains bad file modes (INFO, tree)

  - git.git: 98 trees

  The full set of mode bits was recorded until [e44794706e](https://git.kernel.org/pub/scm/git/git.git/commit/?id=e44794706eeb57f2ee38ed1604821aa38b8ad9d2)
  (Be much more liberal about the file mode bits, 2005-04-16).
  Added to fsck in [42ea9cb286](https://git.kernel.org/pub/scm/git/git.git/commit/?id=42ea9cb286423c949d42ad33823a5221182f84bf)
  (Be more careful about tree entry modes, 2005-05-05).

- `badName`: invalid author/committer line - bad name (ERROR, commit or tag)
- `badObjectSha1`: invalid 'object' line format - bad sha1 (ERROR, tag)
- `badParentSha1`: invalid 'parent' line format - bad sha1 (ERROR, commit)
- `badTagName`: invalid 'tag' name (INFO, tag)
- `badTimezone`: invalid author/committer line - bad time zone (ERROR, commit or tag)

  - rails/rails:
    `1312735823 +051800` in [4cf94979c9](https://github.com/rails/rails/commit/4cf94979c9f4d6683c9338d694d5eb3106a4e734)
    (edit changelog to mention about x_sendfile_header default change, 2011-08-29)

  Invalid time zones were [discussed](https://lore.kernel.org/git/CABPp-BFfa6q96qMUN07Dq3di6d3WuUzhyktBytbX=FGgarXgjg@mail.gmail.com/)
  on the Git mailing list. More issues have been reported in
  [newren/git-filter-repo#88](https://github.com/newren/git-filter-repo/issues/88).

  GitHub's incoming fsck checks have loosened this to allow time zones of any
  length [since August 2011](https://lore.kernel.org/git/20200521195513.GA1542632@coredump.intra.peff.net/).

- `badTree`: cannot be parsed as a tree (ERROR, tree)
- `badTreeSha1`: invalid 'tree' line format - bad sha1 (ERROR, commit)
- `badType`: invalid 'type' value (ERROR, tag)
- `duplicateEntries`: contains duplicate file entries (ERROR, tree)
- `emptyName`: contains empty pathname (WARN, tree)
- `extraHeaderEntry`: invalid format - extra header(s) after 'tagger' (IGNORE, tag)
- `fullPathname`: contains full pathnames (WARN, tree)

  Tree objects used absolute paths until [f768846e34](https://git.kernel.org/pub/scm/git/git.git/commit/?id=f768846e34997fb847c9b875615867d4716d632f)
  (Teach "fsck" and "read-tree" about recursive tree-nodes, 2005-04-09).
  Added to fsck in [4e6616ab77](https://git.kernel.org/pub/scm/git/git.git/commit/?id=4e6616ab77ed6a53f857d4b1082c4dc4140f34f5)
  (Make fsck-cache warn about old-style tree objects that have full pathnames in
  them, 2005-04-09).

- `gitattributesBlob`: non-blob found at .gitattributes (ERROR, any)
- `gitattributesLarge`: .gitattributes too large to parse (ERROR, blob)
- `gitattributesLineLength`: .gitattributes has too long lines to parse (ERROR, blob)
- `gitattributesMissing`: unable to read .gitattributes blob (ERROR, blob)
- `gitattributesSymlink`: .gitattributes is a symlink (INFO, tree)
- `gitignoreSymlink`: .gitignore is a symlink (INFO, tree)
- `gitmodulesBlob`: non-blob found at .gitmodules (ERROR, any)
- `gitmodulesLarge`: .gitmodules too large to parse (ERROR, blob)
- `gitmodulesMissing`: unable to read .gitmodules blob (ERROR, blob)
- `gitmodulesName`: disallowed submodule name (ERROR, blob)
- `gitmodulesParse`: could not parse gitmodules blob (INFO, blob)
- `gitmodulesPath`: disallowed submodule path (ERROR, blob)
- `gitmodulesSymlink`: .gitmodules is a symbolic link (ERROR, tree)
- `gitmodulesUpdate`: disallowed submodule update setting (ERROR, blob)
- `gitmodulesUrl`: disallowed submodule url (ERROR, blob)
- `hasDot`: contains '.' (WARN, tree)
- `hasDotdot`: contains '..' (WARN, tree)
- `hasDotgit`: contains '.git' (WARN, tree)
- `largePathname`: contains excessively large pathname (WARN, tree)
- `mailmapSymlink`: .mailmap is a symlink (INFO, tree)
- `missingAuthor`: invalid format - expected 'author' line (ERROR, commit)
- `missingCommitter`: invalid format - expected 'committer' line (ERROR, commit)
- `missingEmail`: invalid author/committer line - missing email (ERROR, commit or tag)
- `missingNameBeforeEmail`: invalid author/committer line - missing space before email (ERROR, commit or tag)
- `missingObject`: invalid format - expected 'object' line (ERROR, tag)
- `missingSpaceBeforeDate`: invalid author/committer line - missing space before date (ERROR, commit or tag)
- `missingSpaceBeforeEmail`: invalid author/committer line - missing space before email (ERROR, commit or tag)
- `missingTag`: invalid format - unexpected end after 'type' line (ERROR, tag)
- `missingTagEntry`: invalid format - expected 'tag' line (ERROR, tag)
- `missingTaggerEntry`: invalid format - expected 'tagger' line (INFO, tag)

  - git/git:
    [v0.99](https://git.kernel.org/pub/scm/git/git.git/tag/?h=v0.99)
  - torvalds/linux:
    [v2.6.11-tree](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.11-tree),
    [v2.6.12](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.12),
    [v2.6.12-rc2](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.12-rc2),
    [v2.6.12-rc3](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.12-rc3),
    [v2.6.12-rc4](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.12-rc4),
    [v2.6.12-rc5](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.12-rc5),
    [v2.6.12-rc6](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.12-rc6),
    [v2.6.13-rc1](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.13-rc1),
    [v2.6.13-rc2](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.13-rc2),
    [v2.6.13-rc3](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.13-rc3)

- `missingTree`: invalid format - expected 'tree' line (ERROR, commit)
- `missingType`: invalid format - unexpected end after 'type' line (ERROR, tag)
- `missingTypeEntry`: invalid format - expected 'type' line (ERROR, tag)
- `multipleAuthors`: invalid format - multiple 'author' lines (ERROR, commit)
- `nulInCommit`: NUL byte in the commit object body (WARN, commit)
- `nulInHeader`: unterminated header: NUL (FATAL, commit or tag)
- `nullSha1`: contains entries pointing to null sha1 (WARN, tree)
- `treeNotSorted`: not properly sorted (ERROR, tree)
- `unknownType`: unknown type (internal fsck error) (ERROR, unknown)
- `unterminatedHeader`: unterminated header (FATAL, commit or tag)
- `zeroPaddedDate`: invalid author/committer line - zero-padded date (ERROR, commit or tag)
- `zeroPaddedFilemode`: contains zero-padded file modes (WARN, tree)

  - rails/rails: 141 trees

  [Grit](https://github.com/mojombo/grit) (Git implementation in Ruby used by
  GitHub) [used to](https://lore.kernel.org/git/20200521185753.GB1308489@coredump.intra.peff.net/)
  create 0-prefixed tree modes. The fix was likely [3073a5c](https://github.com/mojombo/grit/commit/3073a5c70d8412e28b64c79fcba06061479a4642)
  (merge in tag listing fix, 2010-05-26).

## Strange object formats

- Non-commit tags:

  - torvalds/linux:
    [v2.6.11-tree](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.11-tree)

  Tags can point to objects other than commits. A Linux release from prior to
  its Git history tags a tree. Blobs and tags can also be tagged.

## Old object formats

- Old-style date parsing:

  Times used to first be parsed as seconds with `strtoul(buf, NULL, 10)` and
  falling back to `strptime` with any of the formats `"%s"`, `"%c"`, or
  `"%a %b %d %T %y"`, if it failed. The `strptime` fallbacks were removed in
  [89d21f4b64](https://git.kernel.org/pub/scm/git/git.git/commit/?id=89d21f4b649d5d31b18da3220608cb349f29e650)
  (Move "parse_commit()" into common revision.h file, 2005-04-17).

  It seems even with this change, time zones were not yet added.
