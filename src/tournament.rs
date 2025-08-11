use std::fs;
use serde_json;
use anyhow::Result;
use crate::raider_io::{PlayerData, RaiderIOClient};
use csv::ReaderBuilder;
use reqwest::Client;

pub fn read_members_data(file_path: &str) -> Result<Vec<PlayerData>> {
    let content = fs::read_to_string(file_path)?;
    let players: Vec<PlayerData> = serde_json::from_str(&content)?;
    Ok(players)
}

pub async fn fetch_tournament_data_from_sheets(sheet_url: &str) -> Result<Vec<PlayerData>> {
    let client = Client::new();
    let response = client.get(sheet_url).send().await?;
    let csv_content = response.text().await?;
    
    let mut reader = ReaderBuilder::new().has_headers(false).from_reader(csv_content.as_bytes());
    let mut players = Vec::new();
    let raider_client = RaiderIOClient::new();
    
    for (i, result) in reader.records().enumerate() {
        if i == 0 {
            continue;
        }
        
        let record = result?;
        if record.len() >= 3 {
            let name = record[0].to_string();
            let realm = record[1].to_string();
            let guild = record[2].to_string();
            
            if let Ok(Some(player)) = raider_client.fetch_player_data(&realm, &name, Some(guild)).await {
                players.push(player);
            }
        }
    }
    
    Ok(players)
}

pub fn get_tournament_players(players: &[PlayerData], guild_filter: Option<&str>, top: usize) -> TournamentRoster {
    let filtered_players: Vec<&PlayerData> = if let Some(guild) = guild_filter {
        players
            .iter()
            .filter(|p| {
                p.guild
                    .as_ref()
                    .map(|g| g.to_lowercase() == guild.to_lowercase())
                    .unwrap_or(false)
            })
            .collect()
    } else {
        players.iter().collect()
    };

    let melee_specs = [
        "frost", "unholy", "havoc", "feral", "survival", "windwalker", 
        "retribution", "assassination", "outlaw", "subtlety", "enhancement", 
        "arms", "fury"
    ];
    
    let ranged_specs = [
        "balance", "augmentation", "devastation", "beast mastery", "marksmanship",
        "arcane", "fire", "frost", "shadow", "elemental", "affliction",
        "demonology", "destruction"
    ];

    let mut tanks: Vec<&PlayerData> = filtered_players
        .iter()
        .filter(|p| p.rio_tank >= 1000)
        .cloned()
        .collect();
    tanks.sort_by(|a, b| b.rio_tank.cmp(&a.rio_tank));

    let mut healers: Vec<&PlayerData> = filtered_players
        .iter()
        .filter(|p| p.rio_healer >= 1000)
        .cloned()
        .collect();
    healers.sort_by(|a, b| b.rio_healer.cmp(&a.rio_healer));

    let mut melee_dps: Vec<&PlayerData> = filtered_players
        .iter()
        .filter(|p| {
            p.active_spec_name
                .as_ref()
                .map(|spec| melee_specs.contains(&spec.to_lowercase().as_str()))
                .unwrap_or(false)
                && p.class.as_ref().map(|c| c != "Mage").unwrap_or(true)
        })
        .cloned()
        .collect();
    melee_dps.sort_by(|a, b| b.rio_dps.cmp(&a.rio_dps));

    let mut ranged_dps: Vec<&PlayerData> = filtered_players
        .iter()
        .filter(|p| {
            p.active_spec_name
                .as_ref()
                .map(|spec| ranged_specs.contains(&spec.to_lowercase().as_str()))
                .unwrap_or(false)
                && p.class.as_ref().map(|c| c != "Death Knight").unwrap_or(true)
        })
        .cloned()
        .collect();
    ranged_dps.sort_by(|a, b| b.rio_dps.cmp(&a.rio_dps));

    TournamentRoster {
        tanks: tanks.into_iter().take(top).cloned().collect(),
        healers: healers.into_iter().take(top).cloned().collect(),
        melee_dps: melee_dps.into_iter().take(top).cloned().collect(),
        ranged_dps: ranged_dps.into_iter().take(top).cloned().collect(),
    }
}

#[derive(Debug)]
pub struct TournamentRoster {
    pub tanks: Vec<PlayerData>,
    pub healers: Vec<PlayerData>,
    pub melee_dps: Vec<PlayerData>,
    pub ranged_dps: Vec<PlayerData>,
}

impl TournamentRoster {
    pub fn format(&self, show_guild: bool) -> String {
        let mut result = String::new();
        
        result.push_str("**Tanks:**\n");
        for (i, player) in self.tanks.iter().enumerate() {
            if show_guild {
                result.push_str(&format!(
                    "{}. {} ({}) - {} {} - {}\n",
                    i + 1,
                    player.name,
                    player.guild.as_deref().unwrap_or("N/A"),
                    player.active_spec_name.as_deref().unwrap_or("Unknown"),
                    player.class.as_deref().unwrap_or("Unknown"),
                    player.rio_tank
                ));
            } else {
                result.push_str(&format!(
                    "{}. {} - {} {} - {}\n",
                    i + 1,
                    player.name,
                    player.active_spec_name.as_deref().unwrap_or("Unknown"),
                    player.class.as_deref().unwrap_or("Unknown"),
                    player.rio_tank
                ));
            }
        }
        
        result.push_str("\n**Healers:**\n");
        for (i, player) in self.healers.iter().enumerate() {
            if show_guild {
                result.push_str(&format!(
                    "{}. {} ({}) - {} {} - {}\n",
                    i + 1,
                    player.name,
                    player.guild.as_deref().unwrap_or("N/A"),
                    player.active_spec_name.as_deref().unwrap_or("Unknown"),
                    player.class.as_deref().unwrap_or("Unknown"),
                    player.rio_healer
                ));
            } else {
                result.push_str(&format!(
                    "{}. {} - {} {} - {}\n",
                    i + 1,
                    player.name,
                    player.active_spec_name.as_deref().unwrap_or("Unknown"),
                    player.class.as_deref().unwrap_or("Unknown"),
                    player.rio_healer
                ));
            }
        }
        
        result.push_str("\n**Melee DPS:**\n");
        for (i, player) in self.melee_dps.iter().enumerate() {
            if show_guild {
                result.push_str(&format!(
                    "{}. {} ({}) - {} {} - {}\n",
                    i + 1,
                    player.name,
                    player.guild.as_deref().unwrap_or("N/A"),
                    player.active_spec_name.as_deref().unwrap_or("Unknown"),
                    player.class.as_deref().unwrap_or("Unknown"),
                    player.rio_dps
                ));
            } else {
                result.push_str(&format!(
                    "{}. {} - {} {} - {}\n",
                    i + 1,
                    player.name,
                    player.active_spec_name.as_deref().unwrap_or("Unknown"),
                    player.class.as_deref().unwrap_or("Unknown"),
                    player.rio_dps
                ));
            }
        }
        
        result.push_str("\n**Ranged DPS:**\n");
        for (i, player) in self.ranged_dps.iter().enumerate() {
            if show_guild {
                result.push_str(&format!(
                    "{}. {} ({}) - {} {} - {}\n",
                    i + 1,
                    player.name,
                    player.guild.as_deref().unwrap_or("N/A"),
                    player.active_spec_name.as_deref().unwrap_or("Unknown"),
                    player.class.as_deref().unwrap_or("Unknown"),
                    player.rio_dps
                ));
            } else {
                result.push_str(&format!(
                    "{}. {} - {} {} - {}\n",
                    i + 1,
                    player.name,
                    player.active_spec_name.as_deref().unwrap_or("Unknown"),
                    player.class.as_deref().unwrap_or("Unknown"),
                    player.rio_dps
                ));
            }
        }
        
        result
    }
}