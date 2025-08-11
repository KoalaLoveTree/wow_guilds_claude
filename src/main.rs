use serenity::async_trait;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::application::command::Command;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::env;

mod raider_io;
mod guild_data;
mod tournament;
mod commands;
mod parser;

use commands::*;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let commands = Command::set_global_application_commands(&ctx.http, |commands| {
            commands
                .create_application_command(|command| guilds_command(command))
                .create_application_command(|command| rank_command(command))
                .create_application_command(|command| tournament_command(command))
                .create_application_command(|command| about_us_command(command))
                .create_application_command(|command| rules_command(command))
                .create_application_command(|command| help_command(command))
        })
        .await;

        println!("I created the following global slash commands: {:#?}", commands);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            println!("Received command: {}", command.data.name);
            
            // For simple commands, respond immediately
            let content = match command.data.name.as_str() {
                "about_us" => handle_about_us_command().await,
                "rules" => handle_rules_command().await,
                "help" => handle_help_command().await,
                _ => {
                    // For complex commands that might take time, defer the response
                    if let Err(why) = command
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(InteractionResponseType::DeferredChannelMessageWithSource)
                        })
                        .await
                    {
                        println!("Failed to defer response: {}", why);
                        return;
                    }

                    let content = match command.data.name.as_str() {
                        "guilds" => handle_guilds_command(&command).await,
                        "rank" => handle_rank_command(&command).await,
                        "tournament" => handle_tournament_command(&command).await,
                        _ => "Unknown command".to_string(),
                    };

                    // Send follow-up response
                    if let Err(why) = command
                        .create_followup_message(&ctx.http, |response| {
                            response.content(content)
                        })
                        .await
                    {
                        println!("Failed to send follow-up: {}", why);
                    }
                    return;
                }
            };

            // Immediate response for simple commands
            if let Err(why) = command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content(content))
                })
                .await
            {
                println!("Cannot respond to slash command: {}", why);
            }
        }
    }
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    
    let args: Vec<String> = env::args().collect();
    
    // Check if user wants to run the parser
    if args.len() > 1 && args[1] == "parse" {
        println!("Running parser to generate members.json...");
        match parser::generate_members_data().await {
            Ok(()) => println!("Parser completed successfully!"),
            Err(e) => eprintln!("Parser failed: {}", e),
        }
        return;
    }
    
    // Run Discord bot by default
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let mut client =
        Client::builder(&token, GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES)
            .event_handler(Handler)
            .await
            .expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}
