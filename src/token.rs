use std::collections::HashMap;

use std::env;
use std::time::Duration;

use chrono::{NaiveDateTime, Utc};
use hyper::header::LOCATION;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode};
use oauth2::{AccessToken, RefreshToken, Scope, Token, TokenType};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::db::{establish_connection, Access};
extern crate url;
use diesel::prelude::*;
use diesel::RunQueryDsl;

lazy_static! {
    static ref SCOPES: Vec<Scope> = vec![
        Scope::from("chat:read"),
        Scope::from("chat:edit"),
        Scope::from("channel:moderate"),
        Scope::from("channel:manage:moderators"),
        Scope::from("moderator:manage:banned_users"),
        Scope::from("user:read:email"),
    ];
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TwitchToken {
    access_token: AccessToken,
    token_type: TokenType,
    expires_at: NaiveDateTime,
    refresh_token: RefreshToken,
    scope: Vec<Scope>,
}

impl Token for TwitchToken {
    fn access_token(&self) -> &AccessToken {
        &self.access_token
    }

    fn token_type(&self) -> &TokenType {
        &self.token_type
    }

    fn expires_in(&self) -> Option<Duration> {
        let now = Utc::now().naive_utc();
        if self.expires_at > now {
            Some(Duration::new(
                (self.expires_at - now).num_milliseconds().unsigned_abs(),
                0,
            ))
        } else {
            None
        }
    }

    fn refresh_token(&self) -> Option<&RefreshToken> {
        Some(&self.refresh_token)
    }

    fn scopes(&self) -> Option<&Vec<Scope>> {
        Some(&self.scope)
    }
}

async fn twitch_auth() -> Result<Response<Body>, hyper::Error> {
    let client_id = env::var("TWITCH_CLIENT_ID").unwrap_or_default();
    let redirect_uri = format!(
        "{}/auth/callback",
        env::var("HOSTNAME_URL").unwrap_or_default()
    );

    let mut params = HashMap::new();
    params.insert("client_id", client_id);
    params.insert("redirect_uri", redirect_uri);
    params.insert("response_type", "code".to_string());
    let joined_scopes = SCOPES
        .iter()
        .map(|scope| scope.to_string())
        .collect::<Vec<String>>()
        .join(" ");
    params.insert("scope", joined_scopes);

    let url = format!(
        "https://id.twitch.tv/oauth2/authorize?{}",
        serde_urlencoded::to_string(&params).unwrap()
    );

    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    response
        .headers_mut()
        .insert(LOCATION, url.parse().unwrap());

    Ok(response)
}

// pub async fn get_token() -> TwitchToken {}

pub fn update_credentials(token: TwitchToken) -> Result<(), diesel::result::Error> {
    use crate::schema::access::dsl::{access, access_token, expires_at, id, refresh_token};

    println!("Storing credentials");

    let connection = &mut establish_connection();

    let not_exists: bool = access
        .filter(id.eq(1))
        .limit(1)
        .load::<Access>(connection)?
        .is_empty();

    if not_exists {
        diesel::insert_into(access)
            .values((
                id.eq(1),
                access_token.eq(token.access_token.to_string()),
                expires_at.eq(token.expires_at),
                refresh_token.eq(token.refresh_token.to_string()),
            ))
            .execute(connection)?;

        println!("New access added to the database");
    } else {
        diesel::update(access.filter(id.eq(1)))
            .set((
                access_token.eq(token.access_token.to_string()),
                expires_at.eq(token.expires_at),
                refresh_token.eq(token.refresh_token.to_string()),
            ))
            .execute(connection)?;
        println!("Access updated on the database");
    }

    Ok(())
}

async fn create_token_params(code: Option<String>) -> HashMap<&'static str, String> {
    let client_id = env::var("TWITCH_CLIENT_ID").expect("TWITCH_CLIENT_ID not set");
    let client_secret = env::var("TWITCH_CLIENT_SECRET").expect("TWITCH_CLIENT_SECRET not set");
    let redirect_uri = format!(
        "{}/auth/callback",
        env::var("HOSTNAME_URL").expect("HOSTNAME_URL not set")
    );

    let mut params = HashMap::new();
    params.insert("client_id", client_id);
    params.insert("client_secret", client_secret);
    params.insert("code", code.unwrap_or_default());
    params.insert("grant_type", "authorization_code".to_string());
    params.insert("redirect_uri", redirect_uri);

    params
}

async fn auth_callback(
    code: Option<String>,
    tx: mpsc::Sender<TwitchToken>,
) -> Result<Response<Body>, hyper::Error> {
    let params = create_token_params(code).await;
    let client = reqwest::Client::new();
    let response = client
        .post("https://id.twitch.tv/oauth2/token")
        .form(&params)
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let data: Value = resp.json().await.expect("Failed to parse JSON");
            let token = TwitchToken {
                access_token: AccessToken::from(
                    data["access_token"]
                        .as_str()
                        .expect("access_token not found")
                        .to_string(),
                ),
                refresh_token: RefreshToken::from(
                    data["refresh_token"]
                        .as_str()
                        .expect("refresh_token not found")
                        .to_string(),
                ),
                expires_at: Utc::now().naive_utc()
                    + chrono::Duration::seconds(data["expires_in"].as_i64().unwrap()),
                token_type: TokenType::Bearer,
                scope: SCOPES.to_vec(),
            };
            update_credentials(token.clone()).unwrap();
            tx.send(token).await.expect("Failed to send tokens");
            Ok(Response::new(Body::from(
                "Authentication was successful! You can close this window now.",
            )))
        }
        _ => Ok(Response::builder()
            .status(500)
            .body(Body::from("OAuth2 could not be obtained"))
            .expect("Failed to build response")),
    }
}

pub async fn start_server() -> Result<TwitchToken, Box<dyn std::error::Error>> {
    let (sender, mut receiver) = mpsc::channel(1);

    // Create a service function to handle incoming requests
    let make_svc = make_service_fn(move |_conn| {
        let sender_clone = sender.clone();
        let service = service_fn(move |req| handle_request(req, sender_clone.clone()));
        async { Ok::<_, hyper::Error>(service) }
    });

    let addr = ([127, 0, 0, 1], 3000).into();
    let server = Server::bind(&addr).serve(make_svc);

    println!("Open http://{}/auth/twitch to get your Twitch token", addr);

    // Start the server in a separate Tokio task
    tokio::spawn(async move {
        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
    });

    // Wait for the token from the receiver
    let token = receiver
        .recv()
        .await
        .ok_or_else(|| "Failed to receive token from the channel".to_string())?;

    Ok(token)
}

async fn handle_request(
    req: Request<Body>,
    token_sender: mpsc::Sender<TwitchToken>,
) -> Result<Response<Body>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&hyper::Method::GET, "/auth/twitch") => twitch_auth().await,
        (&hyper::Method::GET, "/auth/callback") => {
            let query = req.uri().query().unwrap_or_default();
            let params: HashMap<_, _> = url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect();
            auth_callback(params.get("code").cloned(), token_sender).await
        }
        _ => Ok(Response::new(Body::from("Not Found"))),
    }
}
