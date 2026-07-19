"""Strict local control bridge for the Patch LCD mascot.

The web server never writes the renderer's state or position files. It stores
validated preferences and publishes a latest-command record for the watcher,
which remains the sole owner of visible mascot state.
"""

from __future__ import annotations

import json
import os
import stat
import threading
import time
from pathlib import Path


CONFIG_VERSION = 1
CONFIG_FILE = Path.home() / ".config" / "lianli" / "mascot-control.json"
COMMAND_FILE_NAME = "lianli-agent-command"
# Commands are published as one queue file per record so rapid taps cannot
# overwrite each other before the watcher's next 100 ms tick. The suffix
# contract shared with the watcher is `.<20-digit created-ms>.<pid>`; the
# padded timestamp makes lexicographic order equal delivery order.
MAX_QUEUED_COMMANDS = 16
CONTROL_STATUS_FILE_NAME = "lianli-agent-control-status"
STATE_FILE_NAME = "lianli-agent-state"
POSITION_FILE_NAME = "lianli-agent-position"
WATCHER_LOCK_FILE_NAME = ".lianli-agent-watch.lock"

LOCATIONS = (
    "home",
    "upper-left",
    "up",
    "upper-right",
    "left",
    "center",
    "right",
    "lower-left",
    "down",
    "lower-right",
)
MOVE_LOCATIONS = tuple(location for location in LOCATIONS if location != "home")
PREVIEW_EVENTS = ("tool", "read", "attention", "success")
PATTERNS = ("full", "horizontal", "vertical")
PACES = ("slow", "normal", "quick")

# The default character owns the un-suffixed v1 runtime files; optional
# crewmates are enabled by name in the `characters` config list and are
# addressed with character-suffixed files and v2 five-field commands.
DEFAULT_CHARACTER = "patch"
OPTIONAL_CHARACTERS = ("navigator",)

DEFAULT_CONFIG = {
    "version": CONFIG_VERSION,
    "mode": "auto",
    "roam_enabled": True,
    "roam_pattern": "full",
    "pace": "normal",
    "micro_idles": True,
    "activity_reactions": True,
    "thermal_reactions": True,
    "characters": [],
    "character_settings": {},
}

CHARACTER_PUBLIC_TO_DISK = {
    "roamEnabled": "roam_enabled",
    "pattern": "roam_pattern",
    "pace": "pace",
    "microIdles": "micro_idles",
    "activityReactions": "activity_reactions",
    "thermalReactions": "thermal_reactions",
}
PUBLIC_TO_DISK = {
    **CHARACTER_PUBLIC_TO_DISK,
    "characters": "characters",
    "characterSettings": "character_settings",
}

KNOWN_STATES = {
    "idle",
    "codex-enter",
    "codex-active",
    "codex-wait",
    "codex-exit",
    "claude-enter",
    "claude-active",
    "claude-wait",
    "claude-exit",
    "both-enter",
    "both-active",
    "both-wait",
    "both-exit",
    "event-error",
    "event-attention",
    "event-success",
    "event-done",
    "event-tool",
    "event-reading",
    "thermal-warm",
    "thermal-hot",
    "thermal-critical",
    "thermal-relief",
    "idle-blink",
    "idle-hat-adjust",
    "idle-stretch",
    "idle-mug",
    "idle-gauge",
    "idle-breeze",
    "stroll-left",
    "stroll-right",
}

# Logical 2288x1048 destinations. These are used only to infer a friendly
# fallback location when the watcher has not published its control-status file.
LOGICAL_LOCATIONS = {
    "upper-left": (300, 200),
    "up": (1144, 200),
    "upper-right": (1988, 200),
    "left": (300, 524),
    "center": (1144, 524),
    "right": (1988, 524),
    "lower-left": (300, 848),
    "down": (1144, 848),
    "lower-right": (1988, 848),
}
HOME_POINT = (1144, 720)


class PatchControlError(ValueError):
    """A safe client-facing validation error."""


def _validate_disk_setting(key: str, value: object) -> None:
    if key in {
        "roam_enabled",
        "micro_idles",
        "activity_reactions",
        "thermal_reactions",
    }:
        if type(value) is not bool:
            raise PatchControlError(f"{key} must be a boolean")
    elif key == "roam_pattern":
        if value not in PATTERNS:
            raise PatchControlError(
                "roam_pattern must be full, horizontal, or vertical"
            )
    elif key == "pace" and value not in PACES:
        raise PatchControlError("pace must be slow, normal, or quick")


def _validate_character_settings(value: object) -> dict:
    if not isinstance(value, dict):
        raise PatchControlError("character_settings must be an object")
    clean = {}
    allowed = set(CHARACTER_PUBLIC_TO_DISK.values())
    for character, settings in value.items():
        if character not in OPTIONAL_CHARACTERS:
            raise PatchControlError("Unknown mascot character in character_settings")
        if not isinstance(settings, dict):
            raise PatchControlError("Each character_settings value must be an object")
        unknown = set(settings) - allowed
        if unknown:
            raise PatchControlError(
                f"Unknown character setting: {next(iter(unknown))}"
            )
        for key, setting in settings.items():
            _validate_disk_setting(key, setting)
        clean[character] = dict(settings)
    return clean


def _character_settings_to_disk(value: object) -> dict:
    if not isinstance(value, dict):
        raise PatchControlError("characterSettings must be an object")
    disk_sections = {}
    for character, settings in value.items():
        if character not in OPTIONAL_CHARACTERS:
            raise PatchControlError("Unknown mascot character in characterSettings")
        if not isinstance(settings, dict):
            raise PatchControlError("Each characterSettings value must be an object")
        unknown = set(settings) - set(CHARACTER_PUBLIC_TO_DISK)
        if unknown:
            raise PatchControlError(
                f"Unknown character setting: {next(iter(unknown))}"
            )
        disk_sections[character] = {
            CHARACTER_PUBLIC_TO_DISK[public]: setting
            for public, setting in settings.items()
        }
    return _validate_character_settings(disk_sections)


def _trusted_runtime_dir() -> Path:
    uid = os.geteuid()
    candidates = []
    configured = os.environ.get("XDG_RUNTIME_DIR")
    if configured:
        candidates.append(Path(configured))
    candidates.append(Path(f"/run/user/{uid}"))
    for path in candidates:
        try:
            stat = path.stat()
        except OSError:
            continue
        if path.is_absolute() and path.is_dir() and stat.st_uid == uid:
            return path
    raise PatchControlError("No trusted user runtime directory is available")


def _atomic_write(path: Path, value: str, exclusive: bool = False) -> None:
    """Atomically publish ``value`` at ``path``.

    With ``exclusive=True`` the publish fails with :class:`FileExistsError`
    instead of replacing an existing file, so concurrent queue writers can
    never silently clobber each other's records.
    """
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    for attempt in range(16):
        temporary = path.with_name(
            f".{path.name}.{os.getpid()}.{threading.get_ident()}.{attempt}.tmp"
        )
        try:
            descriptor = os.open(
                temporary,
                os.O_WRONLY | os.O_CREAT | os.O_EXCL,
                0o600,
            )
        except FileExistsError:
            continue
        try:
            with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
                stream.write(value)
                stream.flush()
                os.fsync(stream.fileno())
            os.chmod(temporary, 0o600)
            if exclusive:
                os.link(temporary, path)
            else:
                os.replace(temporary, path)
            return
        finally:
            try:
                temporary.unlink()
            except OSError:
                pass
    raise OSError("Could not reserve a temporary Patch control file")


def _validate_config(value: object) -> dict:
    if not isinstance(value, dict) or set(value) != set(DEFAULT_CONFIG):
        raise PatchControlError("Invalid mascot configuration shape")
    if value.get("version") != CONFIG_VERSION:
        raise PatchControlError("Unsupported mascot configuration version")
    if value.get("mode") not in {"auto", "manual"}:
        raise PatchControlError("mode must be auto or manual")
    for key in (
        "roam_pattern",
        "pace",
        "roam_enabled",
        "micro_idles",
        "activity_reactions",
        "thermal_reactions",
    ):
        _validate_disk_setting(key, value.get(key))
    characters = value.get("characters")
    if (
        not isinstance(characters, list)
        or len(set(characters)) != len(characters)
        or any(character not in OPTIONAL_CHARACTERS for character in characters)
    ):
        raise PatchControlError("characters must be a list of known crewmates")
    clean = dict(value)
    clean["character_settings"] = _validate_character_settings(
        value.get("character_settings")
    )
    return clean


def _public_settings(config: dict) -> dict:
    settings = {
        public: config[disk]
        for public, disk in PUBLIC_TO_DISK.items()
        if public != "characterSettings"
    }
    settings["characterSettings"] = {
        character: {
            public: overrides[disk]
            for public, disk in CHARACTER_PUBLIC_TO_DISK.items()
            if disk in overrides
        }
        for character, overrides in config["character_settings"].items()
    }
    return settings


class PatchControl:
    def __init__(
        self,
        config_file: Path | None = None,
        runtime_dir: Path | None = None,
        clock_ms=None,
    ):
        self.config_file = Path(config_file or CONFIG_FILE)
        self.runtime_dir = Path(runtime_dir) if runtime_dir else _trusted_runtime_dir()
        self.clock_ms = clock_ms or (lambda: time.time_ns() // 1_000_000)
        self._lock = threading.Lock()
        self._last_command_ms = 0

    def _queued_command_files(self) -> list[Path]:
        pending = []
        prefix = COMMAND_FILE_NAME + "."
        for path in self.runtime_dir.iterdir():
            suffix = path.name.removeprefix(prefix)
            if suffix == path.name:
                continue
            stamp, _, pid = suffix.partition(".")
            if (
                len(stamp) == 20
                and stamp.isascii()
                and stamp.isdigit()
                and 1 <= len(pid) <= 10
                and pid.isascii()
                and pid.isdigit()
            ):
                pending.append(path)
        return sorted(pending)

    def _command_queue_entry(self, created_ms: int) -> Path:
        return self.runtime_dir / (
            f"{COMMAND_FILE_NAME}.{created_ms:020d}.{os.getpid()}"
        )

    def load_config(self) -> dict:
        try:
            raw = json.loads(self.config_file.read_text(encoding="utf-8"))
            return _validate_config(raw)
        except (OSError, json.JSONDecodeError, PatchControlError):
            return dict(DEFAULT_CONFIG)

    def _save_config(self, config: dict) -> None:
        clean = _validate_config(config)
        _atomic_write(
            self.config_file,
            json.dumps(clean, indent=2, sort_keys=True) + "\n",
        )

    def update_settings(self, changes: object) -> dict:
        if not isinstance(changes, dict):
            raise PatchControlError("Settings payload must be an object")
        unknown = set(changes) - set(PUBLIC_TO_DISK)
        if unknown:
            raise PatchControlError(f"Unknown Patch setting: {sorted(unknown)[0]}")
        with self._lock:
            config = self.load_config()
            for public, value in changes.items():
                disk = PUBLIC_TO_DISK[public]
                if public in CHARACTER_PUBLIC_TO_DISK:
                    _validate_disk_setting(disk, value)
                elif public == "characters" and (
                    not isinstance(value, list)
                    or len(set(value)) != len(value)
                    or any(
                        character not in OPTIONAL_CHARACTERS
                        for character in value
                    )
                ):
                    raise PatchControlError(
                        "characters must be a list of known crewmates"
                    )
                elif public == "characterSettings":
                    merged = {
                        character: dict(overrides)
                        for character, overrides in config[disk].items()
                    }
                    disk_sections = _character_settings_to_disk(value)
                    for character, overrides in disk_sections.items():
                        if overrides:
                            merged.setdefault(character, {}).update(overrides)
                        else:
                            merged.pop(character, None)
                    value = merged
                config[disk] = value
            self._save_config(config)
        return self.snapshot()

    def _next_command_ms(self) -> int:
        now = int(self.clock_ms())
        self._last_command_ms = max(now, self._last_command_ms + 1)
        return self._last_command_ms

    def send_command(self, payload: object) -> dict:
        if not isinstance(payload, dict):
            raise PatchControlError("Command payload must be an object")
        command = payload.get("command")
        if command not in {"auto", "home", "move", "preview"}:
            raise PatchControlError("command must be auto, home, move, or preview")
        expected = {
            "auto": {"command"},
            "home": {"command"},
            "move": {"command", "location"},
            "preview": {"command", "event"},
        }[command]
        if not (expected <= set(payload) <= expected | {"character"}):
            raise PatchControlError("Command payload has missing or unknown fields")
        character = payload.get("character", DEFAULT_CHARACTER)
        if character != DEFAULT_CHARACTER and character not in OPTIONAL_CHARACTERS:
            raise PatchControlError("Unknown mascot character")

        argument = "-"
        if command == "move":
            argument = payload.get("location")
            if argument not in MOVE_LOCATIONS:
                raise PatchControlError("Unknown physical Patch location")
        elif command == "preview":
            argument = payload.get("event")
            if argument not in PREVIEW_EVENTS:
                raise PatchControlError("Preview must be tool, read, attention, or success")

        with self._lock:
            if len(self._queued_command_files()) >= MAX_QUEUED_COMMANDS:
                raise PatchControlError(
                    "Too many queued mascot commands; try again shortly"
                )
            config = self.load_config()
            if character != DEFAULT_CHARACTER and character not in config["characters"]:
                raise PatchControlError("That crewmate is not enabled")
            # The stored mode only tracks the default character; crewmate
            # manual state lives in the watcher and its control-status file.
            if character == DEFAULT_CHARACTER:
                if command == "auto":
                    config["mode"] = "auto"
                    self._save_config(config)
                elif command in {"home", "move"}:
                    config["mode"] = "manual"
                    self._save_config(config)
            # v1 four-field records address the default character; other
            # crewmates use the v2 record with the character field inserted.
            if character == DEFAULT_CHARACTER:
                def record(created_ms: int) -> str:
                    return f"v1 {created_ms} {command} {argument}\n"
            else:
                def record(created_ms: int) -> str:
                    return f"v1 {created_ms} {character} {command} {argument}\n"
            # Timestamps are only unique within this instance; another
            # bridge instance or process can race the same millisecond, so
            # publish without clobbering and re-stamp on collision.
            for _ in range(8):
                created_ms = self._next_command_ms()
                try:
                    _atomic_write(
                        self._command_queue_entry(created_ms),
                        record(created_ms),
                        exclusive=True,
                    )
                    break
                except FileExistsError:
                    continue
            else:
                raise PatchControlError(
                    "Could not reserve a mascot command slot; try again"
                )
        return self.snapshot()

    def _control_status(self, character: str | None = None) -> tuple[str, str] | None:
        name = CONTROL_STATUS_FILE_NAME
        if character is not None:
            name = f"{name}-{character}"
        path = self.runtime_dir / name
        try:
            metadata = os.lstat(path)
            if (
                not stat.S_ISREG(metadata.st_mode)
                or metadata.st_uid != os.geteuid()
                or metadata.st_size > 128
            ):
                return None
            fields = path.read_text(encoding="ascii").split()
        except (OSError, UnicodeError):
            return None
        if (
            len(fields) == 3
            and fields[0] == "v1"
            and fields[1] in {"auto", "manual"}
            and fields[2] in LOCATIONS
        ):
            return fields[1], fields[2]
        return None

    def _state(self, character: str | None = None) -> str:
        name = STATE_FILE_NAME
        if character is not None:
            name = f"{name}-{character}"
        try:
            state = (self.runtime_dir / name).read_text(
                encoding="ascii"
            ).strip()
        except (OSError, UnicodeError):
            return "unknown"
        return state if state in KNOWN_STATES else "unknown"

    def _fallback_location(self) -> str:
        try:
            fields = (self.runtime_dir / POSITION_FILE_NAME).read_text(
                encoding="ascii"
            ).split()
            if len(fields) != 5 or fields[0] != "v1":
                return "unknown"
            point = (int(round(float(fields[2]))), int(round(float(fields[3]))))
        except (OSError, UnicodeError, ValueError):
            return "unknown"
        if (point[0] - HOME_POINT[0]) ** 2 + (point[1] - HOME_POINT[1]) ** 2 <= 100**2:
            return "home"
        return min(
            LOGICAL_LOCATIONS,
            key=lambda location: (
                point[0] - LOGICAL_LOCATIONS[location][0]
            ) ** 2
            + (point[1] - LOGICAL_LOCATIONS[location][1]) ** 2,
        )

    def _watcher_service(self) -> dict:
        lock_path = self.runtime_dir / WATCHER_LOCK_FILE_NAME
        try:
            pid = int(lock_path.read_text(encoding="ascii").strip())
            comm = Path(f"/proc/{pid}/comm").read_text(encoding="ascii").strip()
            active = comm.startswith("lianli-agent-w")
        except (OSError, UnicodeError, ValueError):
            active = False
        return {
            "active": active,
            "status": "running" if active else "offline",
        }

    def snapshot(self) -> dict:
        config = self.load_config()
        status = self._control_status()
        mode, location = status or (config["mode"], self._fallback_location())
        state = self._state()
        # The single-character shape stays intact for existing dashboards;
        # per-character detail rides alongside it.
        characters = {
            DEFAULT_CHARACTER: {
                "mode": mode,
                "state": state,
                "location": location,
            }
        }
        for character in config["characters"]:
            crew_status = self._control_status(character)
            crew_mode, crew_location = crew_status or ("auto", "unknown")
            characters[character] = {
                "mode": crew_mode,
                "state": self._state(character),
                "location": crew_location,
            }
        return {
            "mode": mode,
            "state": state,
            "location": location,
            "service": self._watcher_service(),
            "settings": _public_settings(config),
            "characters": characters,
        }
