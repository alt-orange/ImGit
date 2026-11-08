# Progress

Status as of 2026-10-26. ✅ implemented and exercised (headless screenshots or the `--script` input replay),
🟡 implemented but not driven by hand, ⬜ not implemented.

## Look

| Feature | Status | Notes |
|---|---|---|
| Open Sans at CSS sizes | ✅ | CSS sizes are em sizes; ImGui sizes by ascent+descent, so sizes go through a per-face factor (1.36 for Open Sans) |
| Chromium-style text rendering | ✅ | DirectWrite ClearType glyphs, drawn by a two-pass per-channel blending renderer with 1/3 px sub-pixel positioning; grey-scale in the graph. Non-Windows builds fall back to FreeType |
| Font Awesome Free icons | ✅ | SVG paths rasterised into cached textures (rotation supported) |
| Gravatar avatars | ✅ | md5 of the lowercase email, downloaded in the background and cached on disk; initials without a Gravatar or until it loads |
| Dark Windows caption, Segoe UI menu bar | ✅ | Menus are drawn like Windows 11 native menus (rows, separators, accelerators, submenus, shadow) |

## Window chrome

| Feature | Status | Notes |
|---|---|---|
| Menu bar File / Edit / View / Help | ✅ | Open / Clone / Init, external editor / terminal / file manager, Undo / Redo, panels, zoom, shortcuts, about; View → Open Tabs List, and Favorites → Open Favorite Repo 1 to 9 (Alt+Ctrl+1 to 9, the starred repositories in Repository Management's order). ImGit has no accounts, so there is no Sign Out |
| Tabs bar: folder, repo tabs, + / New Tab, tabs list, bell, gear, profile | ✅ | Tabs list, notifications (toast history with the All / Read / Unread filter, read on close or by the eye) and profile popovers |
| New Tab page | ✅ | Open / Clone / Create, search, Recent |
| Repository Management view | ✅ | Open / Favorites / Recent sections with branch, ahead / behind and WIP summaries. Each section's tools: Fetch all, Pull all ▾ (fast-forward only / rebase), Open all in tabs and a ⋮ menu (those, Collapse / Expand, Remove all favorites); the All repositories rows' ⋮ menu (open, open in a new window, file manager, favourite, remove from recent). Open in a new window starts a second ImGit on the repository |
| Reorder tabs | 🟡 | Drag a repository tab along the bar (the drag-and-drop cursor, the tab over the others, swapping as its centre passes a neighbour's); the order is saved with the tabs |
| Repo tab groups, rename tab | ✅ | Tab menu → Rename tab / Reset tab name (the prompt bar), Add tab to group (New group... or an existing one; the tab moves beside the group) / Remove tab from group. A group is a coloured label in front of its tabs (a click collapses it to the label and its tab count, the active tab staying shown) and a line of its colour under them; the label's menu has Rename / Expand-Collapse / Next color / Ungroup / Close group. Names and groups are saved with the tabs |
| Repository aliases | ✅ | Tab menu → Alias repository (empty removes it), kept per profile in aliases.txt; shown in the tab, the tabs list, the repository breadcrumb and its recent list, the New Tab page and Repository Management |
| Toolbar breadcrumbs *repository › branch* with dropdowns | ✅ | |
| Toolbar Undo / Redo / Pull ▾ / Push / Branch / Stash / Pop / Terminal / LFS ▾ / Actions / Search | ✅ | LFS menu (Fetch / Checkout / Pull / Push / Prune) runs `git lfs` on PATH |
| Toolbar labels on/off | ✅ | Preferences → UI |
| Status bar: activity / shortcuts / release notes icons, zoom select, version | ✅ | |
| Zoom (Ctrl+= / Ctrl+- / Ctrl+0) | ✅ | |
| Panels open and close at once; dialog / palette fade, toolbar hover fade | ✅ | |
| Panel resize: left panel 215–800, detail panel 353–(window − 687), terminal; col-resize cursor; sizes saved | ✅ | |
| Themes | ✅ | ImGit Dark (orange accent, the default), ImGit Light, Ocean, Midnight Teal, Amethyst, Rose, Claude Code: Preferences → UI Customization or Ctrl+P → Switch Theme, applied at once |
| Light theme, custom themes | ✅ | A light background derives its own surfaces, dark text, darker lanes, Windows' light context menus and a light terminal; text and overlays use the theme's ink instead of white. Custom themes: *.json files in the profile's themes folder (a name, the built-in base, optional background / accent / link colours, palette colours by name), with Reload / Open Folder in UI Customization and a toast per file that cannot be used. The window caption and the menu bar stay dark |
| Remember tabs, recent repositories | ✅ | |

## Left panel

| Feature | Status | Notes |
|---|---|---|
| Collapse arrow, "Viewing N", filter box | ✅ | |
| Sections collapsed by default, section height algorithm, draggable section borders | ✅ | |
| LOCAL / REMOTE folder trees, counts, checked-out mark, ahead / behind arrows, semver-sorted tags | ✅ | |
| Collapsed 36px strip with section icons and counts (also while a file is open) | ✅ | |
| Hover eye / eye-slash (hide in the graph), ellipsis menu, Solo | ✅ | |
| Create branch / add remote / create tag | ✅ | From the context menus |
| Double-click to checkout, right-click menus | ✅ | |
| Drag branch onto branch → Merge / Rebase / Reset menu | ✅ | Fast-forward, merge, rebase, interactive rebase, push, reset; gold drop targets, the 75% drag image and the Windows drag cursors |
| TAGS, STASHES, WORKTREES, SUBMODULES sections | ✅ | A worktree row's menu opens it, copies its path or removes it; Add Submodule is the panel sliding out of the left panel |
| Edit / open / delete a submodule | 🟡 | Double-click a submodule: the Edit Submodule panel slides out (Remote URL via `git submodule set-url`, in-sync / out of sync / not initialized with Update / Initialize, the checked-out commit, Open / Delete). Open shows it in the same tab, *repository › submodule ✕ › branch*; the cross goes back |
| Gitflow | ✅ | Preferences → Gitflow → Initialize Gitflow writes the branch names and prefixes to the repository's `gitflow.*` configuration (what `git flow init` sets) and creates the develop branch from main. Once `gitflow.branch.master` is set, a GITFLOW left-panel section lists the feature / release / hotfix branches and its "+" (also the LOCAL section menu and Ctrl+P "Gitflow: Start …") starts one: `<prefix><name>` from develop (hotfix: main), checked out. Finish (a gitflow branch's context menu, Ctrl+P "Gitflow: Finish …" on the checked-out one) asks first, then runs git: a feature merges into develop with `--no-ff`; a release / hotfix merges into main with `--no-ff`, is tagged `<versiontag><name>` (annotated), merges into develop; the branch is deleted. A merge conflict stops there in git's normal merge state; after committing the resolution, Finish again completes it (merges already done are no-ops, an existing tag is kept). Needs a working directory without changes (untracked files aside). A hotfix finished while a release branch exists still merges into develop (git-flow merges it into the release). The 2026-11-08 runs wrote the `gitflow.*` config directly rather than through the Initialize button |
| Branch cleanup | ✅ | LOCAL section menu → Clean Up Branches..., or Ctrl+P "Clean Up Branches": a review list of the local branches merged into the default branch (origin/HEAD's branch, else gitflow's main, else main / master) or whose upstream is gone (`[gone]` after a pruning fetch), each ticked, with why it is offered; the ticked ones are deleted (one Undo entry restores them all). Never offers the checked-out branch (here or in another worktree), the default branch or gitflow's main / develop. A branch at the default branch's tip counts as merged, as `git branch --merged` does |

## Commit graph

| Feature | Status | Notes |
|---|---|---|
| Lane / edge algorithm, 10 lane colours, arcs, dashed WIP / stash lines | ✅ | Straight edges are pixel-aligned 2px strokes, continuous across rows |
| Commit order | ✅ | `git rev-list --date-order` from the visible refs in name order, then the stashes (libgit2's topological walk without git) |
| Avatar nodes, merge nodes, stash nodes, WIP node | ✅ | |
| Ref labels grouped by name (local / remote / tag icons), checked-out first, "+N" overflow with a hover list | ✅ | |
| Ghost refs (the nearest descendant's refs, dimmed) on hover / selection | ✅ | |
| Inline "enter branch name" / "enter tag name" in the ref zone | ✅ | |
| Highlight rows when hovering a branch label | ✅ | Rows whose own or child refs miss the hovered group fade after 1s; Ctrl dims around the selected commit's branch |
| Commit message + dimmed description | ✅ | |
| Columns: BRANCH / TAG, GRAPH, COMMIT MESSAGE, AUTHOR, DATE / TIME, SHA, CHANGES with gear settings | ✅ | Every column can be hidden from the header menu but the last one shown. CHANGES counts each commit's modified / added / deleted files: read on a worker thread for the rows on screen and cached by sha |
| Column drag-reorder and resize | 🟡 | Resize (preferred widths, the last zone fills, handles in the header and over the rows, widths saved); drag a header cell to reorder the columns (the cell follows the mouse, the columns swap as it passes their centres, the order saved; "Reset columns" restores the default). A GRAPH zone narrower than its lanes scrolls them (Shift + wheel, horizontal wheel, scrollbar) under shadowed gutters, nodes pinned and fading at the edges, single-column mode below one lane |
| Commit search (Ctrl+F) with n of m navigation | ✅ | Floating bar over the graph |
| Multi-select (Ctrl / Shift), keyboard j / k / arrows / Home / End | ✅ | |
| Lazy loading / "Show more commits" | ✅ | |
| Timeline markers | 🟡 | A line and an age label ("1 hour ago", "yesterday", "3 weeks ago"...) on the first row past each age step |
| Scroll markers for HEAD / branches | ✅ | On the rows' scrollbar: HEAD a full-width accent bar, local branches a tick in their lane colour (remote ones fainter), the selected commits a thin bar |
| Smart Branch Visibility | ✅ | Graph header menu: the graph walks only the checked-out branch, its upstream and the default branch (the remote's HEAD, else main / master, local and remote); a soloed ref still wins. Saved in the settings |
| Compact graph mode | ✅ | 20px rows, 14px lanes, plain 10px dots without avatars and 16px ref pills (header menu → Compact Graph Column) |
| Pin branch to left | ✅ | Context menu |

## Right panel

| Feature | Status | Notes |
|---|---|---|
| WIP header: trash, "N file changes on [branch]" | ✅ | |
| Sort, Path \| Tree, folder counts ✎ / + / − | ✅ | Sort: path A to Z (default) or Z to A, file name, or status (conflicted, added, modified, renamed, deleted); folders first by name for the last two; one setting for every file list, saved |
| Unstaged / Staged / Conflicted sections, Stage all / Unstage All, per-file hover actions | ✅ | |
| Commit form: Commit / Stash tabs, amend, 72 counter, description, Commit options, commit button | ✅ | |
| Commit template, GPG signing, skip hooks | ✅ | The form starts from `commit.template` (comment lines left out; again after each commit). Commit options: Sign commit with GPG (`-S`, ticked by `commit.gpgsign`) and Skip commit hooks (`--no-verify`). A commit goes through `git commit` when it is signed or the repository has commit hooks (libgit2 runs neither), otherwise through libgit2 |
| Commit details: working-directory banner, commit sha, message view, author, parent, co-authors, change summary, Path / Tree, View all files | ✅ | View all files lists every file of the commit's tree (libgit2 tree walk): the changes with their status, the rest with a plain file icon; clicking an unchanged file shows it as the commit left it (File History view) |
| Commit message height resize (row-resize handle, saved) and click-to-amend the message (Update Message / Cancel Amend) | ✅ | HEAD's message, or that of any commit of the current branch's first-parent chain above its last merge commit (rebuilt with the history rewrite, like the menu's Edit commit message; needs a clean working directory, one Undo entry); commits below a merge are not editable. The reworded commit stays selected |
| Multi-select summary | ✅ | |
| Compare a commit against the working directory | ✅ | The commit menu's Compare commit against working directory, or the WIP row and one commit selected together: the files that differ (staged, unstaged and new files included), each opening its diff (commit → working directory) |
| File view: whole-file inline diff, split view, hunk view, file view, change navigation, overview rulers, stage / unstage from the header | ✅ | |
| File history | ✅ | Blame / History in the file view's toolbar, File History / File Blame in the file menus: the commits that changed the file (libgit2 walk, renames followed) down the left, the file as the selected commit left it or its diff on the right, Esc or the cross closes it |
| Blame | ✅ | The file view in the File History view: each line's commit in the gutter (the author's avatar at each run's first line, a bar in the commit's colour); the Blame toggle adds the commit's summary and date; clicking the gutter selects that commit; hovering it shows the commit |
| Syntax highlighting | 🟡 | In the diff / file / File History editors and the merge tool's A / B panes; Preferences → Editor → Syntax Highlighting (on by default). A small line-based tokenizer (`src/ui/syntax/`), Monaco's vs-dark token colours (vs on a light background) from the theme. Own scanners for C, C++, C#, Java, Kotlin, Scala, Go, Rust, Dart, JavaScript, TypeScript, PHP, Python, Ruby, Lua, shell, PowerShell, SQL, JSON, YAML, INI, XML, HTML (with `<script>` / `<style>`), CSS / SCSS / Less, Markdown (fenced code by its language) and Dockerfile; keyword / comment / string tables for the rest of the list in `languages.cpp`. Each diff hunk starts from a fresh state (a hunk that starts inside a comment can be mis-coloured); the merge panes are tokenized whole. Lines over 2,000 characters and files over 5,000 lines or 1 MiB are plain. Unit tests in `test/` |
| Hunk / line staging | ✅ | Working-directory diffs: Stage / Unstage Hunk on each hunk header (hunk view); click a changed line's number gutter to pick it (Shift: a range), then Stage / Unstage N Lines in the header. A partial patch goes through `git apply --cached` (reversed to unstage); every change picked stages the whole file |
| Word wrap, whitespace | ✅ | Toolbar toggles, saved in the settings (Word Wrap also in Preferences → Editor). Wrap breaks long lines at the column's width (whole characters) in the hunk / inline / split / file views and File History; the whitespace toggle shows whitespace-only changes and draws the spaces (·) and tabs (→) of changed lines, off it ignores whitespace-only changes |
| File editing, Create file | ✅ | Edit This File / Edit in Working Directory and the file menus' Edit file open the working directory's file as text in the file view (Save or Ctrl+S writes it, keeping CRLF files CRLF; Cancel and the file view's cross ask before dropping unsaved changes, Esc closes only an unchanged editor); binary files and files over 8 MiB are refused. A folder's "Create a file in this folder" asks for a name (sub-folders allowed) in the prompt bar, creates the empty file and opens it in the editor. The editor is plain text: no syntax colours or line numbers |
| Merge conflict editor (A / B / Output) | ✅ | Opens from a conflicted file: three-way line merge of the index's base / ours / theirs, pick whole conflicts or single lines from A and B, conflict navigator, Reset, double-click Output to edit it by hand, Save writes and stages the file; Open in external merge tool runs `git mergetool`. A / B and the Output are syntax highlighted (not while the Output is edited by hand); drag handles between A and B and above the Output resize the panes, the split saved in the settings |
| Interactive rebase (pick / reword / squash / drop, P R S D, reorder with Ctrl + ↑ / ↓, Reset) | ✅ | Takes the left panel's and graph's place; rows show the commit's lane node and streak; reword edits the message in a floating editor; the right panel shows the selected row's commit |

## Git operations

| Feature | Status | Notes |
|---|---|---|
| Checkout branch / remote branch / tag / commit | ✅ | |
| Create / rename / delete branch, set upstream | ✅ | Delete all "x" branches deletes the local one (undoable) and every remote one at a commit; renaming asks for the new name in the prompt bar; pushing a branch without an upstream asks there too (remote select, branch name) |
| Text field right-click menu | ✅ | Cut / Copy / Paste / Select All; no spelling suggestions |
| Create (annotated) tag, delete tag, push tag | ✅ | |
| Stage / unstage / discard files or all, commit, amend, push after commit | ✅ | |
| Stash push / apply / pop / drop, edit stash message | ✅ | Edit stash message asks in the prompt bar and rewrites the stash's reflog entry in place |
| Merge (ff or merge commit), rebase (git CLI), reset soft / mixed / hard | ✅ | |
| Cherry pick (single / multi), revert | ✅ | |
| Squash, drop, edit message of any commit on the branch, interactive rebase | ✅ | Refused across merge commits; conflicts roll back |
| Fetch / pull (ff, ff-only, rebase) / push / clone / delete remote refs | ✅ | Through `git` on PATH |
| Credentials prompt (username / password or token, Remember me) | ✅ | Tested against a local HTTP remote requiring Basic auth |
| Auto-fetch | 🟡 | Timer runs `git fetch --all --prune` |
| Add / edit / remove remote | ✅ | |
| Undo / Redo | ✅ | checkout, commit, branch / tag create / delete / rename, reset, stash, merge, cherry pick, revert, squash, drop, rebase; uncommitted changes are auto-stashed around undo |
| Abort in-progress merge / rebase / cherry-pick | 🟡 | Toolbar banner |
| Worktree create / remove | ✅ | Create worktree from a branch (a remote one through a local branch tracking it) or a commit (detached): a dialog asks for the folder, `git worktree add` runs; Remove / Remove and discard its changes (`git worktree remove [--force]`) from the worktree's row |
| Submodule add / update | ✅ | Add Submodule panel from the SUBMODULES section; Update / Initialize in the Edit Submodule panel |
| Patch files | ✅ | Create patch from commits, apply patch |

## Other

| Feature | Status | Notes |
|---|---|---|
| Command palette (Ctrl+P) | ✅ | Every command works. Blame, History, View / Edit / Open / Delete File and Open in External Diff/Merge Tool list the repository's files (HEAD's tree plus new files) to pick from; Activity Logs opens the notification history; Create File asks for a name |
| Toasts | ✅ | Bottom-left cards |
| Confirmations | ✅ | The prompt bar over the toolbar |
| Preferences | ✅ | General, UI Customization and Commit options drive ImGit's settings, LFS and sparse checkout run git, the rest are stored. SSH → Generate makes an ed25519 key pair in ~/.ssh (`ssh-keygen`) and selects it; Commit Signing lists gpg's secret keys (refresh button), choosing one sets `user.signingkey` in the global .gitconfig, and Generate makes a 4096-bit RSA signing key for the profile's name and email (gpg on PATH, else Git for Windows' own) and selects it; UI Customization's default colour for new tab groups (next colour, or a lane colour) with a reset for the groups already there |
| Connections | 🟡 | Preferences → Connections: GitHub, GitLab, Bitbucket and Azure DevOps, plus any other host an open repository has a remote on. HTTPS: the username entered this session, else what the Git credential helper has stored (`git credential fill`, never interactive); Forget drops a session credential or, after a confirmation, the stored one (`git credential reject`). SSH: `ssh -T` in BatchMode, reporting the account the service greets (or why it refused). Checked in the background when the page opens and by Check Again; the parsing has unit tests (`test/connections_test.cpp`). Forget not driven by hand |
| Profiles | ✅ | Local profiles (`.imgit/profiles/<guid>/`), each with its own settings, tabs, recent and favourite repositories and commit author (used unless the repository sets its own `user.name` / `user.email`); Add / Edit Profile modal with "Copy settings from current profile", Delete profile confirmation, the profile table's ⋮ menu; switch from the profile menu, the Preferences sidebar, the table or Ctrl+P → Switch to Profile; "Keep my .gitconfig updated" writes the author to the global `.gitconfig` |
| Keyboard Shortcuts dialog, About | ✅ | |
| Terminal panel | ✅ | A real PowerShell on ConPTY with a VT/xterm screen (colours, cursor addressing, scroll regions, alternate screen, 5000 lines of scrollback), 6x16 Consolas cells, tabs per repository, Minimize / Kill All / Hide, selection, copy / paste, focus dimming |
| Native file / folder dialogs | ✅ | |
| Linux | 🟡 | Builds and runs in WSL; see `PORTING.md` |

## Known nits

- Edit Commit Message dialog opens with the cursor at the start of the message.
- Toasts can cover the interactive rebase panel's buttons until they expire.
- Hidden refs are kept in memory only; they reset when the tab is closed.
