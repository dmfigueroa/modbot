#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = access)]
pub struct Access {
    id: Option<i32>,
    access_token: String,
    refresh_token: String,
    expires_in: Option<u64>,
}
