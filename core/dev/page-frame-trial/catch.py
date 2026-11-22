"""Listens where a running page might try to send something, and writes down everything that arrives:
HTTP on 7390, UDP (STUN, for WebRTC) on 7391, on this computer only."""
import http.server, socket, threading, time, sys

LOG = sys.argv[1]

def note(what):
    with open(LOG, 'a') as f:
        f.write(f'{time.strftime("%H:%M:%S")} {what}\n')

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        note(f'HTTP {self.command} {self.path} from {self.client_address[0]}')
        self.send_response(200); self.send_header('content-type', 'text/html'); self.end_headers()
        self.wfile.write(b'<p>caught</p>')
    do_POST = do_GET
    def log_message(self, *a): pass

def udp():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(('127.0.0.1', 7391))
    while True:
        data, addr = s.recvfrom(4096)
        note(f'UDP {len(data)} bytes from {addr} {data[:60]!r}')

threading.Thread(target=udp, daemon=True).start()
note('listening')
http.server.ThreadingHTTPServer(('127.0.0.1', 7390), H).serve_forever()
