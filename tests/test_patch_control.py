import json
import os
import tempfile
import unittest
from pathlib import Path

from grand_line.patch_control import (
    COMMAND_FILE_NAME,
    CONFIG_VERSION,
    DEFAULT_CONFIG,
    PatchControl,
    PatchControlError,
)


class FixedClock:
    def __init__(self, value=1_000_000):
        self.value = value

    def __call__(self):
        return self.value


class PatchControlTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.config_file = self.root / "config" / "mascot-control.json"
        self.runtime_dir = self.root / "runtime"
        self.runtime_dir.mkdir()
        self.clock = FixedClock()
        self.control = PatchControl(
            config_file=self.config_file,
            runtime_dir=self.runtime_dir,
            clock_ms=self.clock,
        )

    def tearDown(self):
        self.temporary.cleanup()

    def test_missing_or_invalid_config_uses_safe_defaults(self):
        self.assertEqual(self.control.load_config(), DEFAULT_CONFIG)
        self.config_file.parent.mkdir()
        self.config_file.write_text("{broken", encoding="utf-8")
        self.assertEqual(self.control.load_config(), DEFAULT_CONFIG)
        self.config_file.write_text(
            json.dumps({**DEFAULT_CONFIG, "surprise": True}),
            encoding="utf-8",
        )
        self.assertEqual(self.control.load_config(), DEFAULT_CONFIG)

    def test_settings_are_strict_atomic_and_mode_600(self):
        snapshot = self.control.update_settings(
            {
                "roamEnabled": False,
                "pattern": "horizontal",
                "pace": "quick",
                "microIdles": False,
                "activityReactions": True,
                "thermalReactions": False,
            }
        )
        self.assertEqual(
            snapshot["settings"],
            {
                "roamEnabled": False,
                "pattern": "horizontal",
                "pace": "quick",
                "microIdles": False,
                "activityReactions": True,
                "thermalReactions": False,
            },
        )
        disk = json.loads(self.config_file.read_text(encoding="utf-8"))
        self.assertEqual(disk["version"], CONFIG_VERSION)
        self.assertEqual(disk["roam_pattern"], "horizontal")
        self.assertEqual(self.config_file.stat().st_mode & 0o777, 0o600)
        self.assertEqual(list(self.config_file.parent.glob("*.tmp")), [])

    def test_invalid_settings_are_rejected_without_mutation(self):
        self.control.update_settings({"pace": "slow"})
        before = self.config_file.read_bytes()
        for payload in (
            {"pace": "warp"},
            {"pattern": "diagonal"},
            {"roamEnabled": 1},
            {"unknown": True},
            [],
        ):
            with self.subTest(payload=payload):
                with self.assertRaises(PatchControlError):
                    self.control.update_settings(payload)
                self.assertEqual(self.config_file.read_bytes(), before)

    def test_commands_have_exact_four_field_wire_format(self):
        auto = self.control.send_command({"command": "auto"})
        self.assertEqual(auto["mode"], "auto")
        command_file = self.runtime_dir / COMMAND_FILE_NAME
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000000 auto -\n",
        )
        self.assertEqual(command_file.stat().st_mode & 0o777, 0o600)

        home = self.control.send_command({"command": "home"})
        self.assertEqual(home["mode"], "manual")
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000001 home -\n",
        )

        self.control.send_command(
            {"command": "move", "location": "upper-left"}
        )
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000002 move upper-left\n",
        )
        self.control.send_command({"command": "preview", "event": "success"})
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000003 preview success\n",
        )
        self.assertEqual(self.control.load_config()["mode"], "manual")

    def test_command_payload_and_semantic_values_are_allowlisted(self):
        invalid = (
            {"command": "shell", "value": "rm"},
            {"command": "auto", "extra": True},
            {"command": "move"},
            {"command": "move", "location": "home"},
            {"command": "move", "location": "north"},
            {"command": "preview", "event": "error"},
            {"command": "preview", "event": "tool", "ttl": 999},
            "auto",
        )
        for payload in invalid:
            with self.subTest(payload=payload):
                with self.assertRaises(PatchControlError):
                    self.control.send_command(payload)
        self.assertFalse((self.runtime_dir / COMMAND_FILE_NAME).exists())

    def test_all_ui_locations_and_previews_are_accepted(self):
        for location in (
            "upper-left",
            "up",
            "upper-right",
            "left",
            "center",
            "right",
            "lower-left",
            "down",
            "lower-right",
        ):
            self.control.send_command(
                {"command": "move", "location": location}
            )
            self.assertTrue(
                (self.runtime_dir / COMMAND_FILE_NAME)
                .read_text(encoding="utf-8")
                .endswith(f" move {location}\n")
            )
        for event in ("tool", "read", "attention", "success"):
            self.control.send_command({"command": "preview", "event": event})
            self.assertTrue(
                (self.runtime_dir / COMMAND_FILE_NAME)
                .read_text(encoding="utf-8")
                .endswith(f" preview {event}\n")
            )

    def test_control_status_is_preferred_and_state_is_allowlisted(self):
        self.control.update_settings({"pace": "slow"})
        (self.runtime_dir / "lianli-agent-control-status").write_text(
            "v1 manual upper-right\n",
            encoding="ascii",
        )
        (self.runtime_dir / "lianli-agent-state").write_text(
            "event-tool\n",
            encoding="ascii",
        )
        snapshot = self.control.snapshot()
        self.assertEqual(snapshot["mode"], "manual")
        self.assertEqual(snapshot["location"], "upper-right")
        self.assertEqual(snapshot["state"], "event-tool")
        self.assertEqual(snapshot["service"], {"active": False, "status": "offline"})

        (self.runtime_dir / "lianli-agent-state").write_text(
            "not-a-selector\n",
            encoding="ascii",
        )
        self.assertEqual(self.control.snapshot()["state"], "unknown")

    def test_position_fallback_understands_home_and_logical_grid(self):
        position = self.runtime_dir / "lianli-agent-position"
        position.write_text("v1 1 1144 720 0\n", encoding="ascii")
        self.assertEqual(self.control.snapshot()["location"], "home")
        position.write_text("v1 2 1988 200 1200\n", encoding="ascii")
        self.assertEqual(self.control.snapshot()["location"], "upper-right")
        position.write_text("not a command\n", encoding="ascii")
        self.assertEqual(self.control.snapshot()["location"], "unknown")

    def test_snapshot_matches_dashboard_contract(self):
        snapshot = self.control.snapshot()
        self.assertEqual(
            set(snapshot),
            {"mode", "state", "location", "service", "settings"},
        )
        self.assertEqual(
            set(snapshot["settings"]),
            {
                "roamEnabled",
                "pattern",
                "pace",
                "microIdles",
                "activityReactions",
                "thermalReactions",
            },
        )
        self.assertEqual(set(snapshot["service"]), {"active", "status"})


if __name__ == "__main__":
    unittest.main()
