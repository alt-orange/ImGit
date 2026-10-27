# ImGit in the browser (WebAssembly)

ImGit builds with Emscripten into a page, `imgit.html`, with `imgit.js` and `imgit.wasm` next to it. It runs the same
UI as the desktop on WebGL 2. Repositories live in the browser's own storage.

## Building

You need [emsdk](https://emscripten.org/docs/getting_started/downloads.html) 3.1.55 or later, activated so that
`EMSDK` is set (CI uses 6.0.12). You also need CMake 3.25+ and Ninja. vcpkg is not used:
`cmake/WebDependencies.cmake` fetches and builds ImGui 1.92.8, FreeType 2.14.3, libgit2 1.9.4 and stb, the versions
`vcpkg.json`'s baseline gives the desktop. GLFW is emsdk's `contrib.glfw3` port.

```bash
source /path/to/emsdk/emsdk_env.sh
cmake --preset web-release
cmake --build --preset web-release
python3 tools/serve_web.py build/web-release      # then open http://localhost:8000/imgit.html
```

The app runs git, the status poll, the file history and the avatar downloads on worker threads. Those threads
share the WebAssembly memory, and a browser only allows that on a *cross-origin isolated* page. The server must send
these two headers with every file:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

`tools/serve_web.py` sends both. A plain `python3 -m http.server` does not, and the page then shows a message that
names the missing headers. On a static host that cannot set headers, such as GitHub Pages, a service worker like
`coi-serviceworker` can add them.

## What works

Everything the desktop does with repositories, with these differences:

- **Storage.** The home folder `/home/web_user` lives in IndexedDB. It holds the settings, profiles, recent and open
  tabs, the avatar cache, and every repository cloned, created or opened here. It is loaded before the app starts.
  It is written back every 10 seconds, whenever the tab is hidden, and before a relaunch. Clearing the site's data
  deletes it.
- **Git.** A page cannot run the `git` executable, so `src/git/web_git.cpp` stands in for it on libgit2.
  It handles everything ImGit hands to git:
  - clone, fetch, pull (fast-forward, merge or rebase) and push (force, force-with-lease, delete, set-upstream)
  - stash, apply, diff, format-patch, rebase, branch and checkout
  - merge / rebase / cherry-pick / revert `--abort`, and `rev-list`

  The terminal can also use init, status, log, add, commit, remote and config. `git help` lists them all.
- **Remotes** are reached over HTTP(S) with the browser's fetch, through a CORS proxy (below). ImGit's sign-in bar
  asks for the username and password (or token) on a 401, as on the desktop. In the terminal, put them in the URL
  (`https://user:token@host/...`). SSH remotes are not possible in a browser.
- **Opening a repository from disk.** *Open Repo* (Ctrl+O) asks the browser for a folder and copies it, `.git`
  included, into the home folder (`/home/web_user/<name>`). Changes stay in the browser's copy; push them to a
  remote to get them out.
- **File dialogs.** *Apply Patch* and the preferences' *Browse* buttons upload the picked file into the page.
  *Create patch* and the other saves hand the file to the browser as a download. *Init* and *Clone* destinations
  are paths in the browser's storage, typed in or chosen with *Browse* (a prompt).
- **The terminal panel** runs a small shell inside the page: `git` (the libgit2 one above), `cd`, `pwd`, `ls`,
  `cat`, `echo` (with `>` / `>>`), `mkdir`, `rm`, `clear`, `help` and `exit`. It has line editing and history
  (↑ / ↓), and Ctrl+C, Ctrl+L and Ctrl+U. A running command keeps typed-ahead keys until it finishes.
- **Full screen.** *View › Toggle Full Screen* puts the page in the browser's full screen and takes it out again.
- **Gravatars** come through the browser's fetch (gravatar.com allows any origin). **Links** open in a new tab.
  *Relaunch* reloads the page. **Hi-DPI:** text is rasterised at the device pixel ratio, as on a Retina Mac.

## The CORS proxy

Git hosts (GitHub, GitLab, ...) do not let pages from other origins call their git endpoints. The browser's git
therefore sends each request to `<proxy><url>`, for example
`http://localhost:8000/git-proxy/https://github.com/owner/repo.git/info/refs?service=git-upload-pack`.

- **The default** is the page's own server, at `/git-proxy/`. `tools/serve_web.py` provides it. It forwards only
  git's smart-HTTP endpoints, so it is not an open proxy. It also drops `WWW-Authenticate`, so the browser shows no
  sign-in box of its own.
- **`?git-proxy=<prefix>`** in the page's address, or `Module.imgitGitProxy` in `res/web/shell.html`, chooses
  another proxy, such as one deployed next to wherever the page is hosted. An empty prefix sends the requests
  directly, for git servers that allow the page's origin themselves.

Whoever runs the proxy sees the traffic, credentials included. Run your own.

## What does not work

- **SSH remotes**, submodule commands, Git LFS, sparse checkout, hooks and credential helpers: the browser's git
  reports that they are not available.
- **Writing back to the folder a repository was opened from.** The page works on its own copy.
- **External programs**: *Open in File Manager*, *Open External Terminal*, *Open in Editor* and the external diff
  and merge tools do nothing.
- **Fonts.** The UI uses Open Sans, as on Linux. The monospace face is DejaVu Sans Mono, embedded from
  `res/web/fonts`. No other system fonts are available.

## Where the code is

| File | Role |
|---|---|
| `cmake/WebDependencies.cmake` | the libraries, built from source with `-pthread` |
| `CMakeLists.txt` (`if(EMSCRIPTEN)`) | the web sources, the link options (thread pool, memory, WebGL 2, fetch, IDBFS, the page) |
| `res/web/shell.html` | the page: a full-window canvas and the loading / isolation message |
| `res/web/pre.js` | mounts IndexedDB at the home folder and stores it (`Module.imgitSyncFiles`) |
| `src/platform/main_loop_web.cpp` | the frames from `emscripten_set_main_loop`, the periodic and on-hide saves, the GLSL ES version |
| `src/platform/window_web.cpp` | the canvas sized to the browser window, CSS cursors, layout scale 1, the page's full screen |
| `src/platform/http_web.cpp` | `emscripten_fetch` (synchronous, on the avatar worker thread) |
| `src/git/web_git.cpp` | the in-process `git` on libgit2, and libgit2's smart HTTP transport over the browser's fetch |
| `src/platform/process_web.cpp`, `programs.hpp` | `run_process()` runs the app's registered in-process programs |
| `src/platform/pseudo_console_web.cpp` | the terminal panel's shell |
| `src/platform/shell_web.cpp` | file inputs for opening files and folders, downloads for saving, `prompt()` for locations, links |
| `src/platform/system_web.cpp` | the home folder, the embedded monospace font |
| `tools/serve_web.py` | a local server with the COOP / COEP headers and the git CORS proxy |
| `tools/web_smoke.mjs` | CI's smoke test: headless Chromium opens the page and takes a screenshot |
