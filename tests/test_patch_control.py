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
                "characters": [],
                "characterSettings": {},
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

    def test_character_settings_round_trip_and_merge_per_crewmate(self):
        snapshot = self.control.update_settings(
            {
                "characterSettings": {
                    "navigator": {
                        "pace": "slow",
                        "roamEnabled": False,
                    }
                }
            }
        )
        self.assertEqual(
            snapshot["settings"]["characterSettings"],
            {"navigator": {"roamEnabled": False, "pace": "slow"}},
        )
        self.assertEqual(
            json.loads(self.config_file.read_text(encoding="utf-8"))[
                "character_settings"
            ],
            {"navigator": {"pace": "slow", "roam_enabled": False}},
        )

        snapshot = self.control.update_settings(
            {"characterSettings": {"navigator": {"pace": "quick"}}}
        )
        self.assertEqual(
            snapshot["settings"]["characterSettings"],
            {"navigator": {"roamEnabled": False, "pace": "quick"}},
        )

    def test_empty_character_settings_section_clears_that_crewmate(self):
        self.control.update_settings(
            {"characterSettings": {"navigator": {"microIdles": False}}}
        )
        snapshot = self.control.update_settings(
            {"characterSettings": {"navigator": {}}}
        )
        self.assertEqual(snapshot["settings"]["characterSettings"], {})
        self.assertEqual(self.control.load_config()["character_settings"], {})

    def test_invalid_character_settings_are_rejected_without_mutation(self):
        self.control.update_settings(
            {"characterSettings": {"navigator": {"pace": "slow"}}}
        )
        before = self.config_file.read_bytes()
        payloads = (
            {"characterSettings": {"patch": {"pace": "quick"}}},
            {"characterSettings": {"stowaway": {"pace": "quick"}}},
            {"characterSettings": {"navigator": {"unknown": True}}},
            {"characterSettings": {"navigator": {"pace": "warp"}}},
            {"characterSettings": {"navigator": {"roamEnabled": 1}}},
        )
        for payload in payloads:
            with self.subTest(payload=payload):
                with self.assertRaises(PatchControlError):
                    self.control.update_settings(payload)
                self.assertEqual(self.config_file.read_bytes(), before)

    def test_invalid_disk_character_settings_use_safe_defaults(self):
        self.config_file.parent.mkdir()
        invalid_sections = (
            [],
            {"patch": {"pace": "slow"}},
            {"stowaway": {"pace": "slow"}},
            {"navigator": []},
            {"navigator": {"unknown": True}},
            {"navigator": {"pace": "warp"}},
            {"navigator": {"thermal_reactions": 1}},
        )
        for section in invalid_sections:
            with self.subTest(section=section):
                config = {**DEFAULT_CONFIG, "character_settings": section}
                self.config_file.write_text(json.dumps(config), encoding="utf-8")
                self.assertEqual(self.control.load_config(), DEFAULT_CONFIG)

    def queued_commands(self):
        return self.control._queued_command_files()

    def test_commands_have_exact_four_field_wire_format(self):
        auto = self.control.send_command({"command": "auto"})
        self.assertEqual(auto["mode"], "auto")
        [command_file] = self.queued_commands()
        self.assertEqual(
            command_file.name,
            f"{COMMAND_FILE_NAME}.{1_000_000:020d}.{os.getpid()}",
        )
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000000 auto -\n",
        )
        self.assertEqual(command_file.stat().st_mode & 0o777, 0o600)
        command_file.unlink()

        home = self.control.send_command({"command": "home"})
        self.assertEqual(home["mode"], "manual")
        [command_file] = self.queued_commands()
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000001 home -\n",
        )
        command_file.unlink()

        self.control.send_command(
            {"command": "move", "location": "upper-left"}
        )
        [command_file] = self.queued_commands()
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000002 move upper-left\n",
        )
        command_file.unlink()
        self.control.send_command({"command": "preview", "event": "success"})
        [command_file] = self.queued_commands()
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000003 preview success\n",
        )
        self.assertEqual(self.control.load_config()["mode"], "manual")

    def test_rapid_commands_queue_in_order_without_loss(self):
        self.control.send_command({"command": "move", "location": "right"})
        self.control.send_command({"command": "preview", "event": "tool"})
        self.control.send_command({"command": "auto"})
        records = [
            path.read_text(encoding="utf-8")
            for path in self.queued_commands()
        ]
        self.assertEqual(
            records,
            [
                "v1 1000000 move right\n",
                "v1 1000001 preview tool\n",
                "v1 1000002 auto -\n",
            ],
        )

    def test_concurrent_bridge_instances_never_clobber_each_other(self):
        # A second instance sharing the same wall clock (and pid) races the
        # same millisecond; the exclusive publish must keep both records.
        other = PatchControl(
            config_file=self.config_file,
            runtime_dir=self.runtime_dir,
            clock_ms=self.clock,
        )
        self.control.send_command({"command": "move", "location": "left"})
        other.send_command({"command": "move", "location": "right"})
        records = sorted(
            path.read_text(encoding="utf-8") for path in self.queued_commands()
        )
        self.assertEqual(len(records), 2)
        self.assertIn("v1 1000000 move left\n", records)
        self.assertTrue(any(" move right\n" in record for record in records))

    def test_command_queue_is_bounded_with_a_clear_error(self):
        for _ in range(16):
            self.control.send_command({"command": "preview", "event": "tool"})
        with self.assertRaises(PatchControlError):
            self.control.send_command({"command": "auto"})
        self.assertEqual(len(self.queued_commands()), 16)

    def test_crewmate_commands_use_v2_records_and_require_enablement(self):
        # Not enabled yet: the bridge refuses rather than queueing a record
        # the watcher would drop.
        with self.assertRaises(PatchControlError):
            self.control.send_command(
                {"command": "move", "location": "lower-left", "character": "navigator"}
            )
        with self.assertRaises(PatchControlError):
            self.control.send_command(
                {"command": "move", "location": "left", "character": "stowaway"}
            )
        self.assertEqual(self.queued_commands(), [])

        snapshot = self.control.update_settings({"characters": ["navigator"]})
        self.assertEqual(snapshot["settings"]["characters"], ["navigator"])

        self.control.send_command(
            {"command": "move", "location": "lower-left", "character": "navigator"}
        )
        [command_file] = self.queued_commands()
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000000 navigator move lower-left\n",
        )
        command_file.unlink()
        # Crewmate commands never flip the stored (default character) mode.
        self.assertEqual(self.control.load_config()["mode"], "auto")

        # An explicit default-character field still writes v1 four-field
        # records so old watchers keep working during migration.
        self.control.send_command(
            {"command": "move", "location": "right", "character": "patch"}
        )
        [command_file] = self.queued_commands()
        self.assertEqual(
            command_file.read_text(encoding="utf-8"),
            "v1 1000001 move right\n",
        )
        command_file.unlink()
        self.assertEqual(self.control.load_config()["mode"], "manual")

    def test_snapshot_reports_enabled_crewmates(self):
        self.control.update_settings({"characters": ["navigator"]})
        (self.runtime_dir / "lianli-agent-state-navigator").write_text(
            "codex-active", encoding="ascii"
        )
        (self.runtime_dir / "lianli-agent-control-status-navigator").write_text(
            "v1 manual lower-left\n", encoding="ascii"
        )
        snapshot = self.control.snapshot()
        self.assertEqual(
            snapshot["characters"]["navigator"],
            {"mode": "manual", "state": "codex-active", "location": "lower-left"},
        )
        self.assertIn("patch", snapshot["characters"])
        with self.assertRaises(PatchControlError):
            self.control.update_settings({"characters": ["navigator", "navigator"]})
        with self.assertRaises(PatchControlError):
            self.control.update_settings({"characters": ["doctor"]})

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
        self.assertEqual(self.queued_commands(), [])

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
            latest = self.queued_commands()[-1]
            self.assertTrue(
                latest.read_text(encoding="utf-8").endswith(f" move {location}\n")
            )
            latest.unlink()
        for event in ("tool", "read", "attention", "success"):
            self.control.send_command({"command": "preview", "event": event})
            latest = self.queued_commands()[-1]
            self.assertTrue(
                latest.read_text(encoding="utf-8").endswith(f" preview {event}\n")
            )
            latest.unlink()

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
            {"mode", "state", "location", "service", "settings", "characters"},
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
                "characters",
                "characterSettings",
            },
        )
        self.assertEqual(set(snapshot["service"]), {"active", "status"})


if __name__ == "__main__":
    unittest.main()
