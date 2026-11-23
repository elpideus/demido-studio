#!/usr/bin/env python3
"""A stand-in for Chrome for Testing's list and Google's storage, on this computer: to try downloading Demido's
browser (DECISIONS Z1) without asking Google again for a file already here.

    python3 -I core/dev/browser-stand-in.py 8456 <chrome-headless-shell-linux64.zip> <version> &
    DEMIDO_DEV_BROWSER_LIST=http://127.0.0.1:8456/list.json ... demido serve

Only a development build listens to DEMIDO_DEV_BROWSER_LIST. The list has the shape of
last-known-good-versions-with-downloads.json; the file is sent with Content-Length and x-goog-hash (its MD5 in
base64), as Google's storage sends it, and goes on from a Range (docs/research/31-the-browser-a-model-drives.md).
"""
import base64, hashlib, json, os, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT, ZIP, VERSION = int(sys.argv[1]), sys.argv[2], sys.argv[3]
SIZE = os.path.getsize(ZIP)
with open(ZIP, 'rb') as f:
    MD5 = base64.b64encode(hashlib.md5(f.read()).digest()).decode()
PLATFORM = 'linux64'

class H(BaseHTTPRequestHandler):
    def log_message(self, fmt, *a):
        sys.stderr.write('browser-stand-in: ' + fmt % a + '\n')

    def list(self):
        url = f'http://127.0.0.1:{PORT}/{VERSION}/{PLATFORM}/chrome-headless-shell-{PLATFORM}.zip'
        return json.dumps({'timestamp': 'stand-in', 'channels': {'Stable': {'channel': 'Stable', 'version': VERSION, 'revision': '0',
                           'downloads': {'chrome-headless-shell': [{'platform': PLATFORM, 'url': url}]}}}}).encode()

    def head_of_file(self, start=0):
        self.send_header('content-type', 'application/zip')
        self.send_header('x-goog-hash', 'crc32c=AAAAAA==')
        self.send_header('x-goog-hash', f'md5={MD5}')
        self.send_header('content-length', str(SIZE - start))

    def do_HEAD(self):
        if self.path.endswith('.zip'):
            self.send_response(200); self.head_of_file(); self.end_headers()
        else:
            self.send_response(404); self.end_headers()

    def do_GET(self):
        if self.path == '/list.json':
            body = self.list()
            self.send_response(200); self.send_header('content-type', 'application/json'); self.send_header('content-length', str(len(body))); self.end_headers()
            self.wfile.write(body)
            return
        if not self.path.endswith('.zip'):
            self.send_response(404); self.send_header('content-length', '0'); self.end_headers(); return
        start = 0
        rng = self.headers.get('range', '')
        if rng.startswith('bytes='):
            start = int(rng[6:].split('-')[0] or 0)
            self.send_response(206); self.send_header('content-range', f'bytes {start}-{SIZE - 1}/{SIZE}')
        else:
            self.send_response(200)
        self.head_of_file(start); self.end_headers()
        with open(ZIP, 'rb') as f:
            f.seek(start)
            while chunk := f.read(1 << 20):
                self.wfile.write(chunk)

ThreadingHTTPServer(('127.0.0.1', PORT), H).serve_forever()
