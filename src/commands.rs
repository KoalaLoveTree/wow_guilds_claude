use serenity::builder::CreateApplicationCommand;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::command::CommandOptionType;
use crate::config::AppConfig;
use crate::database::{Database, DbMember};
use crate::guild_data::{fetch_all_guild_data, sort_guilds, format_guild_list};
use crate::raider_io::PlayerData;
use crate::types::{RaidTier, PlayerName, RealmName, GuildName, MythicPlusScore};

pub fn guilds_command(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command
        .name("guilds")
        .description("Guilds Raid Rank")
        .create_option(|option| {
            option
                .name("season")
                .description("1/2/3")
                .kind(CommandOptionType::Integer)
                .required(false)
        })
        .create_option(|option| {
            option
                .name("limit")
                .description("Number of guilds to display (or 'all' for full list)")
                .kind(CommandOptionType::String)
                .required(false)
        })
}

pub fn rank_command(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command
        .name("rank")
        .description("Guilds Mythic+ Rank")
        .create_option(|option| {
            option
                .name("top")
                .description("1-50")
                .kind(CommandOptionType::Integer)
                .required(false)
        })
        .create_option(|option| {
            option
                .name("guilds")
                .description("all/Guild Name/... multiple guilds can be entered through ','")
                .kind(CommandOptionType::String)
                .required(false)
        })
        .create_option(|option| {
            option
                .name("classes")
                .description("all/death knight/death knight:3/... ':3' means you want to specify the spec")
                .kind(CommandOptionType::String)
                .required(false)
        })
        .create_option(|option| {
            option
                .name("role")
                .description("all/dps/healer/tank")
                .kind(CommandOptionType::String)
                .required(false)
        })
        .create_option(|option| {
            option
                .name("rio")
                .description("0-3500")
                .kind(CommandOptionType::Integer)
                .required(false)
        })
}


pub fn about_us_command(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command.name("about_us").description("About us")
}

pub fn rules_command(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command.name("rules").description("Rules")
}

pub fn help_command(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command.name("help").description("Get information about available commands")
}

pub async fn handle_guilds_command(command: &ApplicationCommandInteraction, config: &AppConfig) -> String {
    let season = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "season")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_i64()))
        .unwrap_or(config.raider_io.default_season as i64) as u8;

    let limit_str = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "limit")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_str()))
        .unwrap_or("10");

    let limit: Option<usize> = if limit_str == "all" {
        None
    } else {
        limit_str.parse().ok()
    };

    match fetch_all_guild_data(RaidTier::from(season), config).await {
        Ok(guilds) => {
            if guilds.is_empty() {
                format!("At the moment, there are no guilds with progression in season {}.", season)
            } else {
                let sorted_guilds = sort_guilds(guilds);
                format_guild_list(&sorted_guilds, limit, limit.is_none())
            }
        }
        Err(e) => {
            eprintln!("Error fetching guild data: {}", e);
            format!("An error occurred while fetching guild data: {}. Please check that uaguildlist.txt exists and contains valid guild URLs.", e)
        }
    }
}

pub async fn handle_rank_command(command: &ApplicationCommandInteraction, database: &Database) -> String {
    let messages = handle_rank_command_multi(command, database).await;
    messages.into_iter().next().unwrap_or_else(|| "No results to display.".to_string())
}

pub async fn handle_rank_command_multi(command: &ApplicationCommandInteraction, database: &Database) -> Vec<String> {
    let top = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "top")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_i64()))
        .unwrap_or(10) as usize;

    let guilds = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "guilds")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_str()))
        .unwrap_or("all");

    let classes = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "classes")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_str()))
        .unwrap_or("all");

    let role = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "role")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_str()))
        .unwrap_or("all");

    let rio = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "rio")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_i64()))
        .unwrap_or(2000) as u32;

    if !(1..=50).contains(&top) {
        return vec!["Error: The value of top must be between 1 and 50 inclusive.".to_string()];
    }

    if rio > 3500 {
        return vec!["Error: The value of rio must be between 0 and 3500 inclusive.".to_string()];
    }

    // Validate class and role like Python version
    let (class_filter, spec_number) = parse_class_spec(classes);
    
    if !validate_class(&class_filter) {
        return vec![format!("Class '{}' does not exist. Use the valid classes: all, death knight, demon hunter, druid, evoker, hunter, mage, monk, paladin, priest, rogue, shaman, warlock, warrior.", class_filter)];
    }
    
    if !validate_role(role) {
        return vec![format!("Role '{}' does not exist. Use the valid roles: all, dps, healer, tank.", role)];
    }

    // Get members from database
    match database.get_all_members().await {
        Ok(db_members) => {
            let mut players: Vec<PlayerData> = db_members.iter().map(db_member_to_player_data).collect();
            println!("Loaded {} players from database", players.len());
            println!("Filtering: class='{}', role='{}', guilds='{}', rio>{}", class_filter, role, guilds, rio);
            
            // Filter by guild
            if guilds != "all" {
                let guild_list: Vec<String> = guilds
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .collect();
                players.retain(|p| {
                    if guild_list.contains(&"none".to_string()) {
                        p.guild.is_none()
                    } else {
                        p.guild
                            .as_ref()
                            .map(|g| guild_list.contains(&g.to_lowercase()))
                            .unwrap_or(false)
                    }
                });
            }

            // Filter by class
            if class_filter != "all" {
                let before_count = players.len();
                players.retain(|p| {
                    p.class
                        .as_ref()
                        .map(|c| c.to_lowercase() == class_filter.to_lowercase())
                        .unwrap_or(false)
                });
                println!("After class filter '{}': {} players (was {})", class_filter, players.len(), before_count);
            }

            // Sort and filter by role/spec (following Python logic exactly)
            if let Some(spec) = spec_number {
                // Spec-based filtering
                players.sort_by(|a, b| {
                    let a_score = get_spec_score(a, spec - 1);
                    let b_score = get_spec_score(b, spec - 1);
                    b_score.partial_cmp(&a_score).unwrap_or(std::cmp::Ordering::Equal)
                });
                players.retain(|p| get_spec_score(p, spec - 1) > rio as f64);
            } else {
                // Role-based filtering - sort by role-specific RIO
                if role != "all" {
                    players.sort_by(|a, b| {
                        let a_score = get_role_score(a, role);
                        let b_score = get_role_score(b, role);
                        b_score.partial_cmp(&a_score).unwrap_or(std::cmp::Ordering::Equal)
                    });
                } else {
                    players.sort_by(|a, b| {
                        let a_score = a.rio_all.value();
                        let b_score = b.rio_all.value();
                        b_score.partial_cmp(&a_score).unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
                
                // Filter by role-specific RIO (exactly like Python)
                let before_count = players.len();
                if role != "all" {
                    players.retain(|p| get_role_score(p, role) > rio as f64);
                } else {
                    players.retain(|p| p.rio_all.value() > rio as f64);
                }
                println!("After RIO filter (>{} for role '{}'): {} players (was {})", rio, role, players.len(), before_count);
            }

            players.truncate(top);

            if players.is_empty() {
                return vec!["No players found matching the criteria.".to_string()];
            }

            // Build multiple message chunks to handle Discord's 2000 character limit
            let header = format!(
                "**Player Rankings (Top {} | Classes: {} | Guilds: {} | Role: {} | RIO > {}):**",
                top, classes, guilds, role, rio
            );

            let table_header = "```\nRank Player                       Guild                              Server               Class/Spec               RIO Score\n──── ───────────────────────────── ────────────────────────────────── ──────────────────── ──────────────────────── ─────────\n";
            let table_footer = "```";
            
            let total_players = players.len();
            let discord_limit = 2000;
            let estimated_row_size = 150;
            let base_message_size = header.len() + table_header.len() + table_footer.len() + 100; // Increased safety margin
            let calculated_max_rows = ((discord_limit - base_message_size) / estimated_row_size).max(1);
            
            // Ensure top 10 always fits in one message, but allow more for smaller requests
            let max_rows_per_message = if total_players <= 10 {
                total_players // Force all players into one message for top 10 or less
            } else {
                calculated_max_rows.max(10) // Ensure at least 10 rows per message for larger requests
            };
            
            let mut messages = Vec::new();
            
            for chunk_start in (0..total_players).step_by(max_rows_per_message) {
                let chunk_end = (chunk_start + max_rows_per_message).min(total_players);
                let chunk_players = &players[chunk_start..chunk_end];
                
                let mut message = if chunk_start == 0 {
                    format!("{}\n", header) // Only include header in first message
                } else {
                    format!("**Player Rankings (continued - {} to {}):**\n", chunk_start + 1, chunk_end)
                };
                
                message.push_str(table_header);
                
                for (i, player) in chunk_players.iter().enumerate() {
                    let global_index = chunk_start + i;
                    let (display_role, score) = if let Some(spec) = spec_number {
                        // For spec-based, show the role but use spec score
                        (role.to_string(), get_spec_score(player, spec - 1))
                    } else if role != "all" {
                        // For role-specific, show role and use role score
                        (role.to_string(), get_role_score(player, role))
                    } else {
                        // For "all", show "all" and use rio_all
                        ("all".to_string(), player.rio_all.value())
                    };

                    let rank_num = format!("#{}", global_index + 1);
                    let player_name = truncate_and_pad(&player.name.to_string(), 31);
                    let guild_name = truncate_and_pad(&player.guild.as_deref().unwrap_or("No Guild"), 34);
                    let server = truncate_and_pad(&player.realm.display_name(), 20);
                    
                    let class_spec = format!(
                        "{} {}",
                        player.active_spec_name.as_deref().unwrap_or("Unknown"),
                        player.class.as_deref().unwrap_or("Unknown")
                    );
                    let class_spec_str = truncate_and_pad(&class_spec, 24);
                    
                    let score_display = if display_role == "all" {
                        format!("{:.1} (Overall)", score)
                    } else {
                        format!("{:.1} ({})", score, display_role.to_uppercase())
                    };

                    message.push_str(&format!(
                        "{:<4} {:<31} {:<34} {:<20} {:<24} {}\n",
                        rank_num,
                        player_name,
                        guild_name,
                        server,
                        class_spec_str,
                        score_display
                    ));
                }
                
                message.push_str(table_footer);
                messages.push(message);
            }
            
            messages
        }
        Err(e) => {
            vec![format!("No data to process: {}. Check that the database contains member data.", e)]
        }
    }
}


pub async fn handle_about_us_command() -> String {
    "https://www.wowprogress.com/guild/eu/tarren-mill/Thorned+Horde".to_string()
}

pub async fn handle_rules_command(config: &AppConfig) -> String {
    if let (Some(server_id), Some(channel_id)) = (&config.discord.server_id, &config.discord.rules_channel_id) {
        format!("Please check the rules in our dedicated channel: https://discord.com/channels/{}/{}", server_id, channel_id)
    } else {
        "Rules channel not configured. Please contact an administrator.".to_string()
    }
}

pub async fn handle_help_command() -> String {
    r#"**Available Commands:**

/guilds - Get guild raid ranks in the current addon.
       -season: Season number (1, 2, or 3, default is configurable).

/rank - Get player ranks in the current M+ season.            
       -top: Number of top players to display (1-50, default is 10).
       -guilds: Guilds to filter (all, guild names separated by ',').
       -classes: Player classes to filter (all or specific class).
       -role: Player role to filter (all, dps, healer, tank, or class:spec number).
       -rio: Minimum RIO score to display (0-3500, default is 2000).


/about_us - Learn more about us.

/rules - Rules.

/help - Get information about available commands.

Source code - https://github.com/CemXokenc/uawowguilds."#.to_string()
}

fn parse_class_spec(classes: &str) -> (String, Option<u8>) {
    if classes.contains(':') {
        let parts: Vec<&str> = classes.split(':').collect();
        if parts.len() == 2 {
            if let Ok(spec_num) = parts[1].parse::<u8>() {
                if (1..=4).contains(&spec_num) {
                    return (parts[0].to_string(), Some(spec_num));
                }
            }
        }
    }
    (classes.to_string(), None)
}

fn validate_class(class_name: &str) -> bool {
    let valid_classes = [
        "all", "death knight", "demon hunter", "druid", "evoker", 
        "hunter", "mage", "monk", "paladin", "priest", "rogue", 
        "shaman", "warlock", "warrior"
    ];
    valid_classes.contains(&class_name.to_lowercase().as_str())
}

fn validate_role(role_name: &str) -> bool {
    let valid_roles = ["all", "dps", "healer", "tank"];
    valid_roles.contains(&role_name.to_lowercase().as_str())
}

fn get_role_score(player: &PlayerData, role: &str) -> f64 {
    match role {
        "dps" => player.rio_dps.value(),
        "healer" => player.rio_healer.value(),
        "tank" => player.rio_tank.value(),
        _ => player.rio_all.value(),
    }
}

fn get_spec_score(player: &PlayerData, spec: u8) -> f64 {
    match spec {
        0 => player.spec_0.value(),
        1 => player.spec_1.value(),
        2 => player.spec_2.value(),
        3 => player.spec_3.value(),
        _ => 0.0,
    }
}

/// Helper function to truncate and pad strings to consistent length for monospace alignment
fn truncate_and_pad(s: &str, target_len: usize) -> String {
    if s.len() >= target_len {
        format!("{}...", &s[..target_len.saturating_sub(3)])
    } else {
        format!("{}{}", s, " ".repeat(target_len - s.len()))
    }
}

/// Convert DbMember to PlayerData for compatibility with existing logic
fn db_member_to_player_data(db_member: &DbMember) -> PlayerData {
    PlayerData {
        name: PlayerName::from(db_member.name.clone()),
        realm: RealmName::from(db_member.realm.clone()),
        guild: db_member.guild_name.as_ref().map(|g| GuildName::from(g.clone())),
        class: db_member.class.clone(),
        active_spec_name: db_member.spec.clone(),
        rio_all: MythicPlusScore::from(db_member.rio_all),
        rio_dps: MythicPlusScore::from(db_member.rio_dps),
        rio_healer: MythicPlusScore::from(db_member.rio_healer),
        rio_tank: MythicPlusScore::from(db_member.rio_tank),
        spec_0: MythicPlusScore::from(db_member.spec_0),
        spec_1: MythicPlusScore::from(db_member.spec_1),
        spec_2: MythicPlusScore::from(db_member.spec_2),
        spec_3: MythicPlusScore::from(db_member.spec_3),
    }
}