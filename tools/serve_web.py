#!/usr/bin/env python3
"""Serves ImGit's browser build on http://localhost:8000 (or the port given).

    python3 tools/serve_web.py [build/web-release] [--port 8000]

then open http://localhost:8000/imgit.html. The page's worker threads share the WebAssembly memory, which browsers
allow only on a cross-origin isolated page, so every response carries the COOP / COEP headers a static web server
would need to send as well.

It is also the page's CORS proxy for git: git hosts do not let other origins fetch from them, so the browser's git
asks for /git-proxy/<url> here and this server makes the request. Only git's smart HTTP endpoints are forwarded
(info/refs?service=..., git-upload-pack, git-receive-pack), so it cannot be used as an open proxy.
"""

import argparse
import functools
import http.server
import re
import urllib.error
import urllib.request

PROXY_PREFIX = '/git-proxy/'
GIT_ENDPOINT = re.compile(r'^https?://[^/]+/.+/(info/refs\?service=git-(upload|receive)-pack|git-(upload|receive)-pack)$')
FORWARDED_HEADERS = ('Authorization', 'Content-Type', 'Accept', 'Content-Encoding', 'Git-Protocol')


class IsolatedHandler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map, '.wasm': 'application/wasm'}

    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        self.send_header('Cache-Control', 'no-cache')
        super().end_headers()

    def do_GET(self):
        if self.path.startswith(PROXY_PREFIX):
            self.proxy(None)
        else:
            super().do_GET()

    def do_POST(self):
        if not self.path.startswith(PROXY_PREFIX):
            self.send_error(405)
            return
        length = int(self.headers.get('Content-Length') or 0)
        self.proxy(self.rfile.read(length))

    def proxy(self, body):
        target = self.path[len(PROXY_PREFIX):]
        if not GIT_ENDPOINT.match(target):
            self.send_error(403, 'Only git smart HTTP requests are forwarded')
            return
        request = urllib.request.Request(target, data=body, method='POST' if body is not None else 'GET')
        for name in FORWARDED_HEADERS:
            if self.headers.get(name):
                request.add_header(name, self.headers[name])
        # Some hosts answer git's smart protocol only to a git user agent.
        request.add_header('User-Agent', 'git/2.47.0 (ImGit in the browser)')
        try:
            response = urllib.request.urlopen(request, timeout=120)
            status, headers, data = response.status, response.headers, response.read()
        except urllib.error.HTTPError as error:
            status, headers, data = error.code, error.headers, error.read()
        except (urllib.error.URLError, OSError) as error:
            self.send_error(502, f'Could not reach the git host: {error}')
            return
        self.send_response(status)
        # No WWW-Authenticate: the browser would show its own sign-in box; ImGit asks for credentials itself.
        for name in ('Content-Type', 'Cache-Control'):
            if headers.get(name):
                self.send_header(name, headers[name])
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)


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
