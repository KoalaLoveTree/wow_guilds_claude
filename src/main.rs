/// WoW Guild Discord Bot - A Rust implementation for guild progression tracking
use serenity::async_trait;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::application::command::Command;
use serenity::model::gateway::Ready;
use serenity::model::guild::Member;
use serenity::model::id::RoleId;
use serenity::prelude::*;
use std::env;
use tracing::{error, info, warn};

// Module declarations
mod commands;
mod config;
mod database;
mod error;
mod guild_data;
mod logging;
mod parser;
mod raider_io;
mod types;

// Re-exports for convenience
use crate::config::AppConfig;
use crate::database::Database;
use crate::error::{BotError, Result};

// Logging macros
macro_rules! log_api_request {
    ($method:expr, $url:expr, $status:expr, duration = $duration:expr) => {
        tracing::info!(
            method = %$method,
            url = %$url,
            status = $status,
            duration_ms = $duration,
            "API request completed"
        );
    };
}

macro_rules! log_discord_command {
    ($command:expr, $user_id:expr) => {
        tracing::info!(
            command = %$command,
            user_id = $user_id,
            "Discord command received"
        );
    };
}

/// Discord event handler
struct Handler {
    config: AppConfig,
    database: Database,
}

impl Handler {
    fn new(config: AppConfig, database: Database) -> Self {
        Self { config, database }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(bot_name = %ready.user.name, "Discord bot connected and ready");

        let commands = Command::set_global_application_commands(&ctx.http, |commands| {
            commands
                .create_application_command(|command| commands::guilds_command(command))
                .create_application_command(|command| commands::rank_command(command))
                .create_application_command(|command| commands::about_us_command(command))
                .create_application_command(|command| commands::rules_command(command))
                .create_application_command(|command| commands::help_command(command))
        })
        .await;

        match commands {
            Ok(commands) => {
                info!(registered_commands = commands.len(), "Slash commands registered successfully");
                for cmd in &commands {
                    info!(command_name = %cmd.name, "Command registered: {}", cmd.name);
                }
            },
            Err(e) => {
                error!(error = %e, "Failed to register slash commands");
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            let command_name = &command.data.name;
            let user_id = command.user.id;

            crate::log_discord_command!(command_name, user_id.0);
            
            // For simple commands, respond immediately
            let content = match command_name.as_str() {
                "about_us" => commands::handle_about_us_command().await,
                "rules" => commands::handle_rules_command(&self.config).await,
                "help" => commands::handle_help_command().await,
                _ => {
                    // For complex commands that might take time, defer the response
                    if let Err(why) = command
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(InteractionResponseType::DeferredChannelMessageWithSource)
                        })
                        .await
                    {
                        error!(command = %command_name, error = %why, "Failed to defer response");
                        return;
                    }

                    info!("Executing command: {}", command_name);

                    match command_name.as_str() {
                        "guilds" => {
                            info!("Executing guilds command...");
                            let content = commands::handle_guilds_command(&command, &self.config).await;
                            
                            // Send follow-up response
                            if let Err(why) = command
                                .create_followup_message(&ctx.http, |response| {
                                    response.content(&content)
                                })
                                .await
                            {
                                error!(command = %command_name, error = %why, "Failed to send follow-up");
                            } else {
                                info!(command = %command_name, user = user_id.0, response_length = content.len(), "Command completed successfully");
                            }
                        },
                        "rank" => {
                            let messages = commands::handle_rank_command_multi(&command, &self.database).await;
                            
                            // Send first message as follow-up
                            if let Some(first_message) = messages.first() {
                                if let Err(why) = command
                                    .create_followup_message(&ctx.http, |response| {
                                        response.content(first_message)
                                    })
                                    .await
                                {
                                    error!(command = %command_name, error = %why, "Failed to send follow-up");
                                    return;
                                }
                            }
                            
                            // Send additional messages as separate follow-ups
                            for (i, message) in messages.iter().skip(1).enumerate() {
                                if let Err(why) = command
                                    .create_followup_message(&ctx.http, |response| {
                                        response.content(message)
                                    })
                                    .await
                                {
                                    error!(command = %command_name, message_index = i + 2, error = %why, "Failed to send additional follow-up message");
                                } else {
                                    info!(command = %command_name, message_index = i + 2, "Additional follow-up message sent successfully");
                                }
                            }
                            
                            let total_length: usize = messages.iter().map(|m| m.len()).sum();
                            info!(command = %command_name, user = user_id.0, messages_sent = messages.len(), total_length = total_length, "Command completed successfully");
                        },
                        _ => {
                            warn!(command = %command_name, "Unknown command received");
                            let content = "❓ Unknown command".to_string();
                            
                            // Send follow-up response
                            if let Err(why) = command
                                .create_followup_message(&ctx.http, |response| {
                                    response.content(&content)
                                })
                                .await
                            {
                                error!(command = %command_name, error = %why, "Failed to send follow-up");
                            } else {
                                info!(command = %command_name, user = user_id.0, response_length = content.len(), "Command completed successfully");
                            }
                        }
                    };
                    return;
                }
            };

            // Immediate response for simple commands
            if let Err(why) = command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content(&content))
                })
                .await
            {
                error!(command = %command_name, error = %why, "Cannot respond to slash command");
            } else {
                info!(command = %command_name, user = user_id.0, response_length = content.len(), "Command completed successfully");
            }
        }
    }

    async fn guild_member_addition(&self, ctx: Context, mut new_member: Member) {
        // Check if auto-role assignment is enabled
        if !self.config.discord.auto_role_enabled {
            return;
        }

        // Get the role ID from config
        let Some(role_id_str) = &self.config.discord.auto_role_id else {
            warn!("Auto-role is enabled but no role ID configured");
            return;
        };

        // Parse role ID
        let role_id = match role_id_str.parse::<u64>() {
            Ok(id) => RoleId(id),
            Err(e) => {
                error!("Failed to parse auto-role ID '{}': {}", role_id_str, e);
                return;
            }
        };

        info!(
            user = %new_member.user.name,
            user_id = new_member.user.id.0,
            guild = %new_member.guild_id,
            role_id = role_id.0,
            "New member joined, assigning auto-role"
        );

        // Check if user already has the role (shouldn't happen for new members, but safety check)
        if new_member.roles.contains(&role_id) {
            info!(
                user = %new_member.user.name,
                role_id = role_id.0,
                "User already has the auto-role, skipping"
            );
            return;
        }

        // Assign the role
        match new_member.add_role(&ctx.http, role_id).await {
            Ok(()) => {
                info!(
                    user = %new_member.user.name,
                    user_id = new_member.user.id.0,
                    role_id = role_id.0,
                    "Successfully assigned auto-role to new member"
                );
            }
            Err(e) => {
                error!(
                    user = %new_member.user.name,
                    user_id = new_member.user.id.0,
                    role_id = role_id.0,
                    error = %e,
                    "Failed to assign auto-role to new member"
                );
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    // Load configuration
    let config = AppConfig::load()?;
    
    // Initialize logging
    logging::init_logging(&config.logging)?;
    info!("WoW Guild Bot starting up...");

    // Initialize database (migrations will populate guild data automatically)
    let database = Database::new(&config.database.url).await?;

    let args: Vec<String> = env::args().collect();
    
    // Check if user wants to run the parser
    if args.len() > 1 && args[1] == "parse" {
        info!("Running parser to generate members.json...");
        match parser::generate_members_data().await {
            Ok(()) => {
                info!("Parser completed successfully!");
                Ok(())
            },
            Err(e) => {
                error!(error = %e, "Parser failed");
                Err(BotError::from(e))
            }
        }
    } else if args.len() > 1 && args[1] == "db-status" {
        // Show database status and migrations
        show_database_status(&database).await?;
        Ok(())
    } else {
        // Run Discord bot
        run_discord_bot(config, database).await
    }
}

/// Show database status and migrations
async fn show_database_status(database: &Database) -> Result<()> {
    info!("=== Database Status ===");
    
    // Get stats
    let (guild_count, member_count) = database.get_stats().await?;
    info!("📊 Guilds: {}", guild_count);
    info!("👥 Members: {}", member_count);
    
    info!("\n=== Executed Migrations ===");
    match database.get_migrations().await {
        Ok(migrations) => {
            for (name, executed_at) in migrations {
                info!("✅ {} (executed: {})", name, executed_at.format("%Y-%m-%d %H:%M:%S UTC"));
            }
        },
        Err(e) => {
            warn!("Could not fetch migrations: {}", e);
        }
    }
    
    info!("\n=== Database Tables ===");
    info!("📋 _migrations - Migration tracking");
    info!("🏰 guilds - Guild data (62 guilds from migration)");
    info!("👤 members - Active member data with complete RIO stats (rio_all, rio_dps, rio_healer, rio_tank, spec_0-3)");
    info!("🔄 members_tmp - Temporary member data for parsing workflow with same complete structure");
    
    Ok(())
}

/// Run the Discord bot with the given configuration
async fn run_discord_bot(config: AppConfig, database: Database) -> Result<()> {
    info!("Starting Discord bot...");

    let intents = GatewayIntents::GUILD_MESSAGES 
        | GatewayIntents::DIRECT_MESSAGES 
        | GatewayIntents::GUILD_MEMBERS;  // Enable after setting up intents in Discord Portal

    let mut client = Client::builder(&config.discord.token, intents)
        .event_handler(Handler::new(config, database))
        .await
        .map_err(|e| BotError::Discord(e))?;

    info!("Discord client created successfully, starting event loop...");

    client.start().await.map_err(|e| {
        error!(error = %e, "Discord client error");
        BotError::Discord(e)
    })?;

    Ok(())
}