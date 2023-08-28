// @generated automatically by Diesel CLI.

diesel::table! {
    access (id) {
        id -> Nullable<Integer>,
        access_token -> Text,
        refresh_token -> Text,
        expires_at -> Timestamp,
    }
}
