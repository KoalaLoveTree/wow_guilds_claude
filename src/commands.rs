use serenity::builder::CreateApplicationCommand;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::command::CommandOptionType;
use crate::guild_data::{fetch_all_guild_data, sort_guilds, format_guild_list};
use crate::tournament::{read_members_data, get_tournament_players, fetch_tournament_data_from_sheets};
use crate::raider_io::PlayerData;

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

pub fn tournament_command(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command
        .name("tournament")
        .description("Get top players in a guild for a tournament")
        .create_option(|option| {
            option
                .name("guild")
                .description("Guild name for the tournament")
                .kind(CommandOptionType::String)
                .required(false)
        })
        .create_option(|option| {
            option
                .name("top")
                .description("Number of players to display (default: 5)")
                .kind(CommandOptionType::Integer)
                .required(false)
        })
        .create_option(|option| {
            option
                .name("format")
                .description("Data source format: new or old (default: new)")
                .kind(CommandOptionType::String)
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

pub async fn handle_guilds_command(command: &ApplicationCommandInteraction) -> String {
    let season = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "season")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_i64()))
        .unwrap_or(2) as u8;

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

    match fetch_all_guild_data(season).await {
        Ok(guilds) => {
            if guilds.is_empty() {
                format!("At the moment, there are no guilds with progression in season {}.", season)
            } else {
                let sorted_guilds = sort_guilds(guilds);
                format_guild_list(&sorted_guilds, limit)
            }
        }
        Err(e) => {
            eprintln!("Error fetching guild data: {}", e);
            format!("An error occurred while fetching guild data: {}. Please check that uaguildlist.txt exists and contains valid guild URLs.", e)
        }
    }
}

pub async fn handle_rank_command(command: &ApplicationCommandInteraction) -> String {
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
        return "Error: The value of top must be between 1 and 50 inclusive.".to_string();
    }

    if rio > 3500 {
        return "Error: The value of rio must be between 0 and 3500 inclusive.".to_string();
    }

    // Validate class and role like Python version
    let (class_filter, spec_number) = parse_class_spec(classes);
    
    if !validate_class(&class_filter) {
        return format!("Class '{}' does not exist. Use the valid classes: all, death knight, demon hunter, druid, evoker, hunter, mage, monk, paladin, priest, rogue, shaman, warlock, warrior.", class_filter);
    }
    
    if !validate_role(role) {
        return format!("Role '{}' does not exist. Use the valid roles: all, dps, healer, tank.", role);
    }

    match read_members_data("members.json") {
        Ok(mut players) => {
            println!("Loaded {} players from members.json", players.len());
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
                    b_score.cmp(&a_score)
                });
                players.retain(|p| get_spec_score(p, spec - 1) > rio);
            } else {
                // Role-based filtering - sort by role-specific RIO
                if role != "all" {
                    players.sort_by(|a, b| {
                        let a_score = get_role_score(a, role);
                        let b_score = get_role_score(b, role);
                        b_score.cmp(&a_score)
                    });
                } else {
                    players.sort_by(|a, b| {
                        let a_score = a.rio_all;
                        let b_score = b.rio_all;
                        b_score.cmp(&a_score)
                    });
                }
                
                // Filter by role-specific RIO (exactly like Python)
                let before_count = players.len();
                if role != "all" {
                    players.retain(|p| get_role_score(p, role) > rio);
                } else {
                    players.retain(|p| p.rio_all > rio);
                }
                println!("After RIO filter (>{} for role '{}'): {} players (was {})", rio, role, players.len(), before_count);
            }

            players.truncate(top);

            if players.is_empty() {
                return "No players found matching the criteria.".to_string();
            }

            let header = format!(
                "Top {} | Classes -> {} | Guilds -> {} | Role -> {} | Rio > {}",
                top, classes, guilds, role, rio
            );

            let mut result = format!("{}\n{}\n", header, "-".repeat(60));

            for (i, player) in players.iter().enumerate() {
                let (display_role, score) = if let Some(spec) = spec_number {
                    // For spec-based, show the role but use spec score
                    (role.to_string(), get_spec_score(player, spec - 1))
                } else if role != "all" {
                    // For role-specific, show role and use role score
                    (role.to_string(), get_role_score(player, role))
                } else {
                    // For "all", show "all" and use rio_all
                    ("all".to_string(), player.rio_all)
                };

                result.push_str(&format!(
                    "{}. {} ({}, {}) - {} {} - RIO {}: {}\n",
                    i + 1,
                    player.name,
                    player.guild.as_deref().unwrap_or("N/A"),
                    player.realm,
                    player.active_spec_name.as_deref().unwrap_or("Unknown"),
                    player.class.as_deref().unwrap_or("Unknown"),
                    display_role,
                    score
                ));
            }

            result
        }
        Err(e) => {
            format!("No data to process: {}. Complete the 'members.json' file before using this command.", e)
        }
    }
}

pub async fn handle_tournament_command(command: &ApplicationCommandInteraction) -> String {
    let guild = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "guild")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_str()))
        .unwrap_or("Нехай Щастить");

    let top = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "top")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_i64()))
        .unwrap_or(5) as usize;

    let format = command
        .data
        .options
        .iter()
        .find(|opt| opt.name == "format")
        .and_then(|opt| opt.value.as_ref().and_then(|v| v.as_str()))
        .unwrap_or("new");

    let players = if format == "new" {
        match fetch_tournament_data_from_sheets(
            "https://docs.google.com/spreadsheets/d/1YdZRWVXzOXaIZfb9YXDfqHEaeaVnv_3j4EykUZ4Kf4E/export?format=csv"
        ).await {
            Ok(players) => players,
            Err(e) => return format!("Error fetching tournament data from Google Sheets: {}", e),
        }
    } else {
        match read_members_data("members.json") {
            Ok(players) => players,
            Err(e) => return format!("No data available in members.json: {}", e),
        }
    };

    if players.is_empty() {
        return "No player data available.".to_string();
    }

    let guild_filter = if format == "old" { Some(guild) } else { None };
    let roster = get_tournament_players(&players, guild_filter, top);
    
    format!("Top {} Players for the Tournament:\n\n{}", top, roster.format(format == "new"))
}

pub async fn handle_about_us_command() -> String {
    "https://youtu.be/xvpVTd1gt5Q".to_string()
}

pub async fn handle_rules_command() -> String {
    "https://cdn.discordapp.com/attachments/786720808788688918/1202356554523742289/image.png?ex=65e8d84d&is=65d6634d&hm=dee787e24cb77005a58568556547af37a24fe98bfcb11c1f6ecabc1bf72842ff&".to_string()
}

pub async fn handle_help_command() -> String {
    r#"**Available Commands:**

/guilds - Get guild raid ranks in the current addon.
       -season: Season number (1, 2, or 3, default is 2).

/rank - Get player ranks in the current M+ season.            
       -top: Number of top players to display (1-50, default is 10).
       -guilds: Guilds to filter (all, guild names separated by ',').
       -classes: Player classes to filter (all or specific class).
       -role: Player role to filter (all, dps, healer, tank, or class:spec number).
       -rio: Minimum RIO score to display (0-3500, default is 2000).

/tournament - Get top players in each category.            
       -guild: Top players of which guild will be searched.
       -top: Top X players.

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

fn get_role_score(player: &PlayerData, role: &str) -> u32 {
    match role {
        "dps" => player.rio_dps,
        "healer" => player.rio_healer,
        "tank" => player.rio_tank,
        _ => player.rio_all,
    }
}

fn get_spec_score(player: &PlayerData, spec: u8) -> u32 {
    match spec {
        0 => player.spec_0,
        1 => player.spec_1,
        2 => player.spec_2,
        3 => player.spec_3,
        _ => 0,
    }
}