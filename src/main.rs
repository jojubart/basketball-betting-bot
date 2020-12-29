use message_handling::run;
use teloxide::prelude::*;
use teloxide::requests::RequestWithFile;

mod message_handling;
mod states;
mod transitions;

#[tokio::main]
async fn main() {
    run().await;
}
