/// Strong types for better type safety and API clarity
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

/// A guild name with validation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GuildName(String);

/// A realm name with validation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RealmName(String);

/// A player/character name with validation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerName(String);

/// A WoW class name
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WowClass(String);

/// A specialization name
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpecName(String);

/// A season identifier (e.g., "current", "season-tww-3")
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Season(String);

/// A raid tier identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RaidTier(u8);

/// Mythic+ rating/score
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MythicPlusScore(u32);

/// World rank
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorldRank(u32);

/// Guild URL parameters for raider.io API
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GuildUrl {
    pub realm: RealmName,
    pub name: GuildName,
}

/// A unique identifier for a player (realm + name)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerId {
    pub realm: RealmName,
    pub name: PlayerName,
}

/// Raid difficulty levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RaidDifficulty {
    Normal,
    Heroic,
    Mythic,
}

/// Role types in WoW
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Tank,
    Healer,
    #[serde(rename = "dps")]
    Dps,
}

// Implementations for GuildName
impl GuildName {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        // Basic validation - guild names can't be empty
        assert!(!name.trim().is_empty(), "Guild name cannot be empty");
        Self(name.trim().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_lowercase(&self) -> String {
        self.0.to_lowercase()
    }
}

impl Deref for GuildName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for GuildName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for GuildName {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.trim().is_empty() {
            Err("Guild name cannot be empty")
        } else {
            Ok(Self(s.trim().to_string()))
        }
    }
}

impl From<String> for GuildName {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for GuildName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

// Implementations for RealmName
impl RealmName {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.trim().is_empty(), "Realm name cannot be empty");
        // Normalize realm names by replacing spaces with hyphens and converting to lowercase
        let normalized = name.trim().to_lowercase().replace(' ', "-");
        Self(normalized)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the realm name formatted for display with proper capitalization and spaces
    pub fn display_name(&self) -> String {
        self.0
            .split('-')
            .map(|word| {
                if word.is_empty() {
                    String::new()
                } else {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + &chars.collect::<String>()
                    }
                }
            })
            .collect::<Vec<String>>()
            .join(" ")
    }
}

impl Deref for RealmName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for RealmName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for RealmName {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.trim().is_empty() {
            Err("Realm name cannot be empty")
        } else {
            Ok(Self::new(s))
        }
    }
}

impl From<String> for RealmName {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for RealmName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

// Implementations for PlayerName
impl PlayerName {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.trim().is_empty(), "Player name cannot be empty");
        // Player names are case-insensitive, normalize to proper case
        let normalized = Self::normalize_name(&name);
        Self(normalized)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn normalize_name(name: &str) -> String {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        
        // Capitalize first letter, lowercase the rest
        let mut chars: Vec<char> = trimmed.chars().collect();
        chars[0] = chars[0].to_uppercase().next().unwrap_or(chars[0]);
        for ch in chars.iter_mut().skip(1) {
            *ch = ch.to_lowercase().next().unwrap_or(*ch);
        }
        chars.into_iter().collect()
    }
}

impl Deref for PlayerName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for PlayerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for PlayerName {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.trim().is_empty() {
            Err("Player name cannot be empty")
        } else {
            Ok(Self::new(s))
        }
    }
}

impl From<String> for PlayerName {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for PlayerName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

// Implementations for Season
impl Season {
    pub fn new(season: impl Into<String>) -> Self {
        Self(season.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn current() -> Self {
        Self("current".to_string())
    }

    pub fn previous() -> Self {
        Self("previous".to_string())
    }
}

impl fmt::Display for Season {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for Season {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for Season {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

// Implementations for MythicPlusScore
impl MythicPlusScore {
    pub fn new(score: u32) -> Self {
        Self(score)
    }

    pub fn value(&self) -> u32 {
        self.0
    }

    pub fn zero() -> Self {
        Self(0)
    }
}

impl PartialOrd<u32> for MythicPlusScore {
    fn partial_cmp(&self, other: &u32) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl PartialEq<u32> for MythicPlusScore {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl fmt::Display for MythicPlusScore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for MythicPlusScore {
    fn from(score: u32) -> Self {
        Self::new(score)
    }
}

impl From<MythicPlusScore> for u32 {
    fn from(score: MythicPlusScore) -> Self {
        score.0
    }
}

// Implementations for WorldRank
impl WorldRank {
    pub fn new(rank: u32) -> Self {
        Self(rank)
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for WorldRank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for WorldRank {
    fn from(rank: u32) -> Self {
        Self::new(rank)
    }
}

// Implementations for RaidTier
impl RaidTier {
    pub fn new(tier: u8) -> Self {
        Self(tier)
    }

    pub fn value(&self) -> u8 {
        self.0
    }

    pub fn nerubar_palace() -> Self {
        Self(1)
    }

    pub fn liberation_of_undermine() -> Self {
        Self(2)
    }

    pub fn manaforge_omega() -> Self {
        Self(3)
    }
}

impl fmt::Display for RaidTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u8> for RaidTier {
    fn from(tier: u8) -> Self {
        Self::new(tier)
    }
}

// Implementations for PlayerId
impl PlayerId {
    pub fn new(realm: impl Into<RealmName>, name: impl Into<PlayerName>) -> Self {
        Self {
            realm: realm.into(),
            name: name.into(),
        }
    }
}

impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.realm)
    }
}

// Implementations for GuildUrl
impl GuildUrl {
    pub fn new(realm: impl Into<RealmName>, name: impl Into<GuildName>) -> Self {
        Self {
            realm: realm.into(),
            name: name.into(),
        }
    }

    pub fn to_query_string(&self) -> String {
        // URL encode the guild name to handle spaces and special characters
        let realm_string = self.realm.to_string();
        let name_string = self.name.to_string();
        let encoded_realm = urlencoding::encode(&realm_string);
        let encoded_name = urlencoding::encode(&name_string);
        format!("realm={}&name={}", encoded_realm, encoded_name)
    }
}

impl fmt::Display for GuildUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.realm, self.name)
    }
}

// Implementations for RaidDifficulty
impl fmt::Display for RaidDifficulty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RaidDifficulty::Normal => write!(f, "normal"),
            RaidDifficulty::Heroic => write!(f, "heroic"),
            RaidDifficulty::Mythic => write!(f, "mythic"),
        }
    }
}

// Implementations for Role
impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Tank => write!(f, "tank"),
            Role::Healer => write!(f, "healer"),
            Role::Dps => write!(f, "dps"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_name_normalization() {
        assert_eq!(PlayerName::new("testplayer").as_str(), "Testplayer");
        assert_eq!(PlayerName::new("TESTPLAYER").as_str(), "Testplayer");
        assert_eq!(PlayerName::new("tESTPLAYER").as_str(), "Testplayer");
    }

    #[test]
    fn test_realm_name_normalization() {
        assert_eq!(RealmName::new("Tarren Mill").as_str(), "tarren-mill");
        assert_eq!(RealmName::new("TARREN MILL").as_str(), "tarren-mill");
    }

    #[test]
    fn test_guild_url_query_string() {
        let guild_url = GuildUrl::new("tarren-mill", "Test Guild");
        assert_eq!(guild_url.to_query_string(), "realm=tarren-mill&name=Test Guild");
    }

    #[test]
    fn test_player_id_display() {
        let player_id = PlayerId::new("tarren-mill", "testplayer");
        assert_eq!(player_id.to_string(), "Testplayer-tarren-mill");
    }
}