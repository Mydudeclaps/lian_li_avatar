use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

const MIN_TTL_MS: u64 = 250;
const MAX_TTL_MS: u64 = 60_000;
const MAX_RECORD_BYTES: usize = 512;
/// Upper bound on undelivered records per actor. If the watcher is not
/// running, the oldest records are dropped so the runtime directory stays
/// bounded and hooks keep succeeding.
const MAX_QUEUED_RECORDS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Actor {
    Codex,
    Claude,
}

impl Actor {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            _ => Err(format!("invalid actor '{value}'; expected codex or claude")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EventKind {
    Start,
    Exit,
    Tool,
    Read,
    Permission,
    Error,
    Success,
}

impl EventKind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "start" => Ok(Self::Start),
            "exit" => Ok(Self::Exit),
            "tool" => Ok(Self::Tool),
            "read" => Ok(Self::Read),
            "permission" => Ok(Self::Permission),
            "error" => Ok(Self::Error),
            "success" => Ok(Self::Success),
            _ => Err(format!(
                "invalid kind '{value}'; expected start, exit, tool, read, permission, error, or success"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Exit => "exit",
            Self::Tool => "tool",
            Self::Read => "read",
            Self::Permission => "permission",
            Self::Error => "error",
            Self::Success => "success",
        }
    }

    fn default_ttl_ms(self) -> u64 {
        match self {
            Self::Start => 5_000,
            Self::Exit => 6_000,
            Self::Tool => 1_500,
            Self::Read => 1_500,
            Self::Permission => 10_000,
            Self::Error => 12_000,
            Self::Success => 8_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cli {
    actor: Actor,
    kind: EventKind,
    ttl_ms: u64,
}

fn usage() -> &'static str {
    "Usage: lianli-agent-event <actor> <event> [--ttl-ms N]\n\
     Actors: codex, claude\n\
     Events: start, exit, tool, read, permission, error, success"
}

fn parse_bounded_u64(flag: &str, value: Option<String>, min: u64, max: u64) -> Result<u64, String> {
    let raw = value.ok_or_else(|| format!("{flag} requires a value"))?;
    let parsed = raw
        .parse::<u64>()
        .map_err(|_| format!("{flag} must be an integer"))?;
    if !(min..=max).contains(&parsed) {
        return Err(format!("{flag} must be in [{min}, {max}]"));
    }
    Ok(parsed)
}

fn parse_args<I>(args: I) -> Result<Cli, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let actor_raw = args.next().ok_or_else(|| usage().to_string())?;
    if actor_raw == "-h" || actor_raw == "--help" {
        return Err(usage().to_string());
    }
    let kind_raw = args.next().ok_or_else(|| usage().to_string())?;
    let actor = Actor::parse(&actor_raw)?;
    let kind = EventKind::parse(&kind_raw)?;
    let mut ttl_ms = kind.default_ttl_ms();
    let mut saw_ttl = false;

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--ttl-ms" if !saw_ttl => {
                ttl_ms = parse_bounded_u64("--ttl-ms", args.next(), MIN_TTL_MS, MAX_TTL_MS)?;
                saw_ttl = true;
            }
            "--ttl-ms" => return Err("--ttl-ms may only be specified once".into()),
            _ => return Err(format!("unknown argument '{flag}'\n{}", usage())),
        }
    }

    Ok(Cli {
        actor,
        kind,
        ttl_ms,
    })
}

fn effective_uid() -> io::Result<u32> {
    let status = fs::read_to_string("/proc/self/status")?;
    let uid_line = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Uid missing from status"))?;
    uid_line
        .split_whitespace()
        .nth(2)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "effective UID missing"))?
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid effective UID"))
}

fn owned_runtime_dir(path: &Path, uid: u32) -> bool {
    path.is_absolute()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.is_dir() && metadata.uid() == uid)
}

fn runtime_dir(uid: u32) -> io::Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        if owned_runtime_dir(&path, uid) {
            return Ok(path);
        }
    }

    let fallback = PathBuf::from(format!("/run/user/{uid}"));
    if owned_runtime_dir(&fallback, uid) {
        Ok(fallback)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no trusted user runtime directory is available",
        ))
    }
}

fn protocol_line(cli: Cli, created_ms: u64) -> String {
    format!("v1 {created_ms} {} {}\n", cli.kind.as_str(), cli.ttl_ms)
}

/// Queue-entry suffix: `<20-digit zero-padded created-ms>.<pid>`. The padded
/// timestamp makes plain lexicographic filename order equal delivery order.
fn queue_entry_suffix(created_ms: u64) -> String {
    format!("{created_ms:020}.{}", process::id())
}

fn is_queue_entry_name(name: &str, prefix: &str) -> bool {
    let Some(suffix) = name.strip_prefix(prefix) else {
        return false;
    };
    let mut parts = suffix.splitn(2, '.');
    let (Some(stamp), Some(pid)) = (parts.next(), parts.next()) else {
        return false;
    };
    stamp.len() == 20
        && !pid.is_empty()
        && pid.len() <= 10
        && stamp.bytes().all(|byte| byte.is_ascii_digit())
        && pid.bytes().all(|byte| byte.is_ascii_digit())
}

/// Drop the oldest queued records for one actor until a new record fits
/// within [`MAX_QUEUED_RECORDS`].
fn prune_queue(runtime_dir: &Path, prefix: &str) -> io::Result<()> {
    let mut pending: Vec<String> = fs::read_dir(runtime_dir)?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_queue_entry_name(name, prefix))
        .collect();
    if pending.len() < MAX_QUEUED_RECORDS {
        return Ok(());
    }
    pending.sort();
    for name in &pending[..=pending.len() - MAX_QUEUED_RECORDS] {
        let _ = fs::remove_file(runtime_dir.join(name));
    }
    Ok(())
}

fn emit(cli: Cli) -> io::Result<PathBuf> {
    let uid = effective_uid()?;
    let runtime_dir = runtime_dir(uid)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "system clock is before epoch"))?;
    let created_ms = u64::try_from(now.as_millis())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "timestamp overflow"))?;
    let line = protocol_line(cli, created_ms);
    if line.len() > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("event record exceeds {MAX_RECORD_BYTES} bytes"),
        ));
    }

    let actor = cli.actor.as_str();
    let prefix = format!("lianli-agent-event-{actor}.");
    let mut temporary = None;
    let mut file = None;
    for attempt in 0..16u8 {
        let candidate = runtime_dir.join(format!(
            ".lianli-agent-event-{actor}.{}.{}.tmp",
            process::id(),
            attempt
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(opened) => {
                temporary = Some(candidate);
                file = Some(opened);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    let temporary = temporary.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve a temporary event file",
        )
    })?;
    let mut file = file.expect("temporary path and file are created together");
    if let Err(error) = file
        .set_permissions(fs::Permissions::from_mode(0o600))
        .and_then(|_| file.write_all(line.as_bytes()))
        .and_then(|_| file.flush())
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    // Prune only after the replacement is fully written, so a failed emit
    // can never have destroyed a previously accepted record.
    if let Err(error) = prune_queue(&runtime_dir, &prefix) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    // Publish without clobbering: `hard_link` fails if the destination
    // exists, unlike `rename`, so an exact timestamp+pid collision (clock
    // rollback plus pid reuse) shifts forward instead of losing a record.
    for bump in 0..4u64 {
        let destination = runtime_dir.join(format!(
            "{prefix}{}",
            queue_entry_suffix(created_ms.saturating_add(bump))
        ));
        match fs::hard_link(&temporary, &destination) {
            Ok(()) => {
                let _ = fs::remove_file(&temporary);
                return Ok(destination);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
        }
    }
    let _ = fs::remove_file(&temporary);
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve an event queue entry",
    ))
}

fn run() -> Result<(), String> {
    let cli = parse_args(env::args().skip(1))?;
    emit(cli).map_err(|error| format!("failed to emit event: {error}"))?;
    Ok(())
}

fn main() {
    if env::args()
        .nth(1)
        .is_some_and(|arg| arg == "-h" || arg == "--help")
    {
        println!("{}", usage());
        return;
    }
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn defaults_are_kind_specific() {
        let parsed = parse_args(strings(&["codex", "permission"])).unwrap();
        assert_eq!(parsed.actor, Actor::Codex);
        assert_eq!(parsed.kind, EventKind::Permission);
        assert_eq!(parsed.ttl_ms, 10_000);
    }

    #[test]
    fn explicit_bounds_are_accepted() {
        let parsed = parse_args(strings(&["claude", "read", "--ttl-ms", "1250"])).unwrap();
        assert_eq!(parsed.ttl_ms, 1_250);
    }

    #[test]
    fn invalid_values_are_rejected() {
        assert!(parse_args(strings(&["unknown", "tool"])).is_err());
        assert!(parse_args(strings(&["both", "tool"])).is_err());
        assert!(parse_args(strings(&["patch", "tool"])).is_err());
        assert!(parse_args(strings(&["codex", "unknown"])).is_err());
        assert!(parse_args(strings(&["codex", "tool", "--ttl-ms", "100"])).is_err());
        assert!(parse_args(strings(&["codex", "tool", "--ttl-ms", "60001"])).is_err());
        assert!(parse_args(strings(&["codex", "tool", "--priority", "100"])).is_err());
    }

    #[test]
    fn wire_record_matches_v1_contract() {
        let cli = parse_args(strings(&["codex", "success"])).unwrap();
        let line = protocol_line(cli, 1234);
        let fields: Vec<&str> = line.trim_end().split(' ').collect();
        assert_eq!(fields, ["v1", "1234", "success", "8000"]);
    }

    #[test]
    fn queue_entry_names_are_strict_and_sort_in_delivery_order() {
        let prefix = "lianli-agent-event-codex.";
        let early = format!("{prefix}{}", queue_entry_suffix(9));
        let late = format!("{prefix}{}", queue_entry_suffix(10));
        assert!(is_queue_entry_name(&early, prefix));
        assert!(is_queue_entry_name(&late, prefix));
        assert!(early < late, "{early} should sort before {late}");

        for invalid in [
            "lianli-agent-event-codex".to_string(),
            format!("{prefix}12.34"),
            format!("{prefix}{}", "9".repeat(20)),
            format!("{prefix}{}.abc", "0".repeat(20)),
            format!("{prefix}{}.{}", "0".repeat(20), "1".repeat(11)),
            format!(".hidden.{}", queue_entry_suffix(9)),
        ] {
            assert!(!is_queue_entry_name(&invalid, prefix), "{invalid}");
        }
    }

    #[test]
    fn prune_drops_only_the_oldest_records_beyond_the_cap() {
        let directory = env::temp_dir().join(format!(
            "lianli-agent-event-prune-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let prefix = "lianli-agent-event-claude.";
        for created_ms in 0..MAX_QUEUED_RECORDS as u64 + 4 {
            let name = format!("{prefix}{created_ms:020}.7");
            fs::write(directory.join(name), "v1 0 tool 1500\n").unwrap();
        }
        fs::write(directory.join("lianli-agent-event-codex.00000000000000000001.7"), "x").unwrap();

        prune_queue(&directory, prefix).unwrap();

        let mut remaining: Vec<String> = fs::read_dir(&directory)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with(prefix))
            .collect();
        remaining.sort();
        assert_eq!(remaining.len(), MAX_QUEUED_RECORDS - 1);
        assert_eq!(remaining[0], format!("{prefix}{:020}.7", 5));
        assert!(directory
            .join("lianli-agent-event-codex.00000000000000000001.7")
            .exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
