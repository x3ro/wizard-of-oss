mod errors;
mod models;
mod server;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, format_err, Context};
use axum::{Extension, Json};
use hyper::{Body, Response};
use lazy_static::lazy_static;
use rand::prelude::SliceRandom;
use slack_morphism::prelude::*;
use tracing::*;
use url::Url;

use crate::errors::AppError;
use crate::models::OpenSourceAttachment;

const RAW_LOADING_MESSAGES: &str = include_str!("../loading-messages.txt");
lazy_static! {
    static ref LOADING_MESSAGES: Vec<&'static str> = RAW_LOADING_MESSAGES.split('\n').collect();
}

const RECORD_HOURS_MODAL: &str = include_str!("../slack-ui/modal.json");
async fn open_oss_modal(state: &AppState, trigger_id: SlackTriggerId) {
    let req = SlackApiViewsOpenRequest {
        trigger_id,
        view: serde_json::from_str(RECORD_HOURS_MODAL).unwrap(),
    };

    state.get_session().views_open(&req).await.unwrap();
}

async fn report_user_stats(state: &AppState, config: &AppConfig, event: &SlackCommandEvent) {
    let req = SlackApiConversationsHistoryRequest {
        channel: Some(SlackChannelId(config.slack_oss_channel_id.clone())),
        cursor: None,
        latest: None,
        // TODO: Do we need to implement pagination here?
        //       Take a look at how the current application handles it.
        limit: Some(100),
        oldest: None,
        inclusive: None,
    };

    let res = state
        .get_session()
        .conversations_history(&req)
        .await
        .unwrap();

    let mut hours: HashMap<String, i16> = HashMap::new();

    for x in &res.messages {
        let Some(attachments) = &x.content.attachments else {
            continue;
        };

        let entries: Vec<anyhow::Result<OpenSourceAttachment>> = attachments
            .iter()
            .map(|x| x.fields.clone())
            .map(|x| {
                if let Some(fields) = x {
                    fields.try_into()
                } else {
                    Err(format_err!("No attachments"))
                }
            })
            .collect();

        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };

            let current = hours.get(&entry.username).unwrap_or(&0);
            hours.insert(entry.username.to_string(), current + entry.number_of_hours);
        }
    }

    let req = SlackApiChatPostEphemeralRequest {
        channel: SlackChannelId(config.slack_oss_channel_id.clone()),
        user: event.user_id.clone(),
        content: SlackMessageContent::new().with_text(format!("{:#?}", hours)),
        as_user: None,
        icon_emoji: None,
        icon_url: None,
        link_names: None,
        parse: None,
        thread_ts: None,
        username: None,
    };

    state.get_session().chat_post_ephemeral(&req).await.unwrap();
}

// --------
// Handlers
// --------

async fn test_oauth_install_function(
    resp: SlackOAuthV2AccessTokenResponse,
    _client: Arc<SlackHyperClient>,
    _states: SlackClientEventsUserState,
) {
    println!("HELLO AUTH {:#?}", resp);
    println!("token {}", resp.access_token.0);
}

// The install handlers aren't really needed if one wants
// to use the slack bot only for a single Slack installation,
// but they would become important if installation from the
// Slack app store should be supported.

async fn install_success_handler() -> String {
    info!("install_success_handler not implemented");
    "Welcome".to_string()
}

async fn install_cancel_handler() -> String {
    info!("install_cancel_handler not implemented");
    "Cancelled".to_string()
}

async fn install_error_handler() -> String {
    info!("install_error_handler not implemented");
    "Error while installing".to_string()
}

// -------------------------------------
// Here the important handlers begin vvv
// -------------------------------------

async fn push_event_handler(Extension(event): Extension<SlackPushEvent>) -> Response<Body> {
    trace!("Received push event: {:?}", event);

    match event {
        SlackPushEvent::UrlVerification(url_ver) => Response::new(Body::from(url_ver.challenge)),
        _ => Response::new(Body::empty()),
    }
}

async fn command_event_handler(
    Extension(event): Extension<SlackCommandEvent>,
    Extension(state): Extension<AppState>,
    Extension(config): Extension<AppConfig>,
) -> Result<Json<SlackCommandEventResponse>, AppError> {
    trace!("Received command event: {:?}", event);

    if event.command.as_ref() != "/woss" {
        return Err(anyhow!("Unknown command {}", event.command.as_ref()).into());
    }

    match event.text.as_deref() {
        Some("stats") => {
            tokio::spawn(async move {
                report_user_stats(&state, &config, &event).await
            });

            Ok(Json(loading_message()))
        }

        Some(_params) => {
            // TODO: Pre-fill form with parameters, and if all parameters are available,
            //       don't show form at all.
            tokio::spawn(async move {
                open_oss_modal(&state, event.trigger_id).await;
            });

            Ok(Json(loading_message()))
        }

        None => Ok(Json(SlackCommandEventResponse::new(
            SlackMessageContent::new().with_text("TODO: Usage information".into()),
        ))),
    }
}

fn loading_message() -> SlackCommandEventResponse {
    let message = LOADING_MESSAGES
        .choose(&mut rand::thread_rng())
        .unwrap_or(&"");

    let mut response = SlackCommandEventResponse::new(
        SlackMessageContent::new().with_text(format!("Please wait... {message}...")),
    );
    response.response_type = Some(SlackMessageResponseType::Ephemeral);
    response
}

/// This handler is called when the user initiates an action, such as
/// using a shortcut or submitting a form. See <https://api.slack.com/interactivity>
/// for details.
async fn interaction_event_handler(
    Extension(event): Extension<SlackInteractionEvent>,
    Extension(state): Extension<AppState>,
    Extension(config): Extension<AppConfig>,
) -> Result<String, AppError> {
    trace!("Received interaction event: {:?}", event);

    match event {
        SlackInteractionEvent::Shortcut(s) => match s.callback_id.as_ref() {
            "record_oss_hours" => {
                open_oss_modal(&state, s.trigger_id).await;
                Ok("".to_string())
            }

            callback_id => Err(anyhow!("Unknown short callback ID {callback_id}").into()),
        },

        SlackInteractionEvent::ViewSubmission(event) => {
            let Some(view_state) = event.view.state_params.state else {
                return Err(anyhow!("View submission did not contain state").into());
            };

            let number_of_hours = get_input_value(&view_state, "number_of_hours")?;
            let url = get_input_value(&view_state, "url")?;
            let description = get_input_value(&view_state, "description")?;
            let country = get_select_value(&view_state, "country")?;

            info!("Received a new submission: {number_of_hours} {url} '{description}' {country}");

            let parsed_hours = number_of_hours
                .parse::<i16>()
                .context("number_of_hours is not an i16")?;

            if parsed_hours <= 0 {
                return Err(AppError::InputValidationError {
                    field_name: "number_of_hours".to_string(),
                    message: "Number of hours must be greater than 0".to_string(),
                });
            }

            let parsed_url = Url::parse(url).map_err(|_err| AppError::InputValidationError {
                field_name: "url".to_string(),
                message: "Not a valid URL".to_string(),
            })?;

            if !parsed_url.scheme().starts_with("http") {
                return Err(AppError::InputValidationError {
                    field_name: "url".to_string(),
                    message: "URL should point to an HTTP or HTTPS resource".to_string(),
                });
            }

            let user_id = event.user.id;
            let user_req = SlackApiUsersInfoRequest {
                user: user_id,
                include_locale: None,
            };
            let res = state.get_session().users_info(&user_req).await.unwrap();

            let profile_image = res
                .user
                .profile
                .and_then(|profile| profile.icon)
                .and_then(|icon| icon.images)
                .and_then(|images| images.resolutions.last().cloned())
                .map(|resolution| resolution.1);

            let Some(username) = res.user.name else {
                return Err(anyhow!("The user information did not contain a username").into());
            };

            let attachment = OpenSourceAttachment {
                username: username.clone(),
                number_of_hours: parsed_hours,
                country: country.clone(),
                url: parsed_url,
                description: description.clone(),
            };

            let req = SlackApiChatPostMessageRequest {
                channel: SlackChannelId(config.slack_oss_channel_id.clone()),
                content: SlackMessageContent::new().with_attachments(vec![
                    SlackMessageAttachment {
                        id: None,
                        color: Some("good".to_string()),
                        fallback: None,
                        title: None,
                        fields: Some(attachment.into()),
                        mrkdwn_in: None,
                    },
                ]),
                as_user: None,
                icon_emoji: None,
                icon_url: profile_image,
                link_names: None,
                parse: None,
                thread_ts: None,
                username: Some(format!("{username} via Wizard of OSS")),
                reply_broadcast: None,
                unfurl_links: None,
                unfurl_media: None,
            };

            state.get_session().chat_post_message(&req).await.unwrap();

            Ok("".to_string())
        }

        _ => {
            error!("Received unknown interaction event: {:?}", event);
            return Err(anyhow!("Received unknown interaction event").into());
        }
    }
}

fn get_input_value(state: &SlackViewState, name: impl AsRef<str>) -> anyhow::Result<&String> {
    let id = name.as_ref();
    state
        .values
        .get(&id.into())
        .and_then(|x| x.get(&id.into()))
        .and_then(|x| x.value.as_ref())
        .ok_or_else(|| anyhow!("Missing field '{}'", name.as_ref()))
}

fn get_select_value(state: &SlackViewState, name: impl AsRef<str>) -> anyhow::Result<&String> {
    let id = name.as_ref();
    state
        .values
        .get(&id.into())
        .and_then(|x| x.get(&id.into()))
        .and_then(|x| x.selected_option.as_ref())
        .map(|x| &x.value)
        .ok_or_else(|| anyhow!("Missing select '{}'", name.as_ref()))
}

fn error_handler(
    err: Box<dyn std::error::Error + Send + Sync>,
    _client: Arc<SlackHyperClient>,
    _states: SlackClientEventsUserState,
) -> http::StatusCode {
    error!("{:#?}", err);
    http::StatusCode::BAD_REQUEST
}

#[derive(Clone, Debug)]
struct AppState {
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
