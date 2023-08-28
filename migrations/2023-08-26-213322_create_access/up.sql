-- Your SQL goes here
CREATE TABLE access (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    access_token VARCHAR NOT NULL,
    refresh_token VARCHAR NOT NULL,
    expires_at DATETIME NOT NULL
)