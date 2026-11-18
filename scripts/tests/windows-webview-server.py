#!/usr/bin/env python3
"""Loopback-only SAML/WebView2 fixture. All returned credentials are synthetic."""
import argparse
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ssl
import threading

BODY = '<html><saml-auth-status>1</saml-auth-status><saml-username>webview-fixture</saml-username><prelogin-cookie>synthetic-webview-cookie</prelogin-cookie></html>'


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', '0')))
        request = base64.b64encode(f'http://127.0.0.1:{args.http_port}/body'.encode()).decode()
        self.respond(f'<prelogin-response><status>Success</status><saml-auth-method>POST</saml-auth-method><saml-request>{request}</saml-request><saml-default-browser>no</saml-default-browser></prelogin-response>')

    def respond(self, body, headers=(), status=200):
        data = body.encode()
        self.send_response(status)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(data)))
        for name, value in headers:
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = self.path.split('?')[0]
        if path == '/redirect':
            self.respond('', [('Location', '/body')], 302)
        elif path == '/headers':
            self.respond('<html>Header response</html>', [('saml-auth-status', '1'), ('saml-username', 'webview-fixture'), ('prelogin-cookie', 'synthetic-header-cookie')])
        elif path == '/callback':
            self.respond('<html><script>location.href="globalprotectcallback:cas-as=1&un=webview-fixture&token=synthetic-webview-token"</script></html>')
        elif path == '/cookie-set':
            self.respond(BODY, [('Set-Cookie', 'webview_fixture=present; Max-Age=3600; Path=/; HttpOnly')])
        elif path in ['/cookie-present', '/cookie-clean']:
            present = 'webview_fixture=present' in self.headers.get('Cookie', '')
            success = present if path == '/cookie-present' else not present
            self.respond(BODY if success else '<saml-auth-status>0</saml-auth-status>')
        elif path == '/user-agent':
            agent = self.headers.get('User-Agent', '')
            self.respond(BODY if agent.startswith('PAN GlobalProtect/') and 'Mozilla/' in agent else '<saml-auth-status>0</saml-auth-status>')
        elif path == '/temporary':
            self.respond('<html>Temporarily Unavailable</html>')
        elif path == '/signin':
            self.respond('<html><title>Fixture sign-in</title><h1>Sign in</h1><a href="/body">Complete fixture sign-in</a></html>')
        elif path == '/rejected':
            self.respond('<saml-auth-status>0</saml-auth-status>')
        elif path == '/cross-host':
            self.respond('', [('Location', f'https://127.0.0.1:{args.https_port}/body')], 302)
        elif path == '/body':
            self.respond(BODY)
        else:
            self.send_error(404)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--http-port', type=int, default=18769)
    parser.add_argument('--https-port', type=int, default=18770)
    parser.add_argument('--cert', required=True)
    parser.add_argument('--key', required=True)
    args = parser.parse_args()
    http = ThreadingHTTPServer(('127.0.0.1', args.http_port), Handler)
    https = ThreadingHTTPServer(('127.0.0.1', args.https_port), Handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(args.cert, args.key)
    https.socket = context.wrap_socket(https.socket, server_side=True)
    threading.Thread(target=https.serve_forever, daemon=True).start()
    print('Webview fixture ready', flush=True)
    http.serve_forever()
