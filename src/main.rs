pub mod db;
pub mod schema;
pub mod token;

use dotenv::dotenv;

use crate::token::get_token;

#[macro_use]
extern crate lazy_static;

#[tokio::main]
async fn main() {
    dotenv().ok();

    println!("Bot is starting");

    println!("Starting credentials server");

    let token_response = get_token().await.unwrap();

    println!("{:?}", token_response);
}
