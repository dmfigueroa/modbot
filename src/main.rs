pub mod token;

use dotenv::dotenv;
use oauth2::Token;

use crate::token::start_server;

#[macro_use]
extern crate lazy_static;

#[tokio::main]
async fn main() {
    dotenv().ok();

    println!("Bot is starting");

    println!("Starting credentials server");

    let token_response = start_server().await.unwrap();

    println!("{:?}", token_response)
}
