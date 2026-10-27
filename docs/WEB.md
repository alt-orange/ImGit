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

The app runs libgit2, the status poll, the file history and the avatar downloads on worker threads. Those threads
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

- The whole local UI on repositories in the browser's storage: the graph, diffs, staging, commits, branches, tags,
  stashes, merges, rebases, file history and blame, themes and preferences. libgit2 does all of it.
- **Storage.** The home folder `/home/web_user` lives in IndexedDB. It holds the settings, profiles, recent and open
  tabs, the avatar cache and any repository created there. It is loaded before the app starts. It is written back
  every 10 seconds, whenever the tab is hidden, and before a relaunch. Clearing the site's data deletes it.
- **Repositories.** *Init* creates one at a path you type, such as `/home/web_user/notes`. *Open* asks for a path
  with the browser's prompt.
- **Gravatars**, through the browser's fetch (gravatar.com allows any origin).
- **Links** (*Help* and the like) open in a new tab. *Relaunch* reloads the page.
- **Hi-DPI.** Text is rasterised at the device pixel ratio, as on a Retina Mac.

## What does not (yet)

- **Network operations**: clone, fetch, pull and push. On the desktop they run the `git` executable, and a page
  cannot start programs. A page cannot open SSH or raw sockets either. A later step could give libgit2 an HTTP
  transport over `fetch()`, for servers that allow cross-origin requests.
- **Opening a folder from your disk.** The page sees only its own storage. There is no import or export of a
  repository yet.
- **The terminal panel** has no shell to run. *Open in File Manager*, *Open External Terminal* and *Open in Editor*
  do nothing.
- **Patch files.** *Save / Apply Patch* need a file dialog, and the page has none.
- **Full screen.** The browser's own full screen (F11) works. The app's *Toggle Full Screen* does nothing.
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
| `src/platform/window_web.cpp` | the canvas sized to the browser window, CSS cursors, layout scale 1 |
| `src/platform/http_web.cpp` | `emscripten_fetch` (synchronous, on the avatar worker thread) |
| `src/platform/*_web.cpp` | no processes, no pseudo console, `prompt()` as the folder picker, the embedded monospace font |
| `tools/serve_web.py` | a local server with the COOP / COEP headers |
| `tools/web_smoke.mjs` | CI's smoke test: headless Chromium opens the page and takes a screenshot |
