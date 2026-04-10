use nucleo::Matcher;
use nucleo::Utf32Str;

const PICKER_ALIASES: &[&str] = &[
    "picker", "open picker", "sessions", "show sessions", "session picker",
    "picture", "open picture", "pick her", "pick", "open the picker",
];
const DOCK_ALIASES: &[&str] = &[
    "dock", "workers", "show workers", "show dock", "open dock", "worker dock",
    "doc", "docs", "open doc", "show doc", "open the dock", "open the doc",
];
const STATS_ALIASES: &[&str] = &[
    "stats", "dashboard", "show stats", "open stats", "statistics",
    "stat", "show stat", "open stat", "show the stats", "open the stats",
];
const NOTIFICATIONS_ALIASES: &[&str] = &[
    "notifications", "notifs", "show notifications", "open notifications", "jump",
    "notification", "show notification", "notify", "show notifs",
];
const ACCEPT_ALIASES: &[&str] = &[];
const SEND_ALIASES: &[&str] = &[
    "send", "send it", "submit", "ship it", "go", "send it now",
    "ok", "okay",
];
const CANCEL_ALIASES: &[&str] = &[
    "cancel", "stop", "nevermind", "never mind", "abort", "discard",
    "throw it away", "scrap it",
];
const CLEAR_ALIASES: &[&str] = &[
    "clear", "clear it", "clear prompt", "clear the prompt", "wipe",
    "wipe it", "wipe prompt", "erase", "erase it", "reset prompt",
];
const CLOSE_ALIASES: &[&str] = &[
    "close", "close session", "close the session", "close this session",
    "kill session", "kill the session", "kill this session",
    "quit session", "quit the session", "quit this session",
    "exit session", "exit the session", "exit this session",
    "close it",
];
const NEXT_ALIASES: &[&str] = &["next", "next agent", "next worker", "next one"];
const PREVIOUS_ALIASES: &[&str] = &[
    "previous", "prev", "previous agent", "prev agent",
    "previous worker", "previous one", "back", "go back",
];
const NEXT_SESSION_ALIASES: &[&str] = &[
    "next session", "next project",
];
const PREVIOUS_SESSION_ALIASES: &[&str] = &[
    "previous session", "prev session", "previous project", "prev project",
];

const CLOSE_PREFIXES: &[&str] = &[
    "close ",
    "close session ",
    "kill ",
    "kill session ",
    "quit ",
];
const OPEN_PREFIXES: &[&str] = &[
    "open session ",
    "start session ",
    "launch ",
    "start ",
    "open ",
];

const CHOOSE_PREFIXES: &[&str] = &[
    "choose ",
    "select ",
    "option ",
    "pick ",
];

const BTW_PREFIXES: &[&str] = &[
    "by the way ",
    "btw ",
];
const BTW_ALIASES: &[&str] = &[
    "by the way", "btw",
];

const DICTATION_PREFIXES: &[&str] = &[
    "tell agent ",
    "tell the agent ",
    "ask agent ",
    "ask the agent ",
    "prompt agent ",
    "tell claude ",
    "ask claude ",
    "prompt claude ",
    "dictate ",
    "say ",
    "prompt ",
];

const JUMP_PREFIXES: &[&str] = &[
    "agent ",
    "jump to ",
    "go to ",
    "switch to ",
    "open session ",
];

#[derive(Debug)]
pub enum Intent {
    OpenPicker,
    OpenDock,
    OpenStats,
    OpenNotifications,
    Accept,
    Choose(usize),
    JumpTo(String),
    JumpToIndex(usize),
    NextAgent,
    PreviousAgent,
    NextSession,
    PreviousSession,
    Btw(Option<String>),
    Dictate(String),
    Send,
    Cancel,
    Clear,
    CloseSession(Option<String>),
    OpenSession(String),
}

impl Intent {
    pub fn label(&self) -> Option<String> {
        let s = match self {
            Intent::OpenPicker => "picker".into(),
            Intent::OpenDock => "dock".into(),
            Intent::OpenStats => "stats".into(),
            Intent::OpenNotifications => "notifications".into(),
            Intent::Accept => "accept".into(),
            Intent::Choose(n) => format!("choose {n}"),
            Intent::JumpTo(s) => format!("jump → {s}"),
            Intent::JumpToIndex(n) => format!("jump → #{n}"),
            Intent::NextAgent => "next agent".into(),
            Intent::PreviousAgent => "prev agent".into(),
            Intent::NextSession => "next session".into(),
            Intent::PreviousSession => "prev session".into(),
            Intent::Send => "send".into(),
            Intent::Cancel => "cancel".into(),
            Intent::Clear => "clear".into(),
            Intent::CloseSession(None) => "close session".into(),
            Intent::CloseSession(Some(s)) => format!("close {s}"),
            Intent::OpenSession(s) => format!("open {s}"),
            Intent::Btw(None) => "/btw".into(),
            Intent::Btw(Some(_)) => "/btw …".into(),
            Intent::Dictate(_) => return None,
        };
        Some(s)
    }
}

fn merge<'a>(defaults: &[&'a str], extra: &'a [String]) -> Vec<&'a str> {
    let mut v: Vec<&str> = defaults.to_vec();
    for s in extra {
        v.push(s.as_str());
    }
    v
}

pub fn parse(
    raw: &str,
    sessions: &[String],
    aliases: &std::collections::HashMap<String, String>,
    cfg: &crate::config::VoiceAliases,
) -> Option<Intent> {
    let normalized = normalize(raw);
    let t = normalized.as_str();
    if t.is_empty() {
        return None;
    }

    let picker = merge(PICKER_ALIASES, &cfg.picker);
    let dock = merge(DOCK_ALIASES, &cfg.dock);
    let stats = merge(STATS_ALIASES, &cfg.stats);
    let notifs = merge(NOTIFICATIONS_ALIASES, &cfg.notifications);
    let send = merge(SEND_ALIASES, &cfg.send);
    let cancel = merge(CANCEL_ALIASES, &cfg.cancel);
    let clear = merge(CLEAR_ALIASES, &cfg.clear);
    let close = merge(CLOSE_ALIASES, &cfg.close);
    let next_a = merge(NEXT_ALIASES, &cfg.next_agent);
    let prev_a = merge(PREVIOUS_ALIASES, &cfg.previous_agent);
    let next_s = merge(NEXT_SESSION_ALIASES, &cfg.next_session);
    let prev_s = merge(PREVIOUS_SESSION_ALIASES, &cfg.previous_session);
    let btw_a = merge(BTW_ALIASES, &cfg.btw);
    let dictate_p = merge(DICTATION_PREFIXES, &cfg.dictate);
    let jump_p = merge(JUMP_PREFIXES, &cfg.jump);
    let choose_p = merge(CHOOSE_PREFIXES, &cfg.choose);
    let open_p = merge(OPEN_PREFIXES, &cfg.open);
    let close_p = merge(CLOSE_PREFIXES, &cfg.close_named);
    let btw_p = merge(BTW_PREFIXES, &cfg.btw);

    for prefix in &btw_p {
        if let Some(rest) = t.strip_prefix(*prefix) {
            let rest = rest.trim();
            if rest.is_empty() {
                return Some(Intent::Btw(None));
            }
            return Some(Intent::Btw(Some(rest.to_string())));
        }
    }
    if matches_alias(t, &btw_a) {
        return Some(Intent::Btw(None));
    }

    for prefix in &dictate_p {
        if let Some(rest) = t.strip_prefix(*prefix) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Some(Intent::Dictate(rest.to_string()));
            }
        }
    }

    if matches_alias(t, &picker) {
        return Some(Intent::OpenPicker);
    }
    if matches_alias(t, &dock) {
        return Some(Intent::OpenDock);
    }
    if matches_alias(t, &stats) {
        return Some(Intent::OpenStats);
    }
    if matches_alias(t, &notifs) {
        return Some(Intent::OpenNotifications);
    }
    for prefix in &choose_p {
        if let Some(rest) = t.strip_prefix(*prefix) {
            if let Some(n) = parse_index(rest.trim()) {
                return Some(Intent::Choose(n));
            }
        }
    }
    if matches_alias(t, ACCEPT_ALIASES) {
        return Some(Intent::Accept);
    }
    if matches_alias(t, &send) {
        return Some(Intent::Send);
    }
    if matches_alias(t, &cancel) {
        return Some(Intent::Cancel);
    }
    if matches_alias(t, &clear) {
        return Some(Intent::Clear);
    }
    if matches_alias(t, &close) {
        return Some(Intent::CloseSession(None));
    }
    for prefix in &close_p {
        if let Some(rest) = t.strip_prefix(*prefix) {
            let rest = rest.trim();
            if rest.is_empty() {
                return Some(Intent::CloseSession(None));
            }
            if let Some(name) = resolve_session(rest, sessions, aliases) {
                return Some(Intent::CloseSession(Some(name)));
            }
        }
    }
    for prefix in &open_p {
        if let Some(rest) = t.strip_prefix(*prefix) {
            let rest = rest.trim();
            if !rest.is_empty() {
                if let Some(name) = resolve_session(rest, sessions, aliases) {
                    return Some(Intent::OpenSession(name));
                }
            }
        }
    }
    if matches_alias(t, &next_s) {
        return Some(Intent::NextSession);
    }
    if matches_alias(t, &prev_s) {
        return Some(Intent::PreviousSession);
    }
    if matches_alias(t, &next_a) {
        return Some(Intent::NextAgent);
    }
    if matches_alias(t, &prev_a) {
        return Some(Intent::PreviousAgent);
    }

    for prefix in &jump_p {
        if let Some(rest) = t.strip_prefix(*prefix) {
            let rest = rest.trim();
            if let Some(n) = parse_index(rest) {
                return Some(Intent::JumpToIndex(n));
            }
            if let Some(name) = resolve_session(rest, sessions, aliases) {
                return Some(Intent::JumpTo(name));
            }
        }
    }

    let word_count = t.split_whitespace().count();
    if word_count <= 3 {
        if let Some(name) = resolve_session(t, sessions, aliases) {
            return Some(Intent::JumpTo(name));
        }
    }

    if word_count >= 4 {
        return Some(Intent::Dictate(t.to_string()));
    }

    None
}

fn matches_alias(t: &str, aliases: &[&str]) -> bool {
    aliases.iter().any(|a| *a == t)
}

// Whisper double-emits words on capitalization uncertainty (`free/Free.`),
// which would otherwise make the fuzzy needle longer than the haystack.
fn normalize(raw: &str) -> String {
    let mut spaced = String::with_capacity(raw.len());
    let mut last_space = true;
    for c in raw.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                spaced.push(lc);
            }
            last_space = false;
        } else if !last_space {
            spaced.push(' ');
            last_space = true;
        }
    }
    let mut out: Vec<&str> = Vec::new();
    for word in spaced.split_whitespace() {
        if out.last() != Some(&word) {
            out.push(word);
        }
    }
    out.join(" ")
}

fn parse_index(s: &str) -> Option<usize> {
    let s = s.trim();
    if let Ok(n) = s.parse::<usize>() {
        return if n >= 1 { Some(n) } else { None };
    }
    let n = match s {
        "one" => 1,
        "two" | "to" | "too" => 2,
        "three" => 3,
        "four" | "for" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" | "ate" => 8,
        "nine" => 9,
        "ten" => 10,
        _ => return None,
    };
    Some(n)
}

fn resolve_session(
    query: &str,
    sessions: &[String],
    aliases: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let q = query.trim();
    if let Some(session) = aliases.get(q) {
        return Some(session.clone());
    }
    fuzzy_session(q, sessions)
}

fn fuzzy_session(query: &str, sessions: &[String]) -> Option<String> {
    if query.is_empty() || sessions.is_empty() {
        return None;
    }
    let normalize = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect()
    };
    let q = normalize(query);
    if q.is_empty() {
        return None;
    }

    let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
    let mut nbuf = Vec::new();
    let needle = Utf32Str::new(&q, &mut nbuf);

    let mut best: Option<(u16, String)> = None;
    for s in sessions {
        let normalized = normalize(s);
        let mut hbuf = Vec::new();
        let hay = Utf32Str::new(&normalized, &mut hbuf);
        if let Some(score) = matcher.fuzzy_match(hay, needle) {
            if best.as_ref().map_or(true, |(b, _)| score > *b) {
                best = Some((score, s.clone()));
            }
        }
    }
    best.map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn no_aliases() -> HashMap<String, String> {
        HashMap::new()
    }

    fn no_cfg() -> crate::config::VoiceAliases {
        crate::config::VoiceAliases::default()
    }

    #[test]
    fn parses_basic_commands() {
        let sessions = vec!["hive".to_string(), "rust-foo".to_string()];
        let a = no_aliases();
        let c = no_cfg();
        assert!(matches!(parse("open dock.", &sessions, &a, &c), Some(Intent::OpenDock)));
        assert!(matches!(parse(" Picker", &sessions, &a, &c), Some(Intent::OpenPicker)));
        assert!(matches!(parse("select 1", &sessions, &a, &c), Some(Intent::Choose(1))));
        assert!(matches!(parse("option 3", &sessions, &a, &c), Some(Intent::Choose(3))));
        assert!(matches!(parse("Choose two.", &sessions, &a, &c), Some(Intent::Choose(2))));
        assert!(matches!(
            parse("jump to rust foo", &sessions, &a, &c),
            Some(Intent::JumpTo(s)) if s == "rust-foo"
        ));
        assert!(matches!(
            parse("tell agent refactor the picker", &sessions, &a, &c),
            Some(Intent::Dictate(s)) if s == "refactor the picker"
        ));
        assert!(matches!(
            parse("tell claude refactor the picker", &sessions, &a, &c),
            Some(Intent::Dictate(s)) if s == "refactor the picker"
        ));
        assert!(matches!(parse("send", &sessions, &a, &c), Some(Intent::Send)));
        assert!(matches!(parse("send it", &sessions, &a, &c), Some(Intent::Send)));
        assert!(matches!(
            parse("refactor the picker module please", &sessions, &a, &c),
            Some(Intent::Dictate(_))
        ));
        assert!(parse("blah blah", &sessions, &a, &c).is_none());
    }

    #[test]
    fn handles_punctuation_inside_command() {
        let sessions = vec!["kensa".to_string(), "rust-foo".to_string()];
        let a = no_aliases();
        let c = no_cfg();
        assert!(matches!(
            parse("Go to, Kensa.", &sessions, &a, &c),
            Some(Intent::JumpTo(s)) if s == "kensa"
        ));
        assert!(matches!(
            parse("Jump to 3!", &sessions, &a, &c),
            Some(Intent::JumpToIndex(3))
        ));
        assert!(matches!(parse("Next agent.", &sessions, &a, &c), Some(Intent::NextAgent)));
        assert!(matches!(parse("Previous!", &sessions, &a, &c), Some(Intent::PreviousAgent)));
        assert!(matches!(parse("Clear.", &sessions, &a, &c), Some(Intent::Clear)));
    }

    #[test]
    fn agent_prefix_jumps_by_index() {
        let sessions = vec!["alpha".to_string()];
        let a = no_aliases();
        let c = no_cfg();
        assert!(matches!(parse("agent 1", &sessions, &a, &c), Some(Intent::JumpToIndex(1))));
        assert!(matches!(parse("Agent 3.", &sessions, &a, &c), Some(Intent::JumpToIndex(3))));
        assert!(matches!(parse("agent two", &sessions, &a, &c), Some(Intent::JumpToIndex(2))));
        assert!(matches!(parse("3", &sessions, &a, &c), None));
        assert!(parse("one", &sessions, &a, &c).is_none());
    }

    #[test]
    fn handles_whisper_capitalization_repeats() {
        let sessions = vec!["free".to_string(), "rust-foo".to_string()];
        let a = no_aliases();
        let c = no_cfg();
        assert!(matches!(
            parse("Go to, free/Free.", &sessions, &a, &c),
            Some(Intent::JumpTo(s)) if s == "free"
        ));
        assert!(matches!(
            parse("jump to RUST/rust", &sessions, &a, &c),
            Some(Intent::JumpTo(s)) if s == "rust-foo"
        ));
    }

    #[test]
    fn resolves_worktree_aliases() {
        let sessions = vec![
            "kensa".to_string(),
            "kensa-nr-editor".to_string(),
            "kensa-nr-oss-licenses".to_string(),
        ];
        let c = no_cfg();
        let mut a = HashMap::new();
        // Full branch aliases
        a.insert("nr editor".to_string(), "kensa-nr-editor".to_string());
        a.insert("nr-editor".to_string(), "kensa-nr-editor".to_string());
        a.insert("nr oss licenses".to_string(), "kensa-nr-oss-licenses".to_string());
        a.insert("nr-oss-licenses".to_string(), "kensa-nr-oss-licenses".to_string());
        // Short aliases (prefix stripped)
        a.insert("editor".to_string(), "kensa-nr-editor".to_string());
        a.insert("oss licenses".to_string(), "kensa-nr-oss-licenses".to_string());
        a.insert("oss-licenses".to_string(), "kensa-nr-oss-licenses".to_string());

        assert!(matches!(
            parse("go to editor", &sessions, &a, &c),
            Some(Intent::JumpTo(s)) if s == "kensa-nr-editor"
        ));
        assert!(matches!(
            parse("jump to oss licenses", &sessions, &a, &c),
            Some(Intent::JumpTo(s)) if s == "kensa-nr-oss-licenses"
        ));
        assert!(matches!(
            parse("editor", &sessions, &a, &c),
            Some(Intent::JumpTo(s)) if s == "kensa-nr-editor"
        ));
        assert!(matches!(
            parse("close editor", &sessions, &a, &c),
            Some(Intent::CloseSession(Some(s))) if s == "kensa-nr-editor"
        ));
    }
}
