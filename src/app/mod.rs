mod cli;
mod conf;

use conf::MyConfig as Config;

use axum::{extract::State, response::Json};
use http::{HeaderName, HeaderValue};
use minijinja::{Environment, context};
use rootcause::prelude::*;
use rovo::{Router, routing::get, rovo};
use rovo::{
    aide,
    aide::{axum::IntoApiResponse, openapi::OpenApi},
};
use rovo::{schemars, schemars::JsonSchema};
use serde::Serialize;
use std::{fs, path::Path};
use tower_http::set_header::SetResponseHeaderLayer;

/// Structured exit codes (Unix-friendly, Windows-friendly)
#[derive(Debug)]
#[allow(dead_code)]
pub enum ExitCode {
    Ok = 0,
    InitFailure = 10,
    //AppFailure = 20,
    //Crash = 30,
}

#[derive(Clone)]
struct AppState {}

#[derive(Serialize, JsonSchema)]
struct User {
    id: u64,
    name: String,
}

/// Get user information.
///
/// Returns the current user's profile information.
///
/// @tag users
/// @response 200 Json<User> User profile retrieved successfully.
#[rovo]
async fn get_user(State(_state): State<AppState>) -> impl IntoApiResponse {
    Json(User {
        id: 1,
        name: "Alice".to_string(),
    })
}

pub async fn run() -> Result<(), Report> {
    tracing::debug!("Starting app logic");

    // Parse the arguments from the command line
    let mycli = cli::parse_cli();

    // Load config
    let toml: String = conf::load_config()?;
    let config: Config = toml::from_str(&toml)?;

    tracing::info!("Loaded config: {:#?}", config);

    let mut env = Environment::new();

    let template_path = Path::new("templates/layout.html");
    let template_content = fs::read_to_string(template_path)?;
    env.add_template("layout.html", &template_content)?;

    let hello_path = Path::new("templates/hello.tmpl");
    let hello_content = fs::read_to_string(hello_path)?;
    env.add_template("hello.tmpl", &hello_content)?;

    let template = env.get_template("hello.tmpl").unwrap();
    let name: String = match mycli.name {
        Some(name) => name,
        None => config.name,
    };

    println!("{}", template.render(context! { name => name }).unwrap());

    let state = AppState {};

    let mut api = OpenApi::default();
    api.info.title = "My API".to_string();

    let app = Router::new()
        .route("/user", get(get_user))
        .with_oas(api)
        .with_redoc("/")
        .with_swagger("/swagger")
        .with_scalar("/scalar")
        .with_state(state);

    // Convert into an Axum router:
    let mut app: axum::Router<_> = app.into();

    // Now you can add layers:
    app = app.layer(SetResponseHeaderLayer::overriding(
        HeaderName::from_static("x-hello-world"),
        HeaderValue::from_str(&name).unwrap(),
    ));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            // An async function that waits for the Ctrl+C signal.
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");

            if mycli.verbose {
                tracing::info!("\n\nReceived Ctrl+C, starting graceful shutdown...");
            } else {
                tracing::debug!("\n\nReceived Ctrl+C, starting graceful shutdown...");
            }
        })
        .await
        .unwrap();

    Ok(())
}
