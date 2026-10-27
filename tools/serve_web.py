#!/usr/bin/env python3
"""Serves ImGit's browser build on http://localhost:8000 (or the port given).

    python3 tools/serve_web.py [build/web-release] [--port 8000]

then open http://localhost:8000/imgit.html. The page's worker threads share the WebAssembly memory, which browsers
allow only on a cross-origin isolated page, so every response carries the COOP / COEP headers a static web server
would need to send as well.
"""

import argparse
import functools
import http.server


class IsolatedHandler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map, '.wasm': 'application/wasm'}

    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        self.send_header('Cache-Control', 'no-cache')
        super().end_headers()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('directory', nargs='?', default='build/web-release')
    parser.add_argument('--port', type=int, default=8000)
    args = parser.parse_args()
    handler = functools.partial(IsolatedHandler, directory=args.directory)
    with http.server.ThreadingHTTPServer(('localhost', args.port), handler) as server:
        print(f'Serving {args.directory} on http://localhost:{args.port}/imgit.html')
        server.serve_forever()


if __name__ == '__main__':
    main()
