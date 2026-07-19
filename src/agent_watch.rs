use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CODEX: u8 = 0b01;
const CLAUDE: u8 = 0b10;
const BOTH: u8 = CODEX | CLAUDE;
const TICK_INTERVAL: Duration = Duration::from_millis(100);
const PROCESS_INTERVAL: Duration = Duration::from_secs(1);
const THERMAL_INTERVAL: Duration = Duration::from_secs(1);
const CONTROL_INTERVAL: Duration = Duration::from_secs(1);
const ENTER_DURATION_MS: u64 = 2_000;
const EXIT_DURATION_MS: u64 = 3_000;
const RELIEF_DURATION_MS: u64 = 3_000;
const MIN_EVENT_TTL_MS: u64 = 250;
const MAX_EVENT_TTL_MS: u64 = 60_000;
const MAX_EVENT_RECORD_BYTES: u64 = 512;
const MAX_EVENT_FUTURE_SKEW_MS: u64 = 2_000;
const MAX_CONTROL_RECORD_BYTES: u64 = 512;
const MAX_CONTROL_AGE_MS: u64 = 15_000;
const LOGICAL_WIDTH: i32 = 2_288;
const LOGICAL_HEIGHT: i32 = 1_048;
#[cfg(test)]
const MASCOT_WIDTH: i32 = 600;
#[cfg(test)]
const MASCOT_HEIGHT: i32 = 400;
const COOLANT_FILE_PREFIX: &str = "lianli-coolant-";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ProcKey {
    pid: u32,
    start_time: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Counters {
    cpu_ticks: u64,
    chars: u64,
    storage_bytes: u64,
}

#[derive(Clone, Debug)]
struct ProcSample {
    key: ProcKey,
    ppid: u32,
    comm: String,
    counters: Counters,
}

#[derive(Clone, Copy, Debug, Default)]
struct Signals {
    cpu_ticks: u64,
    chars: u64,
    storage_bytes: u64,
    cgroup_cpu_usec: u64,
    cgroup_hot: bool,
    cgroup_warm: bool,
    spawned: bool,
}

impl Signals {
    fn add_process_delta(&mut self, current: Counters, previous: Counters) {
        self.cpu_ticks = self
            .cpu_ticks
            .saturating_add(current.cpu_ticks.saturating_sub(previous.cpu_ticks));
        self.chars = self
            .chars
            .saturating_add(current.chars.saturating_sub(previous.chars));
        self.storage_bytes = self
            .storage_bytes
            .saturating_add(current.storage_bytes.saturating_sub(previous.storage_bytes));
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Presence {
    present: bool,
    misses: u8,
}

impl Presence {
    fn update(&mut self, observed: bool) {
        if observed {
            self.present = true;
            self.misses = 0;
        } else if self.present {
            self.misses = self.misses.saturating_add(1);
            if self.misses >= 2 {
                self.present = false;
                self.misses = 0;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ActivityDetector {
    active: bool,
    warm_streak: u8,
    cold_streak: u8,
}

impl Default for ActivityDetector {
    fn default() -> Self {
        Self {
            active: false,
            warm_streak: 0,
            cold_streak: 0,
        }
    }
}

impl ActivityDetector {
    fn update(&mut self, present: bool, signals: Signals) {
        if !present {
            self.active = false;
            self.warm_streak = 0;
            self.cold_streak = 0;
            return;
        }

        let hot = signals.spawned
            || signals.cpu_ticks >= 4
            || signals.chars >= 64 * 1024
            || signals.storage_bytes >= 64 * 1024
            || signals.cgroup_hot;
        let warm = signals.cgroup_warm
            || signals.chars >= 32 * 1024
            || signals.storage_bytes >= 16 * 1024
            || (signals.cpu_ticks >= 2
                && (signals.chars >= 16 * 1024 || signals.storage_bytes >= 4 * 1024));

        if hot {
            self.active = true;
            self.warm_streak = 0;
            self.cold_streak = 0;
        } else if warm {
            self.warm_streak = self.warm_streak.saturating_add(1);
            self.cold_streak = 0;
            if self.warm_streak >= 2 {
                self.active = true;
            }
        } else {
            self.warm_streak = 0;
            self.cold_streak = self.cold_streak.saturating_add(1);
            if self.active && self.cold_streak >= 4 {
                self.active = false;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CgroupTracker {
    last_usage_usec: u64,
    idle_ewma_usec: f64,
    bootstrap_samples: u8,
}

impl CgroupTracker {
    fn new(usage_usec: u64) -> Self {
        Self {
            last_usage_usec: usage_usec,
            idle_ewma_usec: 25_000.0,
            bootstrap_samples: 0,
        }
    }

    fn observe(&mut self, usage_usec: u64) -> (u64, bool, bool) {
        let delta = usage_usec.saturating_sub(self.last_usage_usec);
        self.last_usage_usec = usage_usec;

        // Learn each terminal scope independently before using it as a
        // signal. This prevents one or several naturally busy idle terminals
        // from permanently pinning Patch in the active state.
        if self.bootstrap_samples < 3 {
            self.idle_ewma_usec = if self.bootstrap_samples == 0 {
                delta as f64
            } else {
                self.idle_ewma_usec * 0.5 + delta as f64 * 0.5
            };
            self.bootstrap_samples += 1;
            return (delta, false, false);
        }

        let hot_threshold = (self.idle_ewma_usec + 25_000.0).max(50_000.0) as u64;
        let warm_threshold = (self.idle_ewma_usec + 12_000.0).max(35_000.0) as u64;
        let hot = delta >= hot_threshold;
        let warm = delta >= warm_threshold;

        // Warm-but-steady CPU is allowed to become the learned idle baseline;
        // only clear spikes are excluded from learning.
        if !hot {
            self.idle_ewma_usec = self.idle_ewma_usec * 0.9 + delta as f64 * 0.1;
        }
        (delta, hot, warm)
    }
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

fn parse_stat(pid: u32, stat: &str, comm: String, counters: Counters) -> Option<ProcSample> {
    let close = stat.rfind(") ")?;
    let fields: Vec<&str> = stat[close + 2..].split_whitespace().collect();
    if fields.len() <= 19 {
        return None;
    }
    let ppid = fields.get(1)?.parse().ok()?;
    let user_ticks: u64 = fields.get(11)?.parse().ok()?;
    let system_ticks: u64 = fields.get(12)?.parse().ok()?;
    let start_time = fields.get(19)?.parse().ok()?;
    Some(ProcSample {
        key: ProcKey { pid, start_time },
        ppid,
        comm,
        counters: Counters {
            cpu_ticks: user_ticks.saturating_add(system_ticks),
            ..counters
        },
    })
}

fn read_io_counters(pid: u32) -> Counters {
    let mut counters = Counters::default();
    let Ok(contents) = fs::read_to_string(format!("/proc/{pid}/io")) else {
        return counters;
    };
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        let Some(value) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        match name {
            "rchar:" | "wchar:" => counters.chars = counters.chars.saturating_add(value),
            "read_bytes:" | "write_bytes:" => {
                counters.storage_bytes = counters.storage_bytes.saturating_add(value)
            }
            _ => {}
        }
    }
    counters
}

fn scan_processes(uid: u32) -> Vec<ProcSample> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut processes = Vec::new();

    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.uid() != uid {
            continue;
        }

        let Ok(comm) = fs::read_to_string(format!("/proc/{pid}/comm")) else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        if let Some(sample) = parse_stat(pid, &stat, comm.trim().to_string(), Counters::default()) {
            processes.push(sample);
        }
    }
    processes
}

fn root_mask(comm: &str) -> u8 {
    match comm {
        "codex" => CODEX,
        "claude" => CLAUDE,
        _ => 0,
    }
}

fn classify_process(pid: u32, by_pid: &HashMap<u32, &ProcSample>, roots: &HashMap<u32, u8>) -> u8 {
    let mut current = pid;
    let mut seen = HashSet::new();
    for _ in 0..64 {
        if let Some(mask) = roots.get(&current) {
            return *mask;
        }
        if !seen.insert(current) {
            break;
        }
        let Some(process) = by_pid.get(&current) else {
            break;
        };
        if process.ppid == 0 || process.ppid == current {
            break;
        }
        current = process.ppid;
    }
    0
}

fn read_cgroup_path(pid: u32) -> Option<String> {
    let contents = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    contents
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(str::to_string)
}

fn trusted_agent_cgroup(path: &str) -> bool {
    if path == "/" || !path.contains("/app.slice/") {
        return false;
    }
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".scope"))
}

fn read_cgroup_cpu_usec(path: &str) -> Option<u64> {
    let relative = path.trim_start_matches('/');
    let cpu_stat = Path::new("/sys/fs/cgroup").join(relative).join("cpu.stat");
    let contents = fs::read_to_string(cpu_stat).ok()?;
    contents.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? == "usage_usec" {
            fields.next()?.parse().ok()
        } else {
            None
        }
    })
}

fn presence_mask(codex: Presence, claude: Presence) -> u8 {
    (if codex.present { CODEX } else { 0 }) | (if claude.present { CLAUDE } else { 0 })
}

fn base_state_name(mask: u8, codex_active: bool, claude_active: bool) -> &'static str {
    match (mask, codex_active, claude_active) {
        (0, _, _) => "idle",
        (CODEX, true, _) => "codex-active",
        (CODEX, false, _) => "codex-wait",
        (CLAUDE, _, true) => "claude-active",
        (CLAUDE, _, false) => "claude-wait",
        (BOTH, true, _) | (BOTH, _, true) => "both-active",
        (BOTH, false, false) => "both-wait",
        _ => "idle",
    }
}

fn enter_state(mask: u8) -> &'static str {
    match mask {
        CODEX => "codex-enter",
        CLAUDE => "claude-enter",
        BOTH => "both-enter",
        _ => "idle",
    }
}

fn exit_state(mask: u8) -> &'static str {
    match mask {
        CODEX => "codex-exit",
        CLAUDE => "claude-exit",
        BOTH => "both-exit",
        _ => "idle",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentActor {
    Codex,
    Claude,
}

impl AgentActor {
    fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    fn mask(self) -> u8 {
        match self {
            Self::Codex => CODEX,
            Self::Claude => CLAUDE,
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
    fn parse(value: &str) -> Option<Self> {
        match value {
            "start" => Some(Self::Start),
            "exit" => Some(Self::Exit),
            "tool" => Some(Self::Tool),
            "read" => Some(Self::Read),
            "permission" => Some(Self::Permission),
            "error" => Some(Self::Error),
            "success" => Some(Self::Success),
            _ => None,
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Error => 230,
            Self::Permission => 220,
            Self::Success => 205,
            Self::Exit => 195,
            Self::Tool => 175,
            Self::Read => 170,
            Self::Start => 160,
        }
    }

    fn state(self, actor: AgentActor, mask: u8) -> &'static str {
        match self {
            Self::Error => "event-error",
            Self::Permission => "event-attention",
            Self::Success => "event-success",
            Self::Exit => "event-done",
            Self::Tool => "event-tool",
            Self::Read => "event-reading",
            Self::Start if mask == BOTH => "both-enter",
            Self::Start => enter_state(actor.mask()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EventRecord {
    actor: AgentActor,
    kind: EventKind,
    created_ms: u64,
    ttl_ms: u64,
}

impl EventRecord {
    fn expires_ms(self) -> Option<u64> {
        self.created_ms.checked_add(self.ttl_ms)
    }

    fn remaining_ms(self, now_unix_ms: u64) -> Option<u64> {
        self.expires_ms()?.checked_sub(now_unix_ms)
    }
}

fn parse_event_record(actor: AgentActor, bytes: &[u8], now_unix_ms: u64) -> Option<EventRecord> {
    if bytes.len() as u64 > MAX_EVENT_RECORD_BYTES {
        return None;
    }
    let value = std::str::from_utf8(bytes).ok()?;
    let mut fields = value.split_ascii_whitespace();
    if fields.next()? != "v1" {
        return None;
    }
    let created_ms = fields.next()?.parse::<u64>().ok()?;
    let kind = EventKind::parse(fields.next()?)?;
    let ttl_ms = fields.next()?.parse::<u64>().ok()?;
    if fields.next().is_some()
        || !(MIN_EVENT_TTL_MS..=MAX_EVENT_TTL_MS).contains(&ttl_ms)
        || created_ms > now_unix_ms.saturating_add(MAX_EVENT_FUTURE_SKEW_MS)
    {
        return None;
    }
    let record = EventRecord {
        actor,
        kind,
        created_ms,
        ttl_ms,
    };
    if record.expires_ms()? <= now_unix_ms {
        return None;
    }
    Some(record)
}

/// Queue-entry suffix contract shared with the writers:
/// `<prefix><20-digit zero-padded created-ms>.<pid>`. Padding makes plain
/// lexicographic filename order equal delivery order.
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

/// Atomically claim one record file and hand its validated bytes to `parse`.
/// The claimed file is always deleted, even when validation fails, so
/// malformed or expired records cannot wedge a queue.
fn claim_record<T>(
    source: &Path,
    claim: &Path,
    uid: u32,
    max_bytes: u64,
    parse: impl FnOnce(&[u8]) -> Option<T>,
) -> Option<T> {
    if fs::rename(source, claim).is_err() {
        return None;
    }
    let record = (|| {
        let metadata = fs::symlink_metadata(claim).ok()?;
        if !metadata.file_type().is_file() || metadata.uid() != uid || metadata.len() > max_bytes {
            return None;
        }
        let bytes = fs::read(claim).ok()?;
        parse(&bytes)
    })();
    let _ = fs::remove_file(claim);
    record
}

/// Drain up to `MAX_RECORDS_PER_TICK` queued records for one prefix in
/// delivery order. Anything beyond the per-tick cap stays queued for the
/// next 100 ms tick.
fn drain_queue<T>(
    runtime_dir: &Path,
    prefix: &str,
    uid: u32,
    max_bytes: u64,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Vec<T> {
    const MAX_RECORDS_PER_TICK: usize = 8;
    let Ok(entries) = fs::read_dir(runtime_dir) else {
        return Vec::new();
    };
    let mut pending: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_queue_entry_name(name, prefix))
        .collect();
    pending.sort();
    pending.truncate(MAX_RECORDS_PER_TICK);

    let mut records = Vec::new();
    for name in pending {
        let source = runtime_dir.join(&name);
        let claim = runtime_dir.join(format!(".{name}.watch-{}", process::id()));
        if let Some(record) = claim_record(&source, &claim, uid, max_bytes, &mut parse) {
            records.push(record);
        }
    }
    records
}

fn consume_agent_events(
    runtime_dir: &Path,
    actor: AgentActor,
    uid: u32,
    now_unix_ms: u64,
) -> Vec<EventRecord> {
    // Writers older than the queue format still use one fixed mailbox file.
    let legacy_source = runtime_dir.join(format!("lianli-agent-event-{}", actor.as_str()));
    let legacy_claim = runtime_dir.join(format!(
        ".lianli-agent-event-{}.watch-{}",
        actor.as_str(),
        process::id()
    ));
    let mut records: Vec<EventRecord> = claim_record(
        &legacy_source,
        &legacy_claim,
        uid,
        MAX_EVENT_RECORD_BYTES,
        |bytes| parse_event_record(actor, bytes, now_unix_ms),
    )
    .into_iter()
    .collect();

    records.extend(drain_queue(
        runtime_dir,
        &format!("lianli-agent-event-{}.", actor.as_str()),
        uid,
        MAX_EVENT_RECORD_BYTES,
        |bytes| parse_event_record(actor, bytes, now_unix_ms),
    ));
    // A legacy record is not necessarily older than queued ones. Later
    // records replace earlier same-actor-and-kind entries during ingestion,
    // so deliver in created-ms order (stable: legacy first on exact ties).
    records.sort_by_key(|record| record.created_ms);
    records
}

fn unix_time_ms() -> io::Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "system clock is before epoch"))?;
    u64::try_from(elapsed.as_millis())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "timestamp overflow"))
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RoamPattern {
    Full,
    Horizontal,
    Vertical,
}

impl RoamPattern {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "full" => Some(Self::Full),
            "horizontal" => Some(Self::Horizontal),
            "vertical" => Some(Self::Vertical),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RoamPace {
    Slow,
    Normal,
    Quick,
}

impl RoamPace {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "slow" => Some(Self::Slow),
            "normal" => Some(Self::Normal),
            "quick" => Some(Self::Quick),
            _ => None,
        }
    }

    fn speed_pixels_per_second(self) -> f64 {
        match self {
            Self::Slow => 260.0,
            Self::Normal => 400.0,
            Self::Quick => 620.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ControlSettings {
    manual_mode: bool,
    roam_enabled: bool,
    pattern: RoamPattern,
    pace: RoamPace,
    micro_idles: bool,
    activity_reactions: bool,
    thermal_reactions: bool,
}

impl Default for ControlSettings {
    fn default() -> Self {
        Self {
            manual_mode: false,
            roam_enabled: true,
            pattern: RoamPattern::Full,
            pace: RoamPace::Normal,
            micro_idles: true,
            activity_reactions: true,
            thermal_reactions: true,
        }
    }
}

fn json_value_after_key<'a>(value: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("\"{key}\"");
    let (_, remainder) = value.split_once(&marker)?;
    remainder
        .trim_start()
        .strip_prefix(':')
        .map(str::trim_start)
}

fn json_bool(value: &str, key: &str) -> Option<bool> {
    let value = json_value_after_key(value, key)?;
    if value.starts_with("true") {
        Some(true)
    } else if value.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn json_string<'a>(value: &'a str, key: &str) -> Option<&'a str> {
    let value = json_value_after_key(value, key)?.strip_prefix('"')?;
    let end = value.find('"')?;
    let parsed = &value[..end];
    (!parsed.contains(['\\', '\n', '\r'])).then_some(parsed)
}

fn parse_control_settings(value: &str) -> ControlSettings {
    let mut settings = ControlSettings::default();
    settings.manual_mode = json_string(value, "mode")
        .map(|mode| mode == "manual")
        .unwrap_or(settings.manual_mode);
    settings.roam_enabled = json_bool(value, "roamEnabled")
        .or_else(|| json_bool(value, "roam_enabled"))
        .unwrap_or(settings.roam_enabled);
    settings.micro_idles = json_bool(value, "microIdles")
        .or_else(|| json_bool(value, "micro_idles"))
        .unwrap_or(settings.micro_idles);
    settings.activity_reactions = json_bool(value, "activityReactions")
        .or_else(|| json_bool(value, "activity_reactions"))
        .unwrap_or(settings.activity_reactions);
    settings.thermal_reactions = json_bool(value, "thermalReactions")
        .or_else(|| json_bool(value, "thermal_reactions"))
        .unwrap_or(settings.thermal_reactions);
    settings.pattern = json_string(value, "pattern")
        .or_else(|| json_string(value, "roam_pattern"))
        .and_then(RoamPattern::parse)
        .unwrap_or(settings.pattern);
    settings.pace = json_string(value, "pace")
        .and_then(RoamPace::parse)
        .unwrap_or(settings.pace);
    settings
}

fn control_config_path() -> Option<PathBuf> {
    let home = PathBuf::from(env::var_os("HOME")?);
    home.is_absolute()
        .then(|| home.join(".config/lianli/mascot-control.json"))
}

fn read_control_settings(path: Option<&Path>, uid: u32) -> ControlSettings {
    let Some(path) = path else {
        return ControlSettings::default();
    };
    let Some(metadata) = fs::symlink_metadata(path).ok() else {
        return ControlSettings::default();
    };
    if !metadata.file_type().is_file()
        || metadata.uid() != uid
        || metadata.len() > 4_096
        || metadata.mode() & 0o077 != 0
    {
        return ControlSettings::default();
    }
    fs::read_to_string(path)
        .ok()
        .map(|value| parse_control_settings(&value))
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreviewKind {
    Tool,
    Read,
    Attention,
    Success,
}

impl PreviewKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "tool" => Some(Self::Tool),
            "read" => Some(Self::Read),
            "attention" => Some(Self::Attention),
            "success" => Some(Self::Success),
            _ => None,
        }
    }

    fn state(self) -> &'static str {
        match self {
            Self::Tool => "event-tool",
            Self::Read => "event-reading",
            Self::Attention => "event-attention",
            Self::Success => "event-success",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlCommand {
    Auto,
    Home,
    Move(ScreenLocation),
    Preview(PreviewKind),
}

fn parse_control_record(bytes: &[u8], now_unix_ms: u64) -> Option<(u64, ControlCommand)> {
    if bytes.len() as u64 > MAX_CONTROL_RECORD_BYTES {
        return None;
    }
    let value = std::str::from_utf8(bytes).ok()?;
    let mut fields = value.split_ascii_whitespace();
    if fields.next()? != "v1" {
        return None;
    }
    let created_ms = fields.next()?.parse::<u64>().ok()?;
    let command = fields.next()?;
    let argument = fields.next()?;
    if fields.next().is_some()
        || created_ms > now_unix_ms.saturating_add(MAX_EVENT_FUTURE_SKEW_MS)
        || now_unix_ms.saturating_sub(created_ms) > MAX_CONTROL_AGE_MS
    {
        return None;
    }
    let command = match command {
        "auto" if argument == "-" => ControlCommand::Auto,
        "home" if argument == "-" => ControlCommand::Home,
        "move" => ControlCommand::Move(ScreenLocation::parse(argument)?),
        "preview" => ControlCommand::Preview(PreviewKind::parse(argument)?),
        _ => return None,
    };
    Some((created_ms, command))
}

fn consume_control_commands(runtime_dir: &Path, uid: u32, now_unix_ms: u64) -> Vec<ControlCommand> {
    // Bridges older than the queue format still use one fixed mailbox file.
    let legacy_source = runtime_dir.join("lianli-agent-command");
    let legacy_claim = runtime_dir.join(format!(".lianli-agent-command.watch-{}", process::id()));
    let mut stamped: Vec<(u64, ControlCommand)> = claim_record(
        &legacy_source,
        &legacy_claim,
        uid,
        MAX_CONTROL_RECORD_BYTES,
        |bytes| parse_control_record(bytes, now_unix_ms),
    )
    .into_iter()
    .collect();

    stamped.extend(drain_queue(
        runtime_dir,
        "lianli-agent-command.",
        uid,
        MAX_CONTROL_RECORD_BYTES,
        |bytes| parse_control_record(bytes, now_unix_ms),
    ));
    // A legacy record is not necessarily older than queued ones: apply in
    // created-ms order. The stable sort keeps legacy first on exact ties.
    stamped.sort_by_key(|(created_ms, _)| *created_ms);
    stamped.into_iter().map(|(_, command)| command).collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Temperatures {
    cpu_c: Option<f32>,
    gpu_c: Option<f32>,
    coolant_c: Option<f32>,
}

fn parse_temperature(value: &str, millidegrees: bool) -> Option<f32> {
    let mut parsed = value.trim().parse::<f32>().ok()?;
    if millidegrees {
        parsed /= 1_000.0;
    }
    (parsed.is_finite() && (-50.0..=200.0).contains(&parsed)).then_some(parsed)
}

/// `tempN_label` file names for one hwmon device, in numeric channel order
/// (`temp2` before `temp10`). The labeled channel is not always `temp1`:
/// amdgpu exposes `edge`, `junction`, and `mem` as `temp1..temp3`, and other
/// drivers start even later.
fn hwmon_label_files(device: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(device) else {
        return Vec::new();
    };
    let mut channels: Vec<(u16, String)> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| {
            let channel = name
                .strip_prefix("temp")?
                .strip_suffix("_label")?
                .parse::<u16>()
                .ok()?;
            (channel > 0).then_some((channel, name))
        })
        .collect();
    channels.sort();
    channels.into_iter().map(|(_, name)| name).collect()
}

fn read_hwmon_temperature(
    hwmon_root: &Path,
    expected_name: &str,
    expected_label: &str,
) -> Option<f32> {
    let entries = fs::read_dir(hwmon_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(name) = fs::read_to_string(path.join("name")) else {
            continue;
        };
        if name.trim() != expected_name {
            continue;
        }
        for label_file in hwmon_label_files(&path) {
            let Ok(label) = fs::read_to_string(path.join(&label_file)) else {
                continue;
            };
            if label.trim() != expected_label {
                continue;
            }
            let input_file = format!("{}input", label_file.trim_end_matches("label"));
            let Ok(input) = fs::read_to_string(path.join(input_file)) else {
                continue;
            };
            if let Some(value) = parse_temperature(&input, true) {
                return Some(value);
            }
        }
    }
    None
}

fn read_coolant_temperature(path: &Path, uid: u32) -> Option<f32> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.uid() != uid {
        return None;
    }
    let input = fs::read_to_string(path).ok()?;
    parse_temperature(&input, false)
}

fn configured_hwmon_root(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(fallback))
}

fn configured_sensor_name(variable: &str, fallback: &str) -> String {
    env::var(variable)
        .ok()
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .unwrap_or_else(|| fallback.to_string())
}

fn coolant_path(runtime_dir: &Path) -> Option<PathBuf> {
    if let Some(configured) = env::var_os("LIANLI_COOLANT_FILE").map(PathBuf::from) {
        if configured.file_name() == Some(configured.as_os_str()) {
            return Some(runtime_dir.join(configured));
        }
    }

    let mut candidates: Vec<PathBuf> = fs::read_dir(runtime_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(COOLANT_FILE_PREFIX))
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

fn read_temperatures(runtime_dir: &Path, uid: u32) -> Temperatures {
    let cpu_root = configured_hwmon_root("LIANLI_CPU_HWMON_ROOT", "/sys/class/hwmon");
    let gpu_root = configured_hwmon_root("LIANLI_GPU_HWMON_ROOT", "/sys/class/hwmon");
    let cpu_name = configured_sensor_name("LIANLI_CPU_HWMON_NAME", "k10temp");
    let cpu_label = configured_sensor_name("LIANLI_CPU_TEMP_LABEL", "Tctl");
    let gpu_name = configured_sensor_name("LIANLI_GPU_HWMON_NAME", "amdgpu");
    let gpu_label = configured_sensor_name("LIANLI_GPU_TEMP_LABEL", "edge");
    Temperatures {
        cpu_c: read_hwmon_temperature(&cpu_root, &cpu_name, &cpu_label),
        gpu_c: read_hwmon_temperature(&gpu_root, &gpu_name, &gpu_label),
        coolant_c: coolant_path(runtime_dir)
            .as_deref()
            .and_then(|path| read_coolant_temperature(path, uid)),
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ThermalLevel {
    Normal,
    Warm,
    Hot,
    Critical,
}

#[derive(Clone, Copy)]
struct ThermalThresholds {
    warm_enter: f32,
    warm_clear: f32,
    hot_enter: f32,
    hot_clear: f32,
    critical_enter: f32,
    critical_clear: f32,
}

const CPU_THRESHOLDS: ThermalThresholds = ThermalThresholds {
    warm_enter: 75.0,
    warm_clear: 70.0,
    hot_enter: 85.0,
    hot_clear: 80.0,
    critical_enter: 92.0,
    critical_clear: 88.0,
};
const GPU_THRESHOLDS: ThermalThresholds = ThermalThresholds {
    warm_enter: 75.0,
    warm_clear: 70.0,
    hot_enter: 85.0,
    hot_clear: 80.0,
    critical_enter: 100.0,
    critical_clear: 95.0,
};
const COOLANT_THRESHOLDS: ThermalThresholds = ThermalThresholds {
    warm_enter: 36.0,
    warm_clear: 34.0,
    hot_enter: 40.0,
    hot_clear: 38.0,
    critical_enter: 45.0,
    critical_clear: 42.0,
};

fn temperature_level(value: f32, thresholds: ThermalThresholds) -> ThermalLevel {
    if value >= thresholds.critical_enter {
        ThermalLevel::Critical
    } else if value >= thresholds.hot_enter {
        ThermalLevel::Hot
    } else if value >= thresholds.warm_enter {
        ThermalLevel::Warm
    } else {
        ThermalLevel::Normal
    }
}

fn below_clear(value: f32, level: ThermalLevel, thresholds: ThermalThresholds) -> bool {
    match level {
        ThermalLevel::Normal => false,
        ThermalLevel::Warm => value < thresholds.warm_clear,
        ThermalLevel::Hot => value < thresholds.hot_clear,
        ThermalLevel::Critical => value < thresholds.critical_clear,
    }
}

#[derive(Clone, Copy, Debug)]
struct SensorThermalTracker {
    level: ThermalLevel,
    upward_candidate: ThermalLevel,
    upward_streak: u8,
    cool_streak: u8,
    missing_streak: u8,
}

impl Default for SensorThermalTracker {
    fn default() -> Self {
        Self {
            level: ThermalLevel::Normal,
            upward_candidate: ThermalLevel::Normal,
            upward_streak: 0,
            cool_streak: 0,
            missing_streak: 0,
        }
    }
}

impl SensorThermalTracker {
    fn update(&mut self, value: Option<f32>, thresholds: ThermalThresholds) -> bool {
        let Some(value) = value.filter(|value| value.is_finite()) else {
            self.missing_streak = self.missing_streak.saturating_add(1);
            if self.missing_streak >= 10 && self.level != ThermalLevel::Normal {
                self.level = ThermalLevel::Normal;
                self.upward_candidate = ThermalLevel::Normal;
                self.upward_streak = 0;
                self.cool_streak = 0;
                return true;
            }
            return false;
        };
        self.missing_streak = 0;
        let candidate = temperature_level(value, thresholds);
        if candidate > self.level {
            self.cool_streak = 0;
            if candidate == ThermalLevel::Critical {
                self.level = ThermalLevel::Critical;
                self.upward_candidate = candidate;
                self.upward_streak = 0;
            } else {
                if candidate == self.upward_candidate {
                    self.upward_streak = self.upward_streak.saturating_add(1);
                } else {
                    self.upward_candidate = candidate;
                    self.upward_streak = 1;
                }
                if self.upward_streak >= 2 {
                    self.level = candidate;
                    self.upward_streak = 0;
                }
            }
        } else if candidate < self.level && below_clear(value, self.level, thresholds) {
            self.upward_streak = 0;
            self.cool_streak = self.cool_streak.saturating_add(1);
            if self.cool_streak >= 3 {
                self.level = candidate;
                self.upward_candidate = candidate;
                self.cool_streak = 0;
            }
        } else {
            self.upward_candidate = candidate;
            self.upward_streak = 0;
            self.cool_streak = 0;
        }
        false
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ThermalTracker {
    cpu: SensorThermalTracker,
    gpu: SensorThermalTracker,
    coolant: SensorThermalTracker,
}

impl ThermalTracker {
    fn level(&self) -> ThermalLevel {
        self.cpu.level.max(self.gpu.level).max(self.coolant.level)
    }

    fn update(&mut self, temperatures: Temperatures) -> (ThermalLevel, ThermalLevel, bool) {
        let previous = self.level();
        let data_loss = self.cpu.update(temperatures.cpu_c, CPU_THRESHOLDS)
            | self.gpu.update(temperatures.gpu_c, GPU_THRESHOLDS)
            | self
                .coolant
                .update(temperatures.coolant_c, COOLANT_THRESHOLDS);
        (previous, self.level(), data_loss)
    }
}

#[derive(Clone, Copy, Debug)]
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
        }
    }

    fn next(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        value
    }

    fn range_inclusive(&mut self, min: u64, max: u64) -> u64 {
        min + self.next() % (max - min + 1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScreenLocation {
    Home,
    UpperLeft,
    Up,
    UpperRight,
    Left,
    Center,
    Right,
    LowerLeft,
    Down,
    LowerRight,
}

impl ScreenLocation {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "home" => Some(Self::Home),
            "upper-left" => Some(Self::UpperLeft),
            "up" => Some(Self::Up),
            "upper-right" => Some(Self::UpperRight),
            "left" => Some(Self::Left),
            "center" => Some(Self::Center),
            "right" => Some(Self::Right),
            "lower-left" => Some(Self::LowerLeft),
            "down" => Some(Self::Down),
            "lower-right" => Some(Self::LowerRight),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::UpperLeft => "upper-left",
            Self::Up => "up",
            Self::UpperRight => "upper-right",
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
            Self::LowerLeft => "lower-left",
            Self::Down => "down",
            Self::LowerRight => "lower-right",
        }
    }

    fn point(self) -> Point {
        match self {
            Self::Home => Point { x: 1_144, y: 720 },
            Self::UpperLeft => Point { x: 300, y: 200 },
            Self::Up => Point { x: 1_144, y: 200 },
            Self::UpperRight => Point { x: 1_988, y: 200 },
            Self::Left => Point { x: 300, y: 524 },
            Self::Center => Point { x: 1_144, y: 524 },
            Self::Right => Point { x: 1_988, y: 524 },
            Self::LowerLeft => Point { x: 300, y: 848 },
            Self::Down => Point { x: 1_144, y: 848 },
            Self::LowerRight => Point { x: 1_988, y: 848 },
        }
    }
}

type Waypoint = ScreenLocation;

fn squared_distance(left: Point, right: Point) -> i64 {
    let dx = i64::from(left.x - right.x);
    let dy = i64::from(left.y - right.y);
    dx * dx + dy * dy
}

#[cfg(test)]
fn movement_duration_ms(from: Point, to: Point) -> u64 {
    movement_duration_for_pace(from, to, RoamPace::Normal)
}

fn movement_duration_for_pace(from: Point, to: Point, pace: RoamPace) -> u64 {
    let dx = f64::from(to.x - from.x);
    let dy = f64::from(to.y - from.y);
    ((dx.hypot(dy) / pace.speed_pixels_per_second()) * 1_000.0)
        .round()
        .clamp(1_200.0, 5_000.0) as u64
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PositionCommand {
    seq: u64,
    point: Point,
    duration_ms: u64,
}

impl PositionCommand {
    fn line(self) -> String {
        format!(
            "v1 {} {} {} {}\n",
            self.seq, self.point.x, self.point.y, self.duration_ms
        )
    }
}

fn read_position_seq(path: &Path, fallback: u64) -> u64 {
    let Some(value) = fs::read_to_string(path).ok() else {
        return fallback;
    };
    let mut fields = value.split_ascii_whitespace();
    if fields.next() != Some("v1") {
        return fallback;
    }
    fields
        .next()
        .and_then(|seq| seq.parse::<u64>().ok())
        .unwrap_or(fallback)
        .max(fallback)
}

fn read_position_point(path: &Path) -> Option<Point> {
    let value = fs::read_to_string(path).ok()?;
    let mut fields = value.split_ascii_whitespace();
    if fields.next()? != "v1" {
        return None;
    }
    let _seq = fields.next()?.parse::<u64>().ok()?;
    let x = fields.next()?.parse::<i32>().ok()?;
    let y = fields.next()?.parse::<i32>().ok()?;
    let _duration_ms = fields.next()?.parse::<u64>().ok()?;
    if fields.next().is_some()
        || !(0..=LOGICAL_WIDTH).contains(&x)
        || !(0..=LOGICAL_HEIGHT).contains(&y)
    {
        return None;
    }
    Some(Point { x, y })
}

fn nearest_screen_location(point: Point) -> ScreenLocation {
    use ScreenLocation::*;
    if squared_distance(point, Home.point()) <= 100_i64.pow(2) {
        return Home;
    }
    [
        UpperLeft, Up, UpperRight, Left, Center, Right, LowerLeft, Down, LowerRight,
    ]
    .into_iter()
    .min_by_key(|location| squared_distance(point, location.point()))
    .unwrap_or(Center)
}

#[derive(Clone, Copy, Debug)]
struct Motion {
    from: Point,
    to: Point,
    edge_from: Waypoint,
    edge_to: Waypoint,
    started_ms: u64,
    duration_ms: u64,
    state: &'static str,
}

impl Motion {
    fn point_at(self, now_ms: u64) -> Point {
        if self.duration_ms == 0 {
            return self.to;
        }
        let linear = now_ms.saturating_sub(self.started_ms).min(self.duration_ms) as f64
            / self.duration_ms as f64;
        let smooth = linear * linear * (3.0 - 2.0 * linear);
        Point {
            x: (f64::from(self.from.x) + f64::from(self.to.x - self.from.x) * smooth).round()
                as i32,
            y: (f64::from(self.from.y) + f64::from(self.to.y - self.from.y) * smooth).round()
                as i32,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum AmbientPhase {
    Roam(Motion),
    MicroIdle { state: &'static str, until_ms: u64 },
}

#[derive(Clone, Copy, Debug)]
struct ActiveEvent {
    record: EventRecord,
    deadline_ms: u64,
}

#[derive(Clone, Copy, Debug)]
struct Transition {
    state: &'static str,
    priority: u8,
    until_ms: u64,
    follow_enter_mask: u8,
}

#[derive(Clone, Debug)]
struct EngineInput {
    now_ms: u64,
    now_unix_ms: u64,
    mask: u8,
    codex_active: bool,
    claude_active: bool,
    events: Vec<EventRecord>,
    thermal_sample: Option<Temperatures>,
    settings: ControlSettings,
    controls: Vec<ControlCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EngineOutput {
    state: &'static str,
    position: Option<PositionCommand>,
}

struct StateEngine {
    mask: u8,
    active_events: Vec<ActiveEvent>,
    transition: Option<Transition>,
    thermal: ThermalTracker,
    relief_until_ms: u64,
    rng: XorShift64,
    ambient: Option<AmbientPhase>,
    ambient_eligible: bool,
    next_ambient_ms: u64,
    last_micro_index: Option<usize>,
    position: Point,
    anchor: Waypoint,
    interrupted_edge: Option<(Waypoint, Waypoint)>,
    manual_location: Option<ScreenLocation>,
    manual_motion: Option<Motion>,
    preview: Option<(PreviewKind, u64)>,
    route_cursor: usize,
    facing_right: bool,
    position_seq: u64,
    initial_position_pending: bool,
    last_step_ms: u64,
}

impl StateEngine {
    fn new(seed: u64, now_ms: u64, position_seq: u64) -> Self {
        let mut engine = Self {
            mask: 0,
            active_events: Vec::new(),
            transition: None,
            thermal: ThermalTracker::default(),
            relief_until_ms: 0,
            rng: XorShift64::new(seed),
            ambient: None,
            ambient_eligible: false,
            next_ambient_ms: now_ms,
            last_micro_index: None,
            position: Waypoint::Home.point(),
            anchor: Waypoint::Home,
            interrupted_edge: None,
            manual_location: None,
            manual_motion: None,
            preview: None,
            route_cursor: 0,
            facing_right: true,
            position_seq,
            initial_position_pending: true,
            last_step_ms: now_ms,
        };
        engine.schedule_ambient(now_ms);
        engine
    }

    fn schedule_ambient(&mut self, now_ms: u64) {
        self.next_ambient_ms = now_ms + self.rng.range_inclusive(20_000, 45_000);
    }

    fn issue_position(&mut self, point: Point, duration_ms: u64) -> PositionCommand {
        self.position_seq = self.position_seq.wrapping_add(1);
        if self.position_seq == 0 {
            self.position_seq = 1;
        }
        PositionCommand {
            seq: self.position_seq,
            point,
            duration_ms,
        }
    }

    fn cancel_ambient(&mut self, now_ms: u64) -> Option<PositionCommand> {
        let command = match self.ambient.take() {
            Some(AmbientPhase::Roam(motion)) => {
                let point = motion.point_at(now_ms);
                self.position = point;
                if squared_distance(point, motion.to) <= 1 {
                    self.anchor = motion.edge_to;
                    self.interrupted_edge = None;
                    None
                } else {
                    self.interrupted_edge = Some((motion.edge_from, motion.edge_to));
                    Some(self.issue_position(point, 0))
                }
            }
            _ => None,
        };
        self.ambient_eligible = false;
        command
    }

    fn ingest_events(&mut self, input: &EngineInput) {
        self.active_events
            .retain(|event| event.deadline_ms > input.now_ms);
        for record in &input.events {
            let Some(remaining_ms) = record.remaining_ms(input.now_unix_ms) else {
                continue;
            };
            self.active_events.retain(|event| {
                event.record.actor != record.actor || event.record.kind != record.kind
            });
            self.active_events.push(ActiveEvent {
                record: *record,
                deadline_ms: input.now_ms.saturating_add(remaining_ms),
            });
        }
    }

    fn best_event(&self) -> Option<ActiveEvent> {
        self.active_events.iter().copied().max_by_key(|event| {
            (
                event.record.kind.priority(),
                event.record.created_ms,
                event.record.actor.mask(),
            )
        })
    }

    fn begin_enter(&mut self, mask: u8, now_ms: u64) {
        self.transition = Some(Transition {
            state: enter_state(mask),
            priority: 190,
            until_ms: now_ms.saturating_add(ENTER_DURATION_MS),
            follow_enter_mask: 0,
        });
    }

    fn handle_presence_change(&mut self, new_mask: u8, now_ms: u64) -> Option<PositionCommand> {
        if new_mask == self.mask {
            return None;
        }
        let old_mask = self.mask;
        let removed = old_mask & !new_mask;
        let added = new_mask & !old_mask;
        self.mask = new_mask;
        let position = self.cancel_ambient(now_ms);

        if removed != 0 {
            self.active_events.retain(|event| {
                !(event.record.kind == EventKind::Exit && event.record.actor.mask() & removed != 0)
            });
            let replacement = old_mask & new_mask == 0 && new_mask != 0;
            self.transition = Some(Transition {
                state: exit_state(removed),
                priority: 200,
                until_ms: now_ms.saturating_add(EXIT_DURATION_MS),
                follow_enter_mask: if replacement { new_mask } else { 0 },
            });
        } else if added != 0 {
            self.active_events.retain(|event| {
                !(event.record.kind == EventKind::Start && event.record.actor.mask() & added != 0)
            });
            self.begin_enter(new_mask, now_ms);
        } else {
            self.transition = None;
        }
        position
    }

    fn finish_transition(&mut self, now_ms: u64) -> Option<PositionCommand> {
        let Some(transition) = self
            .transition
            .filter(|transition| transition.until_ms <= now_ms)
        else {
            return None;
        };
        self.transition = None;
        if transition.follow_enter_mask != 0 && self.mask == transition.follow_enter_mask {
            self.begin_enter(transition.follow_enter_mask, now_ms);
        }
        None
    }

    fn pause_preempted_transition(&mut self, now_ms: u64) {
        let elapsed_ms = now_ms.saturating_sub(self.last_step_ms);
        self.last_step_ms = now_ms;
        let Some(transition) = self.transition else {
            return;
        };
        let event_priority = self
            .best_event()
            .map(|event| event.record.kind.priority())
            .unwrap_or(0);
        let thermal_priority = match self.thermal.level() {
            ThermalLevel::Normal if now_ms < self.relief_until_ms => 180,
            ThermalLevel::Normal => 0,
            ThermalLevel::Warm => 180,
            ThermalLevel::Hot => 210,
            ThermalLevel::Critical => 255,
        };
        if event_priority.max(thermal_priority) > transition.priority {
            if let Some(transition) = self.transition.as_mut() {
                transition.until_ms = transition.until_ms.saturating_add(elapsed_ms);
            }
        }
    }

    fn update_thermal(&mut self, sample: Option<Temperatures>, now_ms: u64) {
        let Some(sample) = sample else {
            return;
        };
        let (previous, current, data_loss) = self.thermal.update(sample);
        if data_loss {
            self.relief_until_ms = 0;
        } else if current > previous {
            self.relief_until_ms = 0;
        } else if previous > current
            && (previous >= ThermalLevel::Hot || current == ThermalLevel::Normal)
        {
            self.relief_until_ms = now_ms.saturating_add(RELIEF_DURATION_MS);
        } else if current >= ThermalLevel::Hot {
            self.relief_until_ms = 0;
        }
    }

    fn movement_state_from(&mut self, from: Point, target: Point) -> &'static str {
        if target.x > from.x {
            self.facing_right = true;
            "stroll-right"
        } else if target.x < from.x {
            self.facing_right = false;
            "stroll-left"
        } else if self.facing_right {
            "stroll-right"
        } else {
            "stroll-left"
        }
    }

    fn movement_state_toward(&mut self, target: Point) -> &'static str {
        let from = self.position;
        self.movement_state_from(from, target)
    }

    /// Where the mascot visually is right now: mid-flight manual moves are
    /// interpolated exactly like the renderer interpolates them, so a
    /// retarget starts from the on-screen point rather than the old target.
    fn current_point(&self, now_ms: u64) -> Point {
        match self.manual_motion {
            Some(motion) if now_ms < motion.started_ms.saturating_add(motion.duration_ms) => {
                motion.point_at(now_ms)
            }
            _ => self.position,
        }
    }

    fn roam_route(pattern: RoamPattern) -> &'static [Waypoint] {
        use ScreenLocation::*;
        match pattern {
            // Start with an unmistakable full-width horizontal leg, then
            // deliberately visit both axes instead of relying on a random
            // center spine.
            RoamPattern::Full => &[
                Left, Right, Up, Down, UpperLeft, LowerRight, UpperRight, LowerLeft, Center,
            ],
            RoamPattern::Horizontal => &[Left, Right],
            RoamPattern::Vertical => &[Up, Down],
        }
    }

    fn next_roam_target(&mut self, pattern: RoamPattern) -> Waypoint {
        let route = Self::roam_route(pattern);
        for _ in 0..route.len() {
            let target = route[self.route_cursor % route.len()];
            self.route_cursor = self.route_cursor.wrapping_add(1);
            if target != self.anchor {
                return target;
            }
        }
        ScreenLocation::Center
    }

    fn begin_roam(&mut self, now_ms: u64, settings: ControlSettings) -> PositionCommand {
        let (edge_from, edge_to) = if let Some((left, right)) = self.interrupted_edge.take() {
            let left_distance = squared_distance(self.position, left.point());
            let right_distance = squared_distance(self.position, right.point());
            if left_distance <= right_distance {
                (right, left)
            } else {
                (left, right)
            }
        } else {
            let target = self.next_roam_target(settings.pattern);
            (self.anchor, target)
        };
        let target = edge_to.point();
        let duration_ms = movement_duration_for_pace(self.position, target, settings.pace);
        let state = self.movement_state_toward(target);
        let motion = Motion {
            from: self.position,
            to: target,
            edge_from,
            edge_to,
            started_ms: now_ms,
            duration_ms,
            state,
        };
        self.ambient = Some(AmbientPhase::Roam(motion));
        self.issue_position(target, duration_ms)
    }

    fn move_manual(
        &mut self,
        location: ScreenLocation,
        now_ms: u64,
        pace: RoamPace,
    ) -> PositionCommand {
        let _ = self.cancel_ambient(now_ms);
        // Retargeting mid-flight must pace the new leg from the on-screen
        // interpolated point, not from the previous destination.
        let from = self.current_point(now_ms);
        let target = location.point();
        let duration_ms = movement_duration_for_pace(from, target, pace);
        let state = self.movement_state_from(from, target);
        self.manual_motion = Some(Motion {
            from,
            to: target,
            edge_from: self.anchor,
            edge_to: location,
            started_ms: now_ms,
            duration_ms,
            state,
        });
        self.position = target;
        self.anchor = location;
        self.interrupted_edge = None;
        self.manual_location = Some(location);
        self.issue_position(target, duration_ms)
    }

    fn apply_control(
        &mut self,
        command: ControlCommand,
        now_ms: u64,
        settings: ControlSettings,
    ) -> Option<PositionCommand> {
        match command {
            ControlCommand::Auto => {
                // Auto releases manual mode but keeps any in-flight motion:
                // the renderer finishes that walk, and a quick follow-up
                // move must still retarget from the interpolated point.
                self.manual_location = None;
                self.schedule_ambient(now_ms);
                None
            }
            ControlCommand::Home => {
                Some(self.move_manual(ScreenLocation::Home, now_ms, settings.pace))
            }
            ControlCommand::Move(location) => {
                Some(self.move_manual(location, now_ms, settings.pace))
            }
            ControlCommand::Preview(kind) => {
                let position = self.cancel_ambient(now_ms);
                self.preview = Some((kind, now_ms.saturating_add(2_200)));
                position
            }
        }
    }

    fn control_status(&self) -> (&'static str, ScreenLocation) {
        let mode = if self.manual_location.is_some() {
            "manual"
        } else {
            "auto"
        };
        let location = self.manual_location.unwrap_or_else(|| match self.ambient {
            Some(AmbientPhase::Roam(motion)) => motion.edge_to,
            _ => self.anchor,
        });
        (mode, location)
    }

    fn random_micro_idle(&mut self, now_ms: u64) -> AmbientPhase {
        const STATES: [&str; 6] = [
            "idle-blink",
            "idle-hat-adjust",
            "idle-stretch",
            "idle-mug",
            "idle-gauge",
            "idle-breeze",
        ];
        let mut index = (self.rng.next() as usize) % STATES.len();
        if self.last_micro_index == Some(index) {
            index = (index + 1) % STATES.len();
        }
        self.last_micro_index = Some(index);
        AmbientPhase::MicroIdle {
            state: STATES[index],
            until_ms: now_ms + self.rng.range_inclusive(1_000, 3_000),
        }
    }

    fn advance_ambient(
        &mut self,
        now_ms: u64,
        settings: ControlSettings,
        position: &mut Option<PositionCommand>,
    ) {
        match self.ambient {
            Some(AmbientPhase::Roam(motion))
                if now_ms >= motion.started_ms.saturating_add(motion.duration_ms) =>
            {
                self.position = motion.to;
                self.anchor = motion.edge_to;
                self.interrupted_edge = None;
                if settings.micro_idles {
                    self.ambient = Some(self.random_micro_idle(now_ms));
                } else {
                    self.ambient = None;
                    self.schedule_ambient(now_ms);
                }
            }
            Some(AmbientPhase::MicroIdle { until_ms, .. }) if now_ms >= until_ms => {
                self.ambient = None;
                self.schedule_ambient(now_ms);
            }
            None if now_ms >= self.next_ambient_ms => {
                *position = Some(self.begin_roam(now_ms, settings));
            }
            _ => {}
        }
    }

    fn step(&mut self, input: EngineInput) -> EngineOutput {
        let mut position = if self.initial_position_pending {
            self.initial_position_pending = false;
            Some(self.issue_position(Waypoint::Home.point(), 0))
        } else {
            None
        };

        for command in &input.controls {
            position = self
                .apply_control(*command, input.now_ms, input.settings)
                .or(position);
        }
        if self
            .preview
            .is_some_and(|(_, until_ms)| until_ms <= input.now_ms)
        {
            self.preview = None;
        }

        if input.settings.activity_reactions {
            self.ingest_events(&input);
        } else {
            self.mask = input.mask;
            self.active_events.clear();
            self.transition = None;
        }
        self.update_thermal(input.thermal_sample, input.now_ms);
        if input.settings.activity_reactions {
            self.pause_preempted_transition(input.now_ms);
            position = self.finish_transition(input.now_ms).or(position);
            position = self
                .handle_presence_change(input.mask, input.now_ms)
                .or(position);
        }

        let thermal_level = self.thermal.level();
        let relief_active = input.settings.thermal_reactions
            && thermal_level <= ThermalLevel::Warm
            && input.now_ms < self.relief_until_ms;
        let activity_busy = input.settings.activity_reactions
            && (input.codex_active
                || input.claude_active
                || !self.active_events.is_empty()
                || self.transition.is_some());
        let thermal_busy = input.settings.thermal_reactions
            && (thermal_level != ThermalLevel::Normal || relief_active);
        let preview_active = self.preview.is_some();
        let ambient_allowed = input.settings.roam_enabled
            && self.manual_location.is_none()
            && !activity_busy
            && !thermal_busy
            && !preview_active;

        if !input.settings.micro_idles
            && matches!(self.ambient, Some(AmbientPhase::MicroIdle { .. }))
        {
            self.ambient = None;
            self.schedule_ambient(input.now_ms);
        }

        if !ambient_allowed {
            position = self.cancel_ambient(input.now_ms).or(position);
        } else {
            if !self.ambient_eligible {
                self.ambient_eligible = true;
                self.schedule_ambient(input.now_ms);
            }
            self.advance_ambient(input.now_ms, input.settings, &mut position);
        }

        let reactions_active = input.settings.activity_reactions;
        let mut selected = (
            if reactions_active && (input.codex_active || input.claude_active) {
                150
            } else {
                100
            },
            if reactions_active {
                base_state_name(input.mask, input.codex_active, input.claude_active)
            } else {
                "idle"
            },
        );
        let mut consider = |priority: u8, state: &'static str| {
            if priority > selected.0 {
                selected = (priority, state);
            }
        };

        if let Some(ambient) = self.ambient {
            match ambient {
                AmbientPhase::Roam(motion) => consider(120, motion.state),
                AmbientPhase::MicroIdle { state, .. } => consider(115, state),
            }
        }
        if let Some(motion) = self.manual_motion {
            if input.now_ms < motion.started_ms.saturating_add(motion.duration_ms) {
                consider(165, motion.state);
            }
        }
        if input.settings.thermal_reactions {
            if relief_active {
                consider(180, "thermal-relief");
            }
            match thermal_level {
                ThermalLevel::Normal => {}
                ThermalLevel::Warm => consider(180, "thermal-warm"),
                ThermalLevel::Hot => consider(210, "thermal-hot"),
                ThermalLevel::Critical => consider(255, "thermal-critical"),
            }
        }
        if reactions_active {
            if let Some(transition) = self.transition {
                consider(transition.priority, transition.state);
            }
            if let Some(event) = self.best_event() {
                consider(
                    event.record.kind.priority(),
                    event.record.kind.state(event.record.actor, input.mask),
                );
            }
        }
        if let Some((preview, _)) = self.preview {
            consider(240, preview.state());
        }

        EngineOutput {
            state: selected.1,
            position,
        }
    }
}

fn write_atomic(path: &Path, value: &str) -> io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("lianli-agent-output");
    for attempt in 0..16u8 {
        let temporary =
            path.with_file_name(format!(".{file_name}.{}.{}.tmp", process::id(), attempt));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let result = file
            .set_permissions(fs::Permissions::from_mode(0o600))
            .and_then(|_| file.write_all(value.as_bytes()))
            .and_then(|_| file.flush());
        drop(file);
        if let Err(error) = result {
            let _ = fs::remove_file(temporary);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(temporary);
            return Err(error);
        }
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a temporary output file",
    ))
}

fn write_state(path: &Path, state: &str) -> io::Result<()> {
    write_atomic(path, &format!("{state}\n"))
}

fn write_position(path: &Path, command: PositionCommand) -> io::Result<()> {
    write_atomic(path, &command.line())
}

fn write_control_status(path: &Path, mode: &str, location: ScreenLocation) -> io::Result<()> {
    write_atomic(path, &format!("v1 {mode} {}\n", location.as_str()))
}

struct InstanceLock {
    path: PathBuf,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn watcher_pid_is_alive(pid: u32) -> bool {
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    if comm.trim().starts_with("lianli-agent-w") {
        return true;
    }
    fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .is_some_and(|name| name.starts_with("lianli-agent-watch"))
}

/// Remove work files abandoned by crashed processes: claim files whose
/// watcher pid is gone, and writer temporaries older than a minute. Runs
/// once at startup, after the instance lock guarantees no live sibling.
fn clean_stale_work_files(runtime_dir: &Path) {
    let Ok(entries) = fs::read_dir(runtime_dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !name.starts_with(".lianli-agent-") {
            continue;
        }
        let stale = if let Some((_, pid)) = name.rsplit_once(".watch-") {
            pid.parse::<u32>().is_ok_and(|pid| !watcher_pid_is_alive(pid))
        } else if name.ends_with(".tmp") {
            entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age.as_secs() >= 60)
        } else {
            false
        };
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn acquire_instance_lock(runtime_dir: &Path) -> io::Result<InstanceLock> {
    let path = runtime_dir.join(".lianli-agent-watch.lock");
    for _ in 0..2 {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                writeln!(file, "{}", process::id())?;
                return Ok(InstanceLock { path });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let existing_pid = fs::read_to_string(&path)
                    .ok()
                    .and_then(|value| value.trim().parse::<u32>().ok());
                if existing_pid.is_some_and(watcher_pid_is_alive) {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "another lianli-agent-watch process is already running",
                    ));
                }
                let _ = fs::remove_file(&path);
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not acquire lianli-agent-watch lock",
    ))
}

fn main() -> io::Result<()> {
    let uid = effective_uid()?;
    let runtime_dir = runtime_dir(uid)?;
    let state_path = runtime_dir.join("lianli-agent-state");
    let position_path = runtime_dir.join("lianli-agent-position");
    let control_status_path = runtime_dir.join("lianli-agent-control-status");
    let control_config_path = control_config_path();
    let _instance_lock = acquire_instance_lock(&runtime_dir)?;
    clean_stale_work_files(&runtime_dir);

    let mut previous_processes: HashMap<ProcKey, Counters> = HashMap::new();
    let mut cgroup_trackers: HashMap<String, CgroupTracker> = HashMap::new();
    let mut codex_presence = Presence::default();
    let mut claude_presence = Presence::default();
    let mut codex_activity = ActivityDetector::default();
    let mut claude_activity = ActivityDetector::default();
    let mut last_state = String::new();
    let mut last_control_status = String::new();
    let mut initialized = false;
    let started = Instant::now();
    let initial_unix_ms = unix_time_ms()?;
    let initial_seq = read_position_seq(&position_path, initial_unix_ms);
    let mut control_settings = read_control_settings(control_config_path.as_deref(), uid);
    let seed =
        initial_unix_ms ^ u64::from(process::id()).rotate_left(17) ^ u64::from(uid).rotate_left(33);
    let mut engine = StateEngine::new(seed, 0, initial_seq);
    if control_settings.manual_mode {
        if let Some(point) = read_position_point(&position_path) {
            let location = nearest_screen_location(point);
            engine.position = point;
            engine.anchor = location;
            engine.manual_location = Some(location);
            engine.initial_position_pending = false;
        } else {
            engine.manual_location = Some(ScreenLocation::Home);
        }
    }
    let mut mask = 0u8;
    let mut next_process_sample = Instant::now();
    let mut next_thermal_sample = Instant::now();
    let mut next_control_sample = Instant::now();
    let mut last_codex_signals = Signals::default();
    let mut last_claude_signals = Signals::default();

    loop {
        let cycle_started = Instant::now();
        if cycle_started >= next_process_sample {
            next_process_sample = cycle_started + PROCESS_INTERVAL;
            let processes = scan_processes(uid);
            let by_pid: HashMap<u32, &ProcSample> = processes
                .iter()
                .map(|process| (process.key.pid, process))
                .collect();
            let roots: HashMap<u32, u8> = processes
                .iter()
                .filter_map(|process| {
                    let mask = root_mask(&process.comm);
                    (mask != 0).then_some((process.key.pid, mask))
                })
                .collect();

            let observed_mask = roots.values().fold(0u8, |mask, value| mask | value);
            codex_presence.update(observed_mask & CODEX != 0);
            claude_presence.update(observed_mask & CLAUDE != 0);
            mask = presence_mask(codex_presence, claude_presence);

            let mut codex_signals = Signals::default();
            let mut claude_signals = Signals::default();
            let mut current_processes = HashMap::new();
            for process in &processes {
                let owner = classify_process(process.key.pid, &by_pid, &roots);
                if owner == 0 {
                    continue;
                }
                let io_counters = read_io_counters(process.key.pid);
                let counters = Counters {
                    cpu_ticks: process.counters.cpu_ticks,
                    chars: io_counters.chars,
                    storage_bytes: io_counters.storage_bytes,
                };
                if let Some(previous) = previous_processes.get(&process.key) {
                    if owner == CODEX {
                        codex_signals.add_process_delta(counters, *previous);
                    } else if owner == CLAUDE {
                        claude_signals.add_process_delta(counters, *previous);
                    }
                } else if initialized {
                    if owner == CODEX {
                        codex_signals.spawned = true;
                    } else if owner == CLAUDE {
                        claude_signals.spawned = true;
                    }
                }
                current_processes.insert(process.key, counters);
            }

            // Cgroup counters preserve work from short-lived children between
            // scans. Attribute only scopes that belong to one agent.
            let mut cgroup_owners: HashMap<String, u8> = HashMap::new();
            for (pid, owner) in &roots {
                if let Some(path) = read_cgroup_path(*pid).filter(|path| trusted_agent_cgroup(path))
                {
                    cgroup_owners
                        .entry(path)
                        .and_modify(|mask| *mask |= *owner)
                        .or_insert(*owner);
                }
            }
            let mut current_cgroups = HashSet::new();
            for (path, owner) in cgroup_owners {
                let Some(usage) = read_cgroup_cpu_usec(&path) else {
                    continue;
                };
                let (delta, hot, warm) = if let Some(tracker) = cgroup_trackers.get_mut(&path) {
                    tracker.observe(usage)
                } else {
                    cgroup_trackers.insert(path.clone(), CgroupTracker::new(usage));
                    (0, false, false)
                };
                if owner == CODEX {
                    codex_signals.cgroup_cpu_usec =
                        codex_signals.cgroup_cpu_usec.saturating_add(delta);
                    codex_signals.cgroup_hot |= hot;
                    codex_signals.cgroup_warm |= warm;
                } else if owner == CLAUDE {
                    claude_signals.cgroup_cpu_usec =
                        claude_signals.cgroup_cpu_usec.saturating_add(delta);
                    claude_signals.cgroup_hot |= hot;
                    claude_signals.cgroup_warm |= warm;
                }
                current_cgroups.insert(path);
            }
            cgroup_trackers.retain(|path, _| current_cgroups.contains(path));

            codex_activity.update(codex_presence.present, codex_signals);
            claude_activity.update(claude_presence.present, claude_signals);
            previous_processes = current_processes;
            last_codex_signals = codex_signals;
            last_claude_signals = claude_signals;
            initialized = true;
        }

        let now_unix_ms = unix_time_ms()?;
        let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        if cycle_started >= next_control_sample {
            next_control_sample = cycle_started + CONTROL_INTERVAL;
            control_settings = read_control_settings(control_config_path.as_deref(), uid);
        }
        let controls = consume_control_commands(&runtime_dir, uid, now_unix_ms);
        let mut events = Vec::with_capacity(2);
        for actor in [AgentActor::Codex, AgentActor::Claude] {
            events.extend(consume_agent_events(&runtime_dir, actor, uid, now_unix_ms));
        }
        let thermal_sample = if cycle_started >= next_thermal_sample {
            next_thermal_sample = cycle_started + THERMAL_INTERVAL;
            Some(read_temperatures(&runtime_dir, uid))
        } else {
            None
        };

        let output = engine.step(EngineInput {
            now_ms,
            now_unix_ms,
            mask,
            codex_active: codex_activity.active,
            claude_active: claude_activity.active,
            events,
            thermal_sample,
            settings: control_settings,
            controls,
        });
        if let Some(command) = output.position {
            write_position(&position_path, command)?;
        }
        let (mode, location) = engine.control_status();
        let control_status = format!("v1 {mode} {}", location.as_str());
        if control_status != last_control_status {
            write_control_status(&control_status_path, mode, location)?;
            last_control_status = control_status;
        }
        if output.state != last_state {
            write_state(&state_path, output.state)?;
            eprintln!(
                "Patch state -> {} | codex cpu={}t chars={}KiB disk={}KiB cgroup={}ms | claude cpu={}t chars={}KiB disk={}KiB cgroup={}ms",
                output.state,
                last_codex_signals.cpu_ticks,
                last_codex_signals.chars / 1024,
                last_codex_signals.storage_bytes / 1024,
                last_codex_signals.cgroup_cpu_usec / 1000,
                last_claude_signals.cpu_ticks,
                last_claude_signals.chars / 1024,
                last_claude_signals.storage_bytes / 1024,
                last_claude_signals.cgroup_cpu_usec / 1000,
            );
            last_state.clear();
            last_state.push_str(output.state);
        }

        let elapsed = cycle_started.elapsed();
        if elapsed < TICK_INTERVAL {
            thread::sleep(TICK_INTERVAL - elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(actor: AgentActor, kind: EventKind, created_ms: u64, ttl_ms: u64) -> EventRecord {
        EventRecord {
            actor,
            kind,
            created_ms,
            ttl_ms,
        }
    }

    fn input(now_ms: u64, mask: u8) -> EngineInput {
        EngineInput {
            now_ms,
            now_unix_ms: 1_000_000 + now_ms,
            mask,
            codex_active: false,
            claude_active: false,
            events: Vec::new(),
            thermal_sample: None,
            settings: ControlSettings::default(),
            controls: Vec::new(),
        }
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "lianli-agent-watch-{label}-{}-{nonce}",
            process::id()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn stat_parser_handles_spaces_and_parentheses_in_comm() {
        let stat = "42 (a tricky ) name) S 7 0 0 0 0 0 0 0 0 0 11 13 0 0 0 0 0 0 1234 0";
        let parsed = parse_stat(42, stat, "a tricky ) name".into(), Counters::default()).unwrap();
        assert_eq!(parsed.ppid, 7);
        assert_eq!(parsed.counters.cpu_ticks, 24);
        assert_eq!(parsed.key.start_time, 1234);
    }

    #[test]
    fn activity_uses_hot_then_four_cold_samples() {
        let mut detector = ActivityDetector::default();
        detector.update(
            true,
            Signals {
                chars: 128 * 1024,
                ..Signals::default()
            },
        );
        assert!(detector.active);
        for _ in 0..3 {
            detector.update(true, Signals::default());
            assert!(detector.active);
        }
        detector.update(true, Signals::default());
        assert!(!detector.active);
    }

    #[test]
    fn presence_requires_two_misses_before_exit() {
        let mut presence = Presence::default();
        presence.update(true);
        assert!(presence.present);
        presence.update(false);
        assert!(presence.present);
        presence.update(true);
        assert!(presence.present);
        assert_eq!(presence.misses, 0);
        presence.update(false);
        presence.update(false);
        assert!(!presence.present);
    }

    #[test]
    fn cgroup_bootstrap_learns_a_busy_idle_scope_without_warm_lock() {
        let mut tracker = CgroupTracker::new(0);
        for usage in [40_000, 80_000, 120_000] {
            let (_, hot, warm) = tracker.observe(usage);
            assert!(!hot);
            assert!(!warm);
        }
        let (_, hot, warm) = tracker.observe(160_000);
        assert!(!hot);
        assert!(!warm);
    }

    #[test]
    fn only_isolated_application_scopes_are_trusted() {
        assert!(trusted_agent_cgroup(
            "/user.slice/user-1000.slice/user@1000.service/app.slice/app-graphical.slice/app-terminal.scope"
        ));
        assert!(!trusted_agent_cgroup("/"));
        assert!(!trusted_agent_cgroup(
            "/user.slice/user-1000.slice/user@1000.service"
        ));
        assert!(!trusted_agent_cgroup(
            "/user.slice/user-1000.slice/session-2.scope"
        ));
    }

    #[test]
    fn state_names_cover_presence_activity_and_transitions() {
        assert_eq!(base_state_name(0, false, false), "idle");
        assert_eq!(enter_state(CODEX), "codex-enter");
        assert_eq!(exit_state(CLAUDE), "claude-exit");
        assert_eq!(base_state_name(CODEX, true, false), "codex-active");
        assert_eq!(base_state_name(CLAUDE, false, false), "claude-wait");
        assert_eq!(base_state_name(CODEX | CLAUDE, false, true), "both-active");
    }

    #[test]
    fn event_parser_accepts_exact_v1_contract_and_bounds() {
        let now = 10_000;
        for kind in [
            "start",
            "exit",
            "tool",
            "read",
            "permission",
            "error",
            "success",
        ] {
            let value = format!("v1 9999 {kind} 250\n");
            assert!(parse_event_record(AgentActor::Codex, value.as_bytes(), now).is_some());
        }
        let maximum = b"v1 10000 success 60000\n";
        let parsed = parse_event_record(AgentActor::Claude, maximum, now).unwrap();
        assert_eq!(parsed.actor, AgentActor::Claude);
        assert_eq!(parsed.kind, EventKind::Success);
        assert_eq!(parsed.ttl_ms, 60_000);
    }

    #[test]
    fn event_parser_rejects_malformed_expired_and_untrusted_time_records() {
        let now = 10_000;
        for value in [
            "v2 9999 tool 1500",
            "v1 9999 nope 1500",
            "v1 9999 tool 249",
            "v1 9999 tool 60001",
            "v1 9000 tool 1000",
            "v1 12001 tool 1500",
            "v1 9999 tool 1500 extra",
            "v1 nope tool 1500",
        ] {
            assert!(
                parse_event_record(AgentActor::Codex, value.as_bytes(), now).is_none(),
                "{value}"
            );
        }
        assert!(parse_event_record(AgentActor::Codex, &[0xff, 0xfe], now).is_none());
        assert!(parse_event_record(
            AgentActor::Codex,
            b"v1 18446744073709551500 tool 250",
            u64::MAX - 100
        )
        .is_none());
        let oversized = vec![b'x'; MAX_EVENT_RECORD_BYTES as usize + 1];
        assert!(parse_event_record(AgentActor::Codex, &oversized, now).is_none());
    }

    #[test]
    fn control_settings_are_strict_owned_and_default_safe() {
        let parsed = parse_control_settings(
            r#"{
                "mode": "manual",
                "roamEnabled": false,
                "roam_pattern": "horizontal",
                "pace": "quick",
                "microIdles": false,
                "activityReactions": false,
                "thermalReactions": true
            }"#,
        );
        assert_eq!(
            parsed,
            ControlSettings {
                manual_mode: true,
                roam_enabled: false,
                pattern: RoamPattern::Horizontal,
                pace: RoamPace::Quick,
                micro_idles: false,
                activity_reactions: false,
                thermal_reactions: true,
            }
        );
        let invalid = parse_control_settings(r#"{"pattern":"diagonal","pace":"warp"}"#);
        assert_eq!(invalid.pattern, RoamPattern::Full);
        assert_eq!(invalid.pace, RoamPace::Normal);

        let directory = temporary_directory("control-settings");
        let path = directory.join("mascot-control.json");
        fs::write(&path, r#"{"roamEnabled":false}"#).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!read_control_settings(Some(&path), effective_uid().unwrap()).roam_enabled);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_control_settings(Some(&path), effective_uid().unwrap()).roam_enabled);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn control_command_contract_rejects_stale_and_unknown_records() {
        let now = 50_000;
        assert_eq!(
            parse_control_record(b"v1 49999 move lower-right\n", now),
            Some((49_999, ControlCommand::Move(ScreenLocation::LowerRight)))
        );
        assert_eq!(
            parse_control_record(b"v1 50000 preview success\n", now),
            Some((50_000, ControlCommand::Preview(PreviewKind::Success)))
        );
        assert_eq!(
            parse_control_record(b"v1 50000 auto -\n", now),
            Some((50_000, ControlCommand::Auto))
        );
        for invalid in [
            "v2 50000 move left",
            "v1 34999 move left",
            "v1 52001 move left",
            "v1 50000 move nowhere",
            "v1 50000 preview start",
            "v1 50000 home extra",
            "v1 50000 auto",
        ] {
            assert!(
                parse_control_record(invalid.as_bytes(), now).is_none(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn manual_moves_hold_position_react_in_place_and_auto_releases() {
        let mut engine = StateEngine::new(31, 0, 900);
        engine.step(input(0, 0));

        let mut moving = input(100, 0);
        moving.controls = vec![ControlCommand::Move(ScreenLocation::Right)];
        let output = engine.step(moving);
        assert_eq!(
            output.position.unwrap().point,
            ScreenLocation::Right.point()
        );
        assert_eq!(output.state, "stroll-right");
        assert_eq!(engine.control_status(), ("manual", ScreenLocation::Right));

        let mut error = input(200, 0);
        error.settings.manual_mode = true;
        error.events.push(event(
            AgentActor::Codex,
            EventKind::Error,
            error.now_unix_ms,
            1_500,
        ));
        assert_eq!(engine.step(error).state, "event-error");
        assert_eq!(engine.control_status(), ("manual", ScreenLocation::Right));

        let mut automatic = input(300, 0);
        automatic.controls = vec![ControlCommand::Auto];
        engine.step(automatic);
        assert_eq!(engine.control_status().0, "auto");
    }

    #[test]
    fn roaming_and_reaction_toggles_take_effect_without_stopping_watcher() {
        let mut no_roam = StateEngine::new(32, 0, 950);
        no_roam.step(input(0, 0));
        no_roam.next_ambient_ms = 100;
        let mut disabled = input(100, 0);
        disabled.settings.roam_enabled = false;
        assert!(no_roam.step(disabled).position.is_none());

        let mut no_activity = StateEngine::new(33, 0, 1_000);
        let mut active = input(0, CODEX);
        active.codex_active = true;
        active.settings.activity_reactions = false;
        assert_eq!(no_activity.step(active).state, "idle");

        let mut vertical = StateEngine::new(34, 0, 1_050);
        vertical.step(input(0, 0));
        vertical.next_ambient_ms = 100;
        let mut vertical_input = input(100, 0);
        vertical_input.settings.pattern = RoamPattern::Vertical;
        let target = vertical.step(vertical_input).position.unwrap().point;
        assert_eq!(target, ScreenLocation::Up.point());
        assert_eq!(target.x, ScreenLocation::Home.point().x);
    }

    #[test]
    fn event_priorities_and_visual_names_are_exact() {
        let ascending = [
            EventKind::Start,
            EventKind::Read,
            EventKind::Tool,
            EventKind::Exit,
            EventKind::Success,
            EventKind::Permission,
            EventKind::Error,
        ];
        assert!(ascending
            .windows(2)
            .all(|pair| pair[0].priority() < pair[1].priority()));
        assert_eq!(
            EventKind::Success.state(AgentActor::Codex, CODEX),
            "event-success"
        );
        assert_eq!(
            EventKind::Exit.state(AgentActor::Claude, CLAUDE),
            "event-done"
        );
        assert_eq!(
            EventKind::Read.state(AgentActor::Codex, CODEX),
            "event-reading"
        );
        assert_eq!(
            EventKind::Start.state(AgentActor::Claude, BOTH),
            "both-enter"
        );
    }

    #[test]
    fn event_claim_does_not_delete_a_newer_canonical_record() {
        let directory = temporary_directory("claim");
        let uid = effective_uid().unwrap();
        let source = directory.join("lianli-agent-event-codex");
        let claim = directory.join(".claimed");
        fs::write(&source, "v1 10000 read 1500\n").unwrap();
        fs::rename(&source, &claim).unwrap();
        fs::write(&source, "v1 10001 tool 1500\n").unwrap();
        let first =
            parse_event_record(AgentActor::Codex, &fs::read(&claim).unwrap(), 10_000).unwrap();
        assert_eq!(first.kind, EventKind::Read);
        fs::remove_file(&claim).unwrap();
        assert_eq!(fs::read_to_string(&source).unwrap(), "v1 10001 tool 1500\n");

        let consumed = consume_agent_events(&directory, AgentActor::Codex, uid, 10_001);
        assert_eq!(consumed.len(), 1);
        assert_eq!(consumed[0].kind, EventKind::Tool);
        assert!(!source.exists());
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn queued_event_bursts_are_delivered_in_order_without_loss() {
        let directory = temporary_directory("event-burst");
        let uid = effective_uid().unwrap();
        // A tool event followed 5 ms later by a permission event, both inside
        // one 100 ms tick: the old single mailbox lost the first record.
        fs::write(
            directory.join(format!("lianli-agent-event-codex.{:020}.41", 10_000)),
            "v1 10000 tool 1500\n",
        )
        .unwrap();
        fs::write(
            directory.join(format!("lianli-agent-event-codex.{:020}.42", 10_005)),
            "v1 10005 permission 10000\n",
        )
        .unwrap();
        // Junk that must be ignored: wrong prefix shape and non-numeric pid.
        fs::write(directory.join("lianli-agent-event-codex.junk"), "x").unwrap();
        fs::write(
            directory.join(format!("lianli-agent-event-claude.{:020}.43", 10_001)),
            "v1 10001 success 8000\n",
        )
        .unwrap();

        let codex = consume_agent_events(&directory, AgentActor::Codex, uid, 10_050);
        assert_eq!(
            codex
                .iter()
                .map(|record| record.kind)
                .collect::<Vec<_>>(),
            vec![EventKind::Tool, EventKind::Permission]
        );
        let claude = consume_agent_events(&directory, AgentActor::Claude, uid, 10_050);
        assert_eq!(claude.len(), 1);
        assert_eq!(claude[0].kind, EventKind::Success);
        // Consumed and junk-non-matching files: queue entries gone, junk kept.
        assert!(directory.join("lianli-agent-event-codex.junk").exists());
        assert!(consume_agent_events(&directory, AgentActor::Codex, uid, 10_050).is_empty());
        fs::remove_file(directory.join("lianli-agent-event-codex.junk")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn queued_control_commands_apply_sequentially_in_one_tick() {
        let directory = temporary_directory("command-burst");
        let uid = effective_uid().unwrap();
        fs::write(
            directory.join(format!("lianli-agent-command.{:020}.9", 20_000)),
            "v1 20000 move upper-left\n",
        )
        .unwrap();
        fs::write(
            directory.join(format!("lianli-agent-command.{:020}.9", 20_001)),
            "v1 20001 preview success\n",
        )
        .unwrap();

        let commands = consume_control_commands(&directory, uid, 20_050);
        assert_eq!(
            commands,
            vec![
                ControlCommand::Move(ScreenLocation::UpperLeft),
                ControlCommand::Preview(PreviewKind::Success),
            ]
        );

        // Both commands take effect in a single engine step: the move sets a
        // manual destination and the preview overrides the visible state.
        let mut engine = StateEngine::new(11, 0, 400);
        engine.step(input(0, 0));
        let mut burst = input(100, 0);
        burst.now_unix_ms = 20_050;
        burst.controls = commands;
        let output = engine.step(burst);
        assert_eq!(output.state, "event-success");
        assert_eq!(
            output.position.unwrap().point,
            ScreenLocation::UpperLeft.point()
        );
        assert_eq!(engine.manual_location, Some(ScreenLocation::UpperLeft));
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn explicit_success_alone_celebrates_and_expiration_falls_back() {
        let mut engine = StateEngine::new(7, 0, 50);
        let startup = engine.step(input(0, 0));
        assert_eq!(startup.state, "idle");
        assert_eq!(startup.position.unwrap().point, Waypoint::Home.point());

        let mut success = input(100, 0);
        success.events.push(event(
            AgentActor::Codex,
            EventKind::Success,
            success.now_unix_ms,
            1_000,
        ));
        assert_eq!(engine.step(success).state, "event-success");
        assert_eq!(engine.step(input(1_099, 0)).state, "event-success");
        assert_eq!(engine.step(input(1_100, 0)).state, "idle");

        let mut active = input(1_200, CODEX);
        active.codex_active = true;
        assert_eq!(engine.step(active).state, "codex-enter");
        let mut waiting = input(3_201, CODEX);
        waiting.codex_active = false;
        assert_eq!(engine.step(waiting).state, "codex-wait");
    }

    #[test]
    fn priority_preemption_and_process_walkoff_are_distinct() {
        let mut engine = StateEngine::new(9, 0, 100);
        let mut start = input(0, CODEX);
        start.events = vec![
            event(
                AgentActor::Codex,
                EventKind::Permission,
                start.now_unix_ms,
                5_000,
            ),
            event(
                AgentActor::Claude,
                EventKind::Error,
                start.now_unix_ms,
                5_000,
            ),
        ];
        assert_eq!(engine.step(start).state, "event-error");

        let mut done_engine = StateEngine::new(11, 0, 200);
        assert_eq!(done_engine.step(input(0, CODEX)).state, "codex-enter");
        let mut done = input(2_100, CODEX);
        done.events.push(event(
            AgentActor::Codex,
            EventKind::Exit,
            done.now_unix_ms,
            6_000,
        ));
        assert_eq!(done_engine.step(done).state, "event-done");
        assert_eq!(done_engine.step(input(2_200, 0)).state, "codex-exit");
        assert_eq!(done_engine.step(input(5_201, 0)).state, "idle");
    }

    #[test]
    fn replacements_walk_off_before_the_new_actor_enters() {
        let mut engine = StateEngine::new(13, 0, 300);
        assert_eq!(engine.step(input(0, CODEX)).state, "codex-enter");
        assert_eq!(engine.step(input(2_001, CODEX)).state, "codex-wait");
        assert_eq!(engine.step(input(2_100, CLAUDE)).state, "codex-exit");
        assert_eq!(engine.step(input(5_101, CLAUDE)).state, "claude-enter");
        assert_eq!(engine.step(input(7_102, CLAUDE)).state, "claude-wait");
    }

    #[test]
    fn higher_priority_events_pause_instead_of_erasing_walkoff() {
        let mut engine = StateEngine::new(14, 0, 350);
        engine.step(input(0, CODEX));
        engine.step(input(2_001, CODEX));

        let mut leaving = input(2_100, 0);
        leaving.events.push(event(
            AgentActor::Codex,
            EventKind::Error,
            leaving.now_unix_ms,
            4_000,
        ));
        assert_eq!(engine.step(leaving).state, "event-error");
        assert_eq!(engine.step(input(6_099, 0)).state, "event-error");
        assert_eq!(engine.step(input(6_100, 0)).state, "codex-exit");
        assert_eq!(engine.step(input(9_101, 0)).state, "idle");
    }

    #[test]
    fn each_sensor_has_independent_hysteresis() {
        let mut tracker = ThermalTracker::default();
        let cpu_warm = Temperatures {
            cpu_c: Some(75.0),
            ..Temperatures::default()
        };
        assert_eq!(tracker.update(cpu_warm).1, ThermalLevel::Normal);
        assert_eq!(tracker.update(cpu_warm).1, ThermalLevel::Warm);

        let cpu_hot = Temperatures {
            cpu_c: Some(85.0),
            ..Temperatures::default()
        };
        assert_eq!(tracker.update(cpu_hot).1, ThermalLevel::Warm);
        assert_eq!(tracker.update(cpu_hot).1, ThermalLevel::Hot);

        let cpu_critical = Temperatures {
            cpu_c: Some(92.0),
            ..Temperatures::default()
        };
        assert_eq!(tracker.update(cpu_critical).1, ThermalLevel::Critical);
        for _ in 0..2 {
            assert_eq!(
                tracker
                    .update(Temperatures {
                        cpu_c: Some(87.9),
                        ..Temperatures::default()
                    })
                    .1,
                ThermalLevel::Critical
            );
        }
        assert_eq!(
            tracker
                .update(Temperatures {
                    cpu_c: Some(87.9),
                    ..Temperatures::default()
                })
                .1,
            ThermalLevel::Hot
        );

        let mut alternating = ThermalTracker::default();
        alternating.update(Temperatures {
            cpu_c: Some(75.0),
            ..Temperatures::default()
        });
        let level = alternating
            .update(Temperatures {
                gpu_c: Some(75.0),
                ..Temperatures::default()
            })
            .1;
        assert_eq!(level, ThermalLevel::Normal);
    }

    #[test]
    fn critical_is_immediate_missing_samples_do_not_clear_and_relief_is_visible() {
        let mut engine = StateEngine::new(15, 0, 400);
        let mut critical = input(0, 0);
        critical.thermal_sample = Some(Temperatures {
            gpu_c: Some(100.0),
            ..Temperatures::default()
        });
        assert_eq!(engine.step(critical).state, "thermal-critical");
        for now in [1_000, 2_000, 3_000] {
            assert_eq!(engine.step(input(now, 0)).state, "thermal-critical");
        }
        for now in (4_000..13_000).step_by(1_000) {
            let mut missing = input(now, 0);
            missing.thermal_sample = Some(Temperatures::default());
            assert_eq!(engine.step(missing).state, "thermal-critical");
        }
        let mut expired = input(13_000, 0);
        expired.thermal_sample = Some(Temperatures::default());
        assert_eq!(engine.step(expired).state, "idle");

        let mut hot_engine = StateEngine::new(17, 0, 500);
        for now in [0, 1_000] {
            let mut hot = input(now, 0);
            hot.thermal_sample = Some(Temperatures {
                cpu_c: Some(85.0),
                ..Temperatures::default()
            });
            let state = hot_engine.step(hot).state;
            if now == 1_000 {
                assert_eq!(state, "thermal-hot");
            }
        }
        for now in [2_000, 3_000] {
            let mut cool = input(now, 0);
            cool.thermal_sample = Some(Temperatures {
                cpu_c: Some(69.0),
                ..Temperatures::default()
            });
            assert_eq!(hot_engine.step(cool).state, "thermal-hot");
        }
        let mut cool = input(4_000, 0);
        cool.thermal_sample = Some(Temperatures {
            cpu_c: Some(69.0),
            ..Temperatures::default()
        });
        assert_eq!(hot_engine.step(cool).state, "thermal-relief");
        assert_eq!(hot_engine.step(input(7_001, 0)).state, "idle");
    }

    #[test]
    fn thermal_and_event_priority_bands_are_enforced() {
        let mut hot_engine = StateEngine::new(19, 0, 600);
        for now in [0, 1_000] {
            let mut hot = input(now, 0);
            hot.thermal_sample = Some(Temperatures {
                cpu_c: Some(85.0),
                ..Temperatures::default()
            });
            if now == 1_000 {
                hot.events.push(event(
                    AgentActor::Codex,
                    EventKind::Success,
                    hot.now_unix_ms,
                    8_000,
                ));
                assert_eq!(hot_engine.step(hot).state, "thermal-hot");
            } else {
                hot_engine.step(hot);
            }
        }
        let mut permission = input(1_100, 0);
        permission.events.push(event(
            AgentActor::Claude,
            EventKind::Permission,
            permission.now_unix_ms,
            5_000,
        ));
        assert_eq!(hot_engine.step(permission).state, "event-attention");

        let mut critical_engine = StateEngine::new(21, 0, 700);
        let mut critical = input(0, 0);
        critical.thermal_sample = Some(Temperatures {
            cpu_c: Some(92.0),
            ..Temperatures::default()
        });
        critical.events.push(event(
            AgentActor::Codex,
            EventKind::Error,
            critical.now_unix_ms,
            10_000,
        ));
        assert_eq!(critical_engine.step(critical).state, "thermal-critical");
    }

    #[test]
    fn exact_sensor_selection_uses_temp1_tctl_and_pci_edge() {
        let directory = temporary_directory("sensors");
        let distractor = directory.join("hwmon0");
        let cpu = directory.join("hwmon1");
        fs::create_dir(&distractor).unwrap();
        fs::create_dir(&cpu).unwrap();
        fs::write(distractor.join("name"), "k10temp\n").unwrap();
        fs::write(distractor.join("temp1_label"), "Tccd1\n").unwrap();
        fs::write(distractor.join("temp1_input"), "99000\n").unwrap();
        fs::write(cpu.join("name"), "k10temp\n").unwrap();
        fs::write(cpu.join("temp1_label"), "Tctl\n").unwrap();
        fs::write(cpu.join("temp1_input"), "66250\n").unwrap();
        assert_eq!(
            read_hwmon_temperature(&directory, "k10temp", "Tctl"),
            Some(66.25)
        );

        fs::write(distractor.join("name"), "amdgpu\n").unwrap();
        fs::write(distractor.join("temp1_label"), "junction\n").unwrap();
        fs::write(cpu.join("name"), "amdgpu\n").unwrap();
        fs::write(cpu.join("temp1_label"), "edge\n").unwrap();
        fs::write(cpu.join("temp1_input"), "65000\n").unwrap();
        assert_eq!(
            read_hwmon_temperature(&directory, "amdgpu", "edge"),
            Some(65.0)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_and_queued_records_merge_in_created_ms_order() {
        let directory = temporary_directory("merge-order");
        let uid = effective_uid().unwrap();
        // A queued move published at 20_000 and a NEWER legacy auto at
        // 20_010: chronological order must put auto last so it wins.
        fs::write(
            directory.join(format!("lianli-agent-command.{:020}.9", 20_000)),
            "v1 20000 move upper-left\n",
        )
        .unwrap();
        fs::write(
            directory.join("lianli-agent-command"),
            "v1 20010 auto -\n",
        )
        .unwrap();
        let commands = consume_control_commands(&directory, uid, 20_050);
        assert_eq!(
            commands,
            vec![
                ControlCommand::Move(ScreenLocation::UpperLeft),
                ControlCommand::Auto,
            ]
        );

        // Same for events: a newer legacy record must be ingested after an
        // older queued one so it replaces it, not the reverse.
        fs::write(
            directory.join(format!("lianli-agent-event-codex.{:020}.9", 30_000)),
            "v1 30000 tool 1500\n",
        )
        .unwrap();
        fs::write(
            directory.join("lianli-agent-event-codex"),
            "v1 30010 read 1500\n",
        )
        .unwrap();
        let events = consume_agent_events(&directory, AgentActor::Codex, uid, 30_050);
        assert_eq!(
            events.iter().map(|record| record.kind).collect::<Vec<_>>(),
            vec![EventKind::Tool, EventKind::Read]
        );
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn retarget_after_returning_to_auto_still_interpolates_the_walk() {
        let mut engine = StateEngine::new(17, 0, 800);
        engine.step(input(0, 0));

        let mut first = input(100, 0);
        first.controls = vec![ControlCommand::Move(ScreenLocation::Right)];
        let first_command = engine.step(first).position.unwrap();

        // Auto releases manual mode mid-flight but must not forget the
        // in-flight walk...
        let auto_ms = 100 + first_command.duration_ms / 4;
        let mut auto = input(auto_ms, 0);
        auto.controls = vec![ControlCommand::Auto];
        engine.step(auto);
        assert_eq!(engine.manual_location, None);
        assert!(engine.manual_motion.is_some());

        // ...so a quick follow-up move still paces from the on-screen point.
        let retarget_ms = 100 + first_command.duration_ms / 2;
        let on_screen = engine.current_point(retarget_ms);
        assert_ne!(on_screen, ScreenLocation::Right.point());
        let mut second = input(retarget_ms, 0);
        second.controls = vec![ControlCommand::Move(ScreenLocation::Home)];
        let second_command = engine.step(second).position.unwrap();
        assert_eq!(
            second_command.duration_ms,
            movement_duration_for_pace(
                on_screen,
                ScreenLocation::Home.point(),
                RoamPace::Normal
            )
        );
    }

    #[test]
    fn mid_flight_manual_retarget_paces_from_the_interpolated_point() {
        let mut engine = StateEngine::new(13, 0, 600);
        engine.step(input(0, 0));

        let mut first = input(100, 0);
        first.controls = vec![ControlCommand::Move(ScreenLocation::Right)];
        let first_command = engine.step(first).position.unwrap();
        assert_eq!(first_command.point, ScreenLocation::Right.point());

        // Retarget halfway through the walk. The new leg must be paced from
        // the on-screen interpolated point, not from the old destination.
        let halfway_ms = 100 + first_command.duration_ms / 2;
        let on_screen = engine.current_point(halfway_ms);
        assert_ne!(on_screen, ScreenLocation::Right.point());
        assert_ne!(on_screen, ScreenLocation::Home.point());

        let mut second = input(halfway_ms, 0);
        second.controls = vec![ControlCommand::Move(ScreenLocation::Left)];
        let second_command = engine.step(second).position.unwrap();
        assert_eq!(second_command.point, ScreenLocation::Left.point());
        assert_eq!(
            second_command.duration_ms,
            movement_duration_for_pace(
                on_screen,
                ScreenLocation::Left.point(),
                RoamPace::Normal
            )
        );
        assert_ne!(
            second_command.duration_ms,
            movement_duration_for_pace(
                ScreenLocation::Right.point(),
                ScreenLocation::Left.point(),
                RoamPace::Normal
            )
        );
        assert_eq!(engine.manual_motion.unwrap().state, "stroll-left");

        // After the motion finishes, the visual point is the final target.
        let settle_ms = halfway_ms + second_command.duration_ms;
        assert_eq!(engine.current_point(settle_ms), ScreenLocation::Left.point());
    }

    #[test]
    fn labeled_sensor_channels_beyond_temp1_are_discovered() {
        let directory = temporary_directory("sensors-tempn");
        let gpu = directory.join("hwmon3");
        fs::create_dir(&gpu).unwrap();
        fs::write(gpu.join("name"), "amdgpu\n").unwrap();
        fs::write(gpu.join("temp1_label"), "edge\n").unwrap();
        fs::write(gpu.join("temp1_input"), "55000\n").unwrap();
        fs::write(gpu.join("temp2_label"), "junction\n").unwrap();
        fs::write(gpu.join("temp2_input"), "71500\n").unwrap();
        fs::write(gpu.join("temp3_label"), "mem\n").unwrap();
        fs::write(gpu.join("temp3_input"), "80000\n").unwrap();
        // A label file with an unreadable twin input must not abort discovery.
        fs::write(gpu.join("temp4_label"), "hotspot\n").unwrap();
        // Duplicate label on a two-digit channel: numeric order means the
        // lowest channel (temp2) wins over temp10.
        fs::write(gpu.join("temp10_label"), "junction\n").unwrap();
        fs::write(gpu.join("temp10_input"), "99000\n").unwrap();

        assert_eq!(
            hwmon_label_files(&gpu),
            vec![
                "temp1_label",
                "temp2_label",
                "temp3_label",
                "temp4_label",
                "temp10_label"
            ]
        );
        assert_eq!(
            read_hwmon_temperature(&directory, "amdgpu", "junction"),
            Some(71.5)
        );
        assert_eq!(
            read_hwmon_temperature(&directory, "amdgpu", "mem"),
            Some(80.0)
        );
        assert_eq!(read_hwmon_temperature(&directory, "amdgpu", "hotspot"), None);
        assert_eq!(read_hwmon_temperature(&directory, "amdgpu", "vram"), None);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn coolant_parser_rejects_non_finite_values() {
        assert_eq!(parse_temperature("34.5\n", false), Some(34.5));
        assert_eq!(parse_temperature("66250\n", true), Some(66.25));
        assert_eq!(parse_temperature("NaN", false), None);
        assert_eq!(parse_temperature("inf", false), None);
        assert_eq!(parse_temperature("999", false), None);

        let directory = temporary_directory("coolant");
        let coolant = directory.join("lianli-coolant-test");
        fs::write(&coolant, "34.5\n").unwrap();
        assert_eq!(
            read_coolant_temperature(&coolant, effective_uid().unwrap()),
            Some(34.5)
        );
        assert_eq!(coolant_path(&directory), Some(coolant.clone()));
        fs::remove_file(coolant).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn screen_grid_durations_and_position_wire_contract_are_exact() {
        use ScreenLocation::*;
        let all = [
            UpperLeft, Up, UpperRight, Left, Center, Right, LowerLeft, Down, LowerRight, Home,
        ];
        for waypoint in all {
            let point = waypoint.point();
            assert!((0..=LOGICAL_WIDTH).contains(&point.x));
            assert!((0..=LOGICAL_HEIGHT).contains(&point.y));
            assert_eq!(ScreenLocation::parse(waypoint.as_str()), Some(waypoint));
        }
        assert_eq!(MASCOT_WIDTH, 600);
        assert_eq!(MASCOT_HEIGHT, 400);
        assert_eq!(
            movement_duration_ms(UpperLeft.point(), UpperRight.point()),
            4_220
        );
        assert_eq!(movement_duration_ms(Up.point(), Center.point()), 1_200);
        let command = PositionCommand {
            seq: 42,
            point: Home.point(),
            duration_ms: 1_200,
        };
        assert_eq!(command.line(), "v1 42 1144 720 1200\n");
    }

    #[test]
    fn ambient_roam_starts_with_visible_horizontal_motion_and_events_cancel_it_once() {
        let mut engine = StateEngine::new(23, 0, 800);
        engine.step(input(0, 0));
        engine.next_ambient_ms = 100;
        let strolling = engine.step(input(100, 0));
        assert!(matches!(strolling.state, "stroll-left" | "stroll-right"));
        let target = strolling.position.unwrap();
        assert_eq!(target.point, ScreenLocation::Left.point());
        assert_ne!(target.point.x, ScreenLocation::Home.point().x);
        assert!((1_200..=5_000).contains(&target.duration_ms));

        let mut tool = input(200, 0);
        tool.events.push(event(
            AgentActor::Codex,
            EventKind::Tool,
            tool.now_unix_ms,
            1_500,
        ));
        let interrupted = engine.step(tool);
        assert_eq!(interrupted.state, "event-tool");
        assert_eq!(interrupted.position.unwrap().duration_ms, 0);
        assert!(engine.ambient.is_none());
        assert!(engine.step(input(300, 0)).position.is_none());
    }

    #[test]
    fn process_enter_and_exit_react_in_place_without_home_teleports() {
        let mut engine = StateEngine::new(24, 0, 850);
        engine.step(input(0, 0));
        engine.next_ambient_ms = 100;
        engine.step(input(100, 0));

        let entering = engine.step(input(400, CODEX));
        assert_eq!(entering.state, "codex-enter");
        let stopped = entering.position.unwrap();
        assert_eq!(stopped.duration_ms, 0);
        assert_ne!(stopped.point, Waypoint::Home.point());
        let preserved = stopped.point;

        assert_eq!(engine.step(input(2_401, CODEX)).state, "codex-wait");
        let exiting = engine.step(input(2_500, 0));
        assert_eq!(exiting.state, "codex-exit");
        assert!(exiting.position.is_none());
        assert_eq!(engine.position, preserved);
        let finished = engine.step(input(5_501, 0));
        assert_eq!(finished.state, "idle");
        assert!(finished.position.is_none());
        assert_eq!(engine.position, preserved);
    }

    #[test]
    fn micro_idle_selection_does_not_repeat_immediately() {
        let mut engine = StateEngine::new(25, 0, 900);
        let first = engine.random_micro_idle(0);
        let second = engine.random_micro_idle(0);
        let AmbientPhase::MicroIdle {
            state: first_state,
            until_ms: first_until,
        } = first
        else {
            unreachable!()
        };
        let AmbientPhase::MicroIdle {
            state: second_state,
            until_ms: second_until,
        } = second
        else {
            unreachable!()
        };
        assert_ne!(first_state, second_state);
        let available = [
            "idle-blink",
            "idle-hat-adjust",
            "idle-stretch",
            "idle-mug",
            "idle-gauge",
            "idle-breeze",
        ];
        assert!(available.contains(&first_state));
        assert!(available.contains(&second_state));
        assert!((1_000..=3_000).contains(&first_until));
        assert!((1_000..=3_000).contains(&second_until));
    }

    #[test]
    fn atomic_position_output_is_mode_600_and_sequence_survives_restart() {
        let directory = temporary_directory("position");
        let path = directory.join("lianli-agent-position");
        let command = PositionCommand {
            seq: 77,
            point: Waypoint::Home.point(),
            duration_ms: 0,
        };
        write_position(&path, command).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), command.line());
        assert_eq!(path.metadata().unwrap().mode() & 0o777, 0o600);
        assert_eq!(read_position_seq(&path, 10), 77);
        assert_eq!(read_position_seq(&path, 100), 100);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
