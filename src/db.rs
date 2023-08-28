use chrono::NaiveDateTime;
use diesel::prelude::*;
use std::env;

#[derive(Debug, Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::access)]
pub struct Access {
    pub id: Option<i32>,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: NaiveDateTime,
}

pub fn establish_connection() -> SqliteConnection {
    let database_url: String = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    SqliteConnection::establish(&database_url)
        .unwrap_or_else(|_| panic!("Error connecting to {}", database_url))
}
