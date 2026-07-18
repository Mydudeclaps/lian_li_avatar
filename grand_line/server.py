"""Small, loopback-only HTTP bridge for Patch's control page."""

from __future__ import annotations

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

from .patch_control import PatchControl, PatchControlError


MAX_REQUEST_BYTES = 16 * 1024
STATIC_ROOT = Path(__file__).with_name("static")
STATIC_ROUTES = {
    "/": ("index.html", "text/html; charset=utf-8"),
    "/index.html": ("index.html", "text/html; charset=utf-8"),
    "/patch.css": ("patch.css", "text/css; charset=utf-8"),
    "/patch.js": ("patch.js", "text/javascript; charset=utf-8"),
}


class RequestError(ValueError):
    def __init__(self, status: int, message: str):
        super().__init__(message)
        self.status = status


class PatchHandler(BaseHTTPRequestHandler):
    """Exact-route handler: it never translates a URL into a filesystem path."""

    control: PatchControl
    server_version = "PatchControl/1"
    sys_version = ""

    def log_message(self, format, *args):  # noqa: A003 - stdlib API name
        return

    def end_headers(self):
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Cross-Origin-Resource-Policy", "same-origin")
        self.send_header(
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; "
            "img-src 'self'; connect-src 'self'; frame-ancestors 'self'",
        )
        super().end_headers()

    def _send_bytes(self, body: bytes, content_type: str, status: int = 200):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def send_json(self, payload: object, status: int = 200):
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self._send_bytes(body, "application/json; charset=utf-8", status)

    def send_error_json(self, status: int, message: str):
        self.send_json({"ok": False, "error": message}, status)

    def _same_origin(self) -> bool:
        origin = self.headers.get("Origin")
        if not origin:
            return True
        parsed = urlsplit(origin)
        return (
            parsed.scheme == "http"
            and parsed.netloc == self.headers.get("Host", "")
            and parsed.hostname in {"127.0.0.1", "localhost", "::1"}
        )

    def _read_json(self) -> object:
        if self.headers.get("Transfer-Encoding"):
            raise RequestError(400, "Transfer-Encoding is not supported")
        raw_length = self.headers.get("Content-Length")
        try:
            length = int(raw_length) if raw_length is not None else -1
        except ValueError as exc:
            raise RequestError(400, "Invalid Content-Length") from exc
        if length < 0:
            raise RequestError(411, "Content-Length is required")
        if length > MAX_REQUEST_BYTES:
            raise RequestError(413, "Request body is too large")
        content_type = self.headers.get("Content-Type", "").split(";", 1)[0].strip()
        if content_type != "application/json":
            raise RequestError(415, "Content-Type must be application/json")
        try:
            return json.loads(self.rfile.read(length).decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise RequestError(400, "Request body must be valid JSON") from exc

    def do_GET(self):  # noqa: N802 - stdlib API name
        path = urlsplit(self.path).path
        if path == "/api/patch":
            return self.send_json(self.control.snapshot())
        route = STATIC_ROUTES.get(path)
        if route is None:
            return self.send_error_json(404, "Not found")
        filename, content_type = route
        try:
            body = (STATIC_ROOT / filename).read_bytes()
        except OSError:
            return self.send_error_json(500, "Control-panel asset is unavailable")
        return self._send_bytes(body, content_type)

    def do_POST(self):  # noqa: N802 - stdlib API name
        path = urlsplit(self.path).path
        if path not in {"/api/patch/settings", "/api/patch/command"}:
            return self.send_error_json(404, "Not found")
        if not self._same_origin():
            return self.send_error_json(403, "Cross-origin requests are not allowed")
        try:
            payload = self._read_json()
            if path == "/api/patch/settings":
                result = self.control.update_settings(payload)
            else:
                result = self.control.send_command(payload)
        except RequestError as exc:
            return self.send_error_json(exc.status, str(exc))
        except PatchControlError as exc:
            return self.send_error_json(400, str(exc))
        except OSError:
            return self.send_error_json(500, "Patch control state could not be saved")
        return self.send_json(result)


def make_server(
    address: tuple[str, int] = ("127.0.0.1", 7071),
    control: PatchControl | None = None,
) -> ThreadingHTTPServer:
    if address[0] not in {"127.0.0.1", "localhost"}:
        raise ValueError("Patch control must bind to loopback")
    configured_control = control or PatchControl()
    handler = type(
        "ConfiguredPatchHandler",
        (PatchHandler,),
        {"control": configured_control},
    )
    return ThreadingHTTPServer(address, handler)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=7071)
    args = parser.parse_args()
    server = make_server(("127.0.0.1", args.port))
    print(f"Patch control: http://127.0.0.1:{server.server_port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
