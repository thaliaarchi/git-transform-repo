# Strange and invalid Git object formats

## `git fsck` reports

A list of all error messages reported by `git fsck` and known violations of them
in the wild. The messages are sourced from what fsck [prints](https://git.kernel.org/pub/scm/git/git.git/tree/fsck.c?id=3bd955d26919e149552f34aacf8a4e6368c26cec),
rather than from [its documentation](https://git-scm.com/docs/git-fsck#_fsck_messages).
Searching for `remote: fatal: fsck error` can help find more examples of pushes
that were rejected due to fsck errors when `receive.fsckObjects` is set on the
remote.

- `badDate`: invalid author/committer line - bad date (ERROR, commit or tag)
- `badDateOverflow`: invalid author/committer line - date causes integer overflow (ERROR, commit or tag)
- `badEmail`: invalid author/committer line - bad email (ERROR, commit or tag)
- `badFilemode`: contains bad file modes (INFO, tree)

  The full set of mode bits was recorded until [e44794706e](https://git.kernel.org/pub/scm/git/git.git/commit/?id=e44794706eeb57f2ee38ed1604821aa38b8ad9d2)
  (Be much more liberal about the file mode bits, 2005-04-16).
  Added to fsck in [42ea9cb286](https://git.kernel.org/pub/scm/git/git.git/commit/?id=42ea9cb286423c949d42ad33823a5221182f84bf)
  (Be more careful about tree entry modes, 2005-05-05).

  - git.git: 98 trees

- `badName`: invalid author/committer line - bad name (ERROR, commit or tag)
- `badObjectSha1`: invalid 'object' line format - bad sha1 (ERROR, tag)
- `badParentSha1`: invalid 'parent' line format - bad sha1 (ERROR, commit)
- `badTagName`: invalid 'tag' name (INFO, tag)
- `badTimezone`: invalid author/committer line - bad time zone (ERROR, commit or tag)

  `git commit` (via `parse_date_basic`) rejects timezone offsets with not
  exactly 4 digits as of [ee646eb48f](https://git.kernel.org/pub/scm/git/git.git/commit/?id=ee646eb48f9a7fc6c225facf2b7449a8a65ef8f2)
  (date.c: Support iso8601 timezone formats, 2011-09-09) [[mail](https://lore.kernel.org/git/1315320996-1997-1-git-send-email-lihaitao@gmail.com/)],
  released in v1.7.6.5 (2011-12-13). Before then, if an external source passed
  an invalid offset to `git commit`, it would accept it; it is unclear if the
  origin of the `+051800` offset was internal or external, though.

  Support for invalid timezone offsets was added to git fast-import in
  [d42a2fb72f](https://git.kernel.org/pub/scm/git/git.git/commit/?id=d42a2fb72f8cbe6efd60a4f90c8e9ec1c888c3a7)
  (fast-import: add new --date-format=raw-permissive format, 2020-05-30) [[mail](https://lore.kernel.org/git/pull.795.git.git.1590693313099.gitgitgadget@gmail.com/),
  [PR](https://github.com/git/git/pull/795)]. Invalid timezone offsets were
  [discussed](https://lore.kernel.org/git/CABPp-BFfa6q96qMUN07Dq3di6d3WuUzhyktBytbX=FGgarXgjg@mail.gmail.com/)
  on the Git mailing list preceding that patch.

  GitHub's incoming fsck checks were loosened this to allow timezone offsets of
  any length [in August 2011](https://lore.kernel.org/git/20200521195513.GA1542632@coredump.intra.peff.net/).

  GitLab allowed this error for receive in [gitlab-org/gitaly#1947](https://gitlab.com/gitlab-org/gitaly/-/merge_requests/1947)
  [0f0c64816](https://gitlab.com/gitlab-org/gitaly/-/commit/0f0c64816f772efe5ddcd5b72b84a413979700e3)
  (git: receivepack: Allow commits with invalid timezones to be pushed,
  2020-03-19) and fetch in [gitlab-org/gitaly#3458](https://gitlab.com/gitlab-org/gitaly/-/merge_requests/3458)
  [692a0d347](https://gitlab.com/gitlab-org/gitaly/-/commit/692a0d3476a5fe5832ec78df5a6d9d5e1d780364)
  (git: Always check fetched objects for consistency, 2021-05-03).

  - rails/rails: `1312735823 +051800` in [4cf94979c9](https://github.com/rails/rails/commit/4cf94979c9f4d6683c9338d694d5eb3106a4e734)
    (edit changelog to mention about x_sendfile_header default change, 2011-08-29)
  - psf/requests: `1313584730 +051800` in [5e6ecdad](https://github.com/psf/requests/commit/5e6ecdad9f69b1ff789a17733b8edc6fd7091bd8)
    (Typo in documentation, 2011-09-08)
  - `4559547106 -7349423` reported in [newren/git-filter-repo#88](https://github.com/newren/git-filter-repo/issues/88)
  - `5859358690 -43455309` reported in newren/git-filter-repo#88

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

  GitLab allowed this error for receive and fetch in [gitlab-org/gitaly#3640](https://gitlab.com/gitlab-org/gitaly/-/merge_requests/3640)
  [2da0b3939](https://gitlab.com/gitlab-org/gitaly/-/commit/2da0b393998d394b743c70e7cf9cd0757a8f2733)
  (git: Accept commits and tags with malformed signatures, 2021-07-01), because
  the most common case is where the date is missing completely, which they can
  handle parsing.

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

  git-cinnabar would [incorrectly convert](https://github.com/glandium/git-cinnabar/issues/228)
  Mercurial repos containing paths with double slashes, which yield duplicate
  paths and this error. This was fixed in [a473240c](https://github.com/glandium/git-cinnabar/commit/a473240c4de34fb90f45070c0c7dd5dbfcea9661)
  (When creating changeset git trees from manifests, merge foo//\* into foo/*,
  2019-10-10).

  go-git [did not check](https://github.com/go-git/go-git/issues/193) that trees
  were sorted when encoding until [go-git/go-git#967](https://github.com/go-git/go-git/pull/967)
  [1d4bec0](https://github.com/go-git/go-git/commit/1d4bec06f4538a4f9f6000eaf5a17921dc1b5128)
  (plumbing: object, check entry order in (*Tree).Encode, export
  TreeEntrySorter, 2023-12-13).

  GitPython would write incorrectly sorted trees until it was fixed in
  [gitpython-developers/GitPython#1799](https://github.com/gitpython-developers/GitPython/pull/1799)
  [365d44f5](https://github.com/gitpython-developers/GitPython/commit/365d44f50a3d72d7ebfa063b142d2abd4082cfaa)
  (fix: treeNotSorted issue, 2024-01-15).

  A commenter on a vague report of this error [hypothesizes](https://www.reddit.com/r/git/comments/wutlt7/comment/j1itdmy/)
  that some `treeNotSorted` errors are related to `core.protectNTFS` problems
  reported in [git-for-windows/git#2777](https://github.com/git-for-windows/git/issues/2777).

- `unknownType`: unknown type (internal fsck error) (ERROR, unknown)
- `unterminatedHeader`: unterminated header (FATAL, commit or tag)
- `zeroPaddedDate`: invalid author/committer line - zero-padded date (ERROR, commit or tag)
- `zeroPaddedFilemode`: contains zero-padded file modes (WARN, tree)

  Usually this error is from the directory file mode being encoded as `040000`,
  instead of `40000`.

  GitLab allowed this error for receive and fetch in [gitlab-org/gitaly#4051](https://gitlab.com/gitlab-org/gitaly/-/merge_requests/4051),
  [db8f2e8da](https://gitlab.com/gitlab-org/gitaly/-/commit/db8f2e8da5e7ff9cf84a99195481303016cd2138)
  (git: Ignore fsck errors for zero-padded filemodes, 2021-11-09).

  [Grit](https://github.com/mojombo/grit), a Git implementation in Ruby used by
  GitHub, [used to](https://lore.kernel.org/git/20200521185753.GB1308489@coredump.intra.peff.net/)
  write 0-padded tree modes until it was fixed in [mojombo/grit 3073a5c](https://github.com/mojombo/grit/commit/3073a5c70d8412e28b64c79fcba06061479a4642)
  (merge in tag listing fix, 2010-05-26).

  - [celery/celery](https://github.com/celery/celery): 2 trees
  - [ddnet/ddnet](https://github.com/ddnet/ddnet): 6 trees
  - [ohmyzsh/ohmyzsh](https://github.com/ohmyzsh/ohmyzsh): 3 trees
  - [rails/rails](https://github.com/rails/rails): 141 trees
  - [teeworlds/teeworlds](https://github.com/teeworlds/teeworlds): 6 trees

## Strange object formats

- Non-commit tags:

  Tags can point to objects other than commits. A Linux release from prior to
  its Git history tags a tree. Blobs and tags can also be tagged.

  - AutoHotkey/AutoHotkey:
    [v2.0-a078](https://github.com/AutoHotkey/AutoHotkey/tree/v2.0-a078)
    points to a tag
  - torvalds/linux:
    [v2.6.11-tree](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tag/?h=v2.6.11-tree)
    points to a tree

## Old object formats

- Old-style date parsing:

  Times used to first be parsed as seconds with `strtoul(buf, NULL, 10)` and
  falling back to `strptime` with any of the formats `"%s"`, `"%c"`, or
  `"%a %b %d %T %y"`, if it failed. The `strptime` fallbacks were removed in
  [89d21f4b64](https://git.kernel.org/pub/scm/git/git.git/commit/?id=89d21f4b649d5d31b18da3220608cb349f29e650)
  (Move "parse_commit()" into common revision.h file, 2005-04-17).

  It seems even with this change, timezone offsets had not yet been added.

## Object corruption

- A bug with `git rebase -i --root` in 2.18.0 caused corruption in the `author`
  header of the root commit. It was fixed in the [“sequencer: fix "rebase -i
  --root" corrupting author header”](https://lore.kernel.org/git/20180730092929.71114-1-sunshine@sunshineco.com/)
  patch series, merged in [1bc505b476](https://git.kernel.org/pub/scm/git/git.git/commit/?id=1bc505b4768e9e48592bebfff35e18c5277412da)
  (Merge branch 'es/rebase-i-author-script-fix', 2018-08-17), and released in
  2.19.0.
