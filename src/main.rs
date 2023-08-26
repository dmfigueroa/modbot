pub mod db;
pub mod token;

use dotenv::dotenv;
use keyring::Keyring;
use sequelite::prelude::Connection;

use crate::db::Access;
use crate::token::start_server;

#[macro_use]
extern crate lazy_static;

#[tokio::main]
async fn main() {
    dotenv().ok();

    println!("Bot is starting");

    println!("Starting credentials server");

    let token_response = start_server().await.unwrap();

    println!("{:?}", token_response);
}

fn setup_db() {
    let mut conn = Connection::new("database.db").unwrap();

    conn.register::<Access>();
}
