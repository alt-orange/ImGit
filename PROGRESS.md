# Progress

Status as of 2026-10-25. ✅ implemented and exercised (headless screenshots or the `--script` input replay),
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
| Menu bar File / Edit / View / Help | ✅ | Open / Clone / Init, external editor / terminal / file manager, Undo / Redo, panels, zoom, shortcuts, about |
| Tabs bar: folder, repo tabs, + / New Tab, tabs list, bell, gear, profile | ✅ | Tabs list, notifications (toast history) and profile popovers |
| New Tab page | ✅ | Open / Clone / Create, search, Recent |
| Repository Management view | ✅ | Open / Favorites / Recent sections with branch, ahead / behind and WIP summaries |
| Repo tab groups, rename tab, reorder tabs | ⬜ | |
| Toolbar breadcrumbs *repository › branch* with dropdowns | ✅ | |
| Toolbar Undo / Redo / Pull ▾ / Push / Branch / Stash / Pop / Terminal / LFS ▾ / Actions / Search | ✅ | LFS menu (Fetch / Checkout / Pull / Push / Prune) runs `git lfs` on PATH |
| Toolbar labels on/off | ✅ | Preferences → UI |
| Status bar: activity / shortcuts / release notes icons, zoom select, version | ✅ | |
| Zoom (Ctrl+= / Ctrl+- / Ctrl+0) | ✅ | |
| Panels open and close at once; dialog / palette fade, toolbar hover fade | ✅ | |
| Panel resize: left panel 215–800, detail panel 353–(window − 687), terminal; col-resize cursor; sizes saved | ✅ | |
| Themes | ✅ | ImGit Dark (orange accent, the default), Ocean, Midnight Teal, Amethyst, Rose, Claude Code: Preferences → UI Customization or Ctrl+P → Switch Theme, applied at once |
| Light theme, custom themes | ⬜ | Text colours are still white-based in places |
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
| TAGS, STASHES, WORKTREES, SUBMODULES sections | ✅ | Worktrees are listed only; Add Submodule is the panel sliding out of the left panel (no remove) |
| Gitflow | ⬜ | |
| Branch cleanup | ⬜ | |

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
| Columns: BRANCH / TAG, GRAPH, COMMIT MESSAGE, AUTHOR, DATE / TIME, SHA, CHANGES with gear settings | ✅ | CHANGES only shows the WIP row's counts (no per-commit stats) |
| Column drag-reorder and resize | 🟡 | Resize (preferred widths, the last zone fills, handles in the header and over the rows, widths saved); drag a header cell to reorder the columns (the cell follows the mouse, the columns swap as it passes their centres, the order saved; "Reset columns" restores the default). A GRAPH zone narrower than its lanes scrolls them (Shift + wheel, horizontal wheel, scrollbar) under shadowed gutters, nodes pinned and fading at the edges, single-column mode below one lane |
| Commit search (Ctrl+F) with n of m navigation | ✅ | Floating bar over the graph |
| Multi-select (Ctrl / Shift), keyboard j / k / arrows / Home / End | ✅ | |
| Lazy loading / "Show more commits" | ✅ | |
| Timeline markers | 🟡 | A line and an age label ("1 hour ago", "yesterday", "3 weeks ago"...) on the first row past each age step |
| Scroll markers for HEAD / branches | ⬜ | |
| Compact graph mode | ⬜ | Setting exists, rendering is always "rich" |
| Pin branch to left | ✅ | Context menu |

## Right panel

| Feature | Status | Notes |
|---|---|---|
| WIP header: trash, "N file changes on [branch]" | ✅ | |
| Sort, Path \| Tree, folder counts ✎ / + / − | ✅ | Sort button has no effect yet |
| Unstaged / Staged / Conflicted sections, Stage all / Unstage All, per-file hover actions | ✅ | |
| Commit form: Commit / Stash tabs, amend, 72 counter, description, Commit options, commit button | ✅ | |
| Commit template, GPG signing, skip hooks | ⬜ | |
| Commit details: working-directory banner, commit sha, message view, author, parent, co-authors, change summary, Path / Tree, View all files | ✅ | View all files is a placeholder |
| Commit message height resize (row-resize handle, saved) and click-to-amend HEAD's message (Update Message / Cancel Amend) | ✅ | Rewording older commits from the detail panel is not done |
| Multi-select summary | ✅ | |
| File view: whole-file inline diff, split view, hunk view, file view, change navigation, overview rulers, stage / unstage from the header | ✅ | |
| Word wrap, whitespace, hunk / line staging, file history, blame, file editing | ⬜ | Buttons present |
| Merge conflict editor (A / B / Output) | ✅ | Opens from a conflicted file: three-way line merge of the index's base / ours / theirs, pick whole conflicts or single lines from A and B, conflict navigator, Reset, double-click Output to edit it by hand, Save writes and stages the file; Open in external merge tool runs `git mergetool`. No syntax colouring, panes not resizable |
| Interactive rebase (pick / reword / squash / drop, P R S D, reorder with Ctrl + ↑ / ↓, Reset) | ✅ | Takes the left panel's and graph's place; rows show the commit's lane node and streak; reword edits the message in a floating editor; the right panel shows the selected row's commit |

## Git operations

| Feature | Status | Notes |
|---|---|---|
| Checkout branch / remote branch / tag / commit | ✅ | |
| Create / rename / delete branch, set upstream | ✅ | Renaming asks for the new name in the prompt bar; pushing a branch without an upstream asks there too (remote select, branch name) |
| Text field right-click menu | ✅ | Cut / Copy / Paste / Select All; no spelling suggestions |
| Create (annotated) tag, delete tag, push tag | ✅ | |
| Stage / unstage / discard files or all, commit, amend, push after commit | ✅ | |
| Stash push / apply / pop / drop | ✅ | |
| Merge (ff or merge commit), rebase (git CLI), reset soft / mixed / hard | ✅ | |
| Cherry pick (single / multi), revert | ✅ | |
| Squash, drop, edit message of any commit on the branch, interactive rebase | ✅ | Refused across merge commits; conflicts roll back |
| Fetch / pull (ff, ff-only, rebase) / push / clone / delete remote refs | ✅ | Through `git` on PATH |
| Credentials prompt (username / password or token, Remember me) | ✅ | Tested against a local HTTP remote requiring Basic auth |
| Auto-fetch | 🟡 | Timer runs `git fetch --all --prune` |
| Add / edit / remove remote | ✅ | |
| Undo / Redo | ✅ | checkout, commit, branch / tag create / delete / rename, reset, stash, merge, cherry pick, revert, squash, drop, rebase; uncommitted changes are auto-stashed around undo |
| Abort in-progress merge / rebase / cherry-pick | 🟡 | Toolbar banner |
| Worktree create / remove, submodule add / update | ⬜ | |
| Patch files | ✅ | Create patch from commits, apply patch |

## Other

| Feature | Status | Notes |
|---|---|---|
| Command palette (Ctrl+P) | ✅ | Unsupported commands are placeholders |
| Toasts | ✅ | Bottom-left cards |
| Confirmations | ✅ | The prompt bar over the toolbar |
| Preferences | ✅ | General, UI Customization and Commit options drive ImGit's settings, LFS and sparse checkout run git, the rest are stored |
| Profiles | ✅ | Local profiles (`.imgit/profiles/<guid>/`), each with its own settings, tabs, recent and favourite repositories and commit author (used unless the repository sets its own `user.name` / `user.email`); Add / Edit Profile modal with "Copy settings from current profile", Delete profile confirmation, the profile table's ⋮ menu; switch from the profile menu, the Preferences sidebar, the table or Ctrl+P → Switch to Profile; "Keep my .gitconfig updated" writes the author to the global `.gitconfig` |
| Keyboard Shortcuts dialog, About | ✅ | |
| Terminal panel | ✅ | A real PowerShell on ConPTY with a VT/xterm screen (colours, cursor addressing, scroll regions, alternate screen, 5000 lines of scrollback), 6x16 Consolas cells, tabs per repository, Minimize / Kill All / Hide, selection, copy / paste, focus dimming |
| Native file / folder dialogs | ✅ | |
| Linux | 🟡 | Builds and runs in WSL; see `PORTING.md` |

## Known nits

- Edit Commit Message dialog opens with the cursor at the start of the message.
- Toasts can cover the interactive rebase panel's buttons until they expire.
- Hidden refs are kept in memory only; they reset when the tab is closed.
