import http.client
import json
import tempfile
import threading
import unittest
from pathlib import Path

from grand_line.patch_control import PatchControl
from grand_line.server import MAX_REQUEST_BYTES, make_server


class PatchServerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        root = Path(self.temporary.name)
        control = PatchControl(
            config_file=root / "mascot-control.json",
            runtime_dir=root,
            clock_ms=lambda: 2_000_000,
        )
        self.server = make_server(("127.0.0.1", 0), control)
        self.thread = threading.Thread(
            target=self.server.serve_forever,
            daemon=True,
        )
        self.thread.start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)
        self.temporary.cleanup()

    def request(self, method, path, payload=None, headers=None):
        connection = http.client.HTTPConnection(
            "127.0.0.1",
            self.server.server_port,
            timeout=2,
        )
        body = None if payload is None else json.dumps(payload).encode()
        request_headers = dict(headers or {})
        if body is not None:
            request_headers.setdefault("Content-Type", "application/json")
            request_headers.setdefault("Content-Length", str(len(body)))
        connection.request(method, path, body=body, headers=request_headers)
        response = connection.getresponse()
        raw = response.read()
        result = (
            json.loads(raw)
            if response.getheader("Content-Type", "").startswith("application/json")
            else raw
        )
        status = response.status
        response_headers = dict(response.getheaders())
        connection.close()
        return status, response_headers, result

    def test_status_settings_and_commands_match_the_dashboard_contract(self):
        status, _, snapshot = self.request("GET", "/api/patch")
        self.assertEqual(status, 200)
        self.assertEqual(snapshot["mode"], "auto")

        status, _, snapshot = self.request(
            "POST",
            "/api/patch/settings",
            {"pattern": "horizontal", "pace": "quick"},
        )
        self.assertEqual(status, 200)
        self.assertEqual(snapshot["settings"]["pattern"], "horizontal")
        self.assertEqual(snapshot["settings"]["pace"], "quick")

        status, _, snapshot = self.request(
            "POST",
            "/api/patch/command",
            {"command": "move", "location": "right"},
        )
        self.assertEqual(status, 200)
        self.assertEqual(snapshot["mode"], "manual")

    def test_static_routes_are_allowlisted_and_have_security_headers(self):
        status, headers, body = self.request("GET", "/")
        self.assertEqual(status, 200)
        self.assertIn(b"PATCH COMMAND", body)
        self.assertEqual(headers["X-Content-Type-Options"], "nosniff")
        self.assertIn("default-src 'self'", headers["Content-Security-Policy"])

        for path in (
            "/../../.bashrc",
            "/%2e%2e/%2e%2e/.bashrc",
            "/..%2f..%2f.bashrc",
            "/missing",
        ):
            with self.subTest(path=path):
                status, _, body = self.request("GET", path)
                self.assertEqual(status, 404)
                self.assertFalse(body["ok"])

    def test_invalid_cross_origin_and_oversized_posts_are_rejected(self):
        status, _, body = self.request(
            "POST",
            "/api/patch/command",
            {"command": "auto"},
            {"Origin": "https://example.invalid"},
        )
        self.assertEqual(status, 403)
        self.assertFalse(body["ok"])

        connection = http.client.HTTPConnection(
            "127.0.0.1",
            self.server.server_port,
            timeout=2,
        )
        connection.request(
            "POST",
            "/api/patch/settings",
            body=b"x" * (MAX_REQUEST_BYTES + 1),
            headers={
                "Content-Type": "application/json",
                "Content-Length": str(MAX_REQUEST_BYTES + 1),
            },
        )
        response = connection.getresponse()
        self.assertEqual(response.status, 413)
        self.assertFalse(json.loads(response.read())["ok"])
        connection.close()

    def test_invalid_semantic_command_returns_json_400(self):
        status, _, body = self.request(
            "POST",
            "/api/patch/command",
            {"command": "move", "location": "../../tmp"},
        )
        self.assertEqual(status, 400)
        self.assertFalse(body["ok"])
        self.assertIn("error", body)


if __name__ == "__main__":
    unittest.main()
