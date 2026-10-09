"""Local TLS GlobalProtect fixture accepting HIP and raw SSL tunnel setup."""
import socket
import ssl
import sys
import threading
from urllib.parse import parse_qs

cert, key, records = sys.argv[1:]
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(cert, key)
listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen()
state = {"configs": 0, "checks": 0, "reports": 0}
lock = threading.Lock()


def serve(connection):
    try:
        connection.settimeout(5)
        with context.wrap_socket(connection, server_side=True) as stream:
            buffer = b""
            while True:
                while b"\r\n\r\n" not in buffer:
                    data = stream.recv(4096)
                    if not data:
                        return
                    buffer += data
                headers, buffer = buffer.split(b"\r\n\r\n", 1)
                method, path, _ = headers.split(b"\r\n", 1)[0].decode().split()
                print(f"Gateway received {method} {path.split('?')[0]}", file=sys.stderr, flush=True)
                if method == "GET" and path.startswith("/ssl-tunnel-connect.sslvpn?"):
                    stream.sendall(b"START_TUNNEL")
                    while stream.recv(4096):
                        pass
                    return
                length = 0
                for header in headers.split(b"\r\n")[1:]:
                    name, value = header.split(b":", 1)
                    if name.lower() == b"content-length":
                        length = int(value)
                while len(buffer) < length:
                    data = stream.recv(4096)
                    if not data:
                        raise EOFError(f"Incomplete body for {path}: {len(buffer)}/{length}")
                    buffer += data
                body, buffer = buffer[:length], buffer[length:]
                form = parse_qs(body.decode())
                status = "200 OK"
                with lock:
                    if path == "/ssl-vpn/getconfig.esp":
                        state["configs"] += 1
                        ip = "10.0.0.1"
                        response = f"<response><ip-address>{ip}</ip-address><gw-address>127.0.0.1</gw-address><ssl-tunnel-url>/ssl-tunnel-connect.sslvpn</ssl-tunnel-url></response>"
                    elif path == "/ssl-vpn/hipreportcheck.esp":
                        state["checks"] += 1
                        needed = "no" if state["checks"] == 4 else "yes"
                        response = f"<response><hip-report-needed>{needed}</hip-report-needed></response>"
                    elif path == "/ssl-vpn/hipreport.esp":
                        state["reports"] += 1
                        seq = state["reports"]
                        expected_ip = "10.0.0.1"
                        assert form.get("authcookie") == ["test" if seq < 3 else "refreshed"]
                        expected = f'<hip seq="{seq}" ip="{expected_ip}"/>'
                        assert form.get("report") == [expected], form
                        assert form.get("client-ip") == [expected_ip], form
                        with open(records, "a", encoding="utf-8") as output:
                            output.write(expected + "\n")
                        if seq == 4:
                            status = "403 Forbidden"
                        response = "<response/>"
                    else:
                        raise AssertionError(path)
                payload = response.encode()
                stream.sendall(f"HTTP/1.1 {status}\r\nContent-Length: {len(payload)}\r\nContent-Type: text/xml\r\nConnection: keep-alive\r\n\r\n".encode() + payload)
                print(f"Gateway responded {status} to {path.split('?')[0]}", file=sys.stderr, flush=True)
    except (ConnectionError, ssl.SSLError):
        pass


print(listener.getsockname()[1], flush=True)
while True:
    connection, _ = listener.accept()
    threading.Thread(target=serve, args=(connection,), daemon=True).start()
