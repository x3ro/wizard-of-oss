mod errors;
mod models;
mod request_handlers;
mod server;
mod slack;

use std::sync::Arc;

use anyhow::Context;
use lazy_static::lazy_static;
use slack_morphism::prelude::*;

const RAW_LOADING_MESSAGES: &str = include_str!("../loading-messages.txt");
lazy_static! {
    static ref LOADING_MESSAGES: Vec<&'static str> = RAW_LOADING_MESSAGES.split('\n').collect();
}

#[derive(Clone, Debug)]
pub struct AppState {
    client: Arc<SlackHyperClient>,
    api_token: SlackApiToken,
}

impl AppState {
    // Sessions are lightweight and basically just a reference to client and token
    pub fn get_session(&self) -> SlackClientSession<SlackClientHyperHttpsConnector> {
        self.client.open_session(&self.api_token)
    }
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    port: u16,
    slack_client_id: String,
    slack_client_secret: String,
    slack_bot_scope: String,
    slack_redirect_host: String,
    slack_signing_secret: String,
    slack_test_token: String,
    slack_oss_channel_id: String,
}

impl AppConfig {
    fn from_env() -> Result<Self, anyhow::Error> {
        Ok(AppConfig {
            port: Self::env_var("PORT")?.parse()?,
            slack_client_id: Self::env_var("SLACK_CLIENT_ID")?,
            slack_client_secret: Self::env_var("SLACK_CLIENT_SECRET")?,
            slack_bot_scope: Self::env_var("SLACK_BOT_SCOPE")?,
            slack_redirect_host: Self::env_var("SLACK_REDIRECT_HOST")?,
            slack_signing_secret: Self::env_var("SLACK_SIGNING_SECRET")?,
            slack_test_token: Self::env_var("SLACK_TEST_TOKEN")?,
            slack_oss_channel_id: Self::env_var("SLACK_OSS_CHANNEL_ID")?,
        })
    }

    fn env_var(name: &str) -> Result<String, anyhow::Error> {
        std::env::var(name).with_context(|| format!("Couldn't find environment variable {}", name))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = AppConfig::from_env()?;

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter("slack_morphism=debug,oss_bot=trace")
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;
    server::start(config).await?;

    Ok(())
}
