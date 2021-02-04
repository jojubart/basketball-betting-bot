#![warn(clippy::all)]

use message_handling::run;
use teloxide::prelude::*;
use teloxide::requests::RequestWithFile;

mod message_handling;
mod states;
mod transitions;

#[tokio::main]
async fn main() {
    simple_logging::log_to_file("bot.log", log::LevelFilter::Info).unwrap();
    log::info!("Bot was started at {now}", now = chrono::Utc::now());
    run().await;
}
