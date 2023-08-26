use std::collections::HashMap;

use std::env;
use std::time::Duration;

use hyper::header::LOCATION;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode};
use oauth2::{AccessToken, RefreshToken, Scope, Token, TokenType};
use serde_json::Value;
use tokio::sync::mpsc;
extern crate url;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<RefreshToken>,
    #[serde(rename = "scope")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    scopes: Option<Vec<Scope>>,
}

impl Token for TwitchToken {
    fn access_token(&self) -> &AccessToken {
        &self.access_token
    }

    fn token_type(&self) -> &TokenType {
        &self.token_type
    }

    fn expires_in(&self) -> Option<Duration> {
        self.expires_in.map(Duration::from_secs)
    }

    fn refresh_token(&self) -> Option<&RefreshToken> {
        self.refresh_token.as_ref()
    }

    fn scopes(&self) -> Option<&Vec<Scope>> {
        self.scopes.as_ref()
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

pub async fn update_credentials(_token: TwitchToken) {
    // Implement the logic to update credentials here
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
                refresh_token: Some(RefreshToken::from(
                    data["refresh_token"]
                        .as_str()
                        .expect("refresh_token not found")
                        .to_string(),
                )),
                expires_in: data["expires_in"].as_u64(),
                token_type: TokenType::Bearer,
                scopes: Some(SCOPES.to_vec()),
            };
            update_credentials(token.clone()).await;
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
