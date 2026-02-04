use regex::Regex;

/// Game-specific patterns for player detection from server logs
pub struct PlayerDetectionPatterns {
    pub join_regex: Regex,
    pub leave_regex: Regex,
    pub server_ready_regex: Regex,
    pub ip_regex: Option<Regex>,
}

impl PlayerDetectionPatterns {
    /// Create detection patterns based on game type
    pub fn for_game_type(game_type: &str) -> Self {
        match game_type.to_lowercase().as_str() {
            "hytale" => Self::hytale(),
            "minecraft" => Self::minecraft(),
            _ => Self::hytale(), // Default to hytale
        }
    }

    /// Hytale server log patterns
    /// Join: "[Universe|P] Adding player 'TheFRcRaZy (uuid)'"
    /// Leave: "[Universe|P] Removing player 'TheFRcRaZy' (uuid)"
    /// Ready: "[HytaleServer] Universe ready!"
    fn hytale() -> Self {
        Self {
            // Join: Adding player 'TheFRcRaZy (uuid)
            join_regex: Regex::new(r"Adding player '(.+?) \((.+?)\)").unwrap(),
            // Leave: Removing player 'TheFRcRaZy' (uuid)
            leave_regex: Regex::new(r"Removing player '(.+?)' \((.+?)\)").unwrap(),
            server_ready_regex: Regex::new(r"Universe ready!").unwrap(),
            // IP: {Playing(QuicConnectionAddress{...} (/82.64.248.19:55745, ...)), UUID, Name}
            ip_regex: Some(Regex::new(r"\{Playing\(.+? \(/([\d\.]+):\d+.*?\)\), ([0-9a-f-]+), (.+?)\}").unwrap()),
        }
    }

    /// Minecraft server log patterns (vanilla/spigot/paper)
    /// Join: "[Server thread/INFO]: PlayerName joined the game"
    /// Leave: "[Server thread/INFO]: PlayerName left the game"
    /// Ready: "Done (X.XXXs)! For help, type "help""
    fn minecraft() -> Self {
        Self {
            join_regex: Regex::new(r"\[.*\]: (.*) joined the game").unwrap(),
            leave_regex: Regex::new(r"\[.*\]: (.*) left the game").unwrap(),
            server_ready_regex: Regex::new(r"Done \([\d.]+s\)! For help").unwrap(),
            ip_regex: None,
        }
    }
}
