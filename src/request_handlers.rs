use std::sync::Arc;

use anyhow::{anyhow, Context};
use axum::{Extension, Json};
use hyper::{Body, Response};
use lazy_static::lazy_static;
use rand::prelude::SliceRandom;
use slack_morphism::prelude::*;
use tracing::*;
use url::Url;

use crate::errors::AppError;
use crate::models::OpenSourceAttachment;
use crate::{slack, AppConfig, AppState};
use crate::slack::SlackViewStateExt;

const RAW_LOADING_MESSAGES: &str = include_str!("../loading-messages.txt");
lazy_static! {
    static ref LOADING_MESSAGES: Vec<&'static str> = RAW_LOADING_MESSAGES.split('\n').collect();
}

// --------
// Handlers
// --------

pub async fn test_oauth_install_function(
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

pub async fn install_success_handler() -> String {
    info!("install_success_handler not implemented");
    "Welcome".to_string()
}

pub async fn install_cancel_handler() -> String {
    info!("install_cancel_handler not implemented");
    "Cancelled".to_string()
}

pub async fn install_error_handler() -> String {
    info!("install_error_handler not implemented");
    "Error while installing".to_string()
}

// -------------------------------------
// Here the important handlers begin vvv
// -------------------------------------

pub async fn push_event_handler(Extension(event): Extension<SlackPushEvent>) -> Response<Body> {
    trace!("Received push event: {:?}", event);

    match event {
        SlackPushEvent::UrlVerification(url_ver) => Response::new(Body::from(url_ver.challenge)),
        _ => Response::new(Body::empty()),
    }
}

pub async fn command_event_handler(
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
            tokio::spawn(async move { slack::report_user_stats(&state, &config, &event).await });

            Ok(Json(loading_message()))
        }

        Some(_params) => {
            // TODO: Pre-fill form with parameters, and if all parameters are available,
            //       don't show form at all.
            tokio::spawn(async move {
                slack::open_oss_modal(&state, event.trigger_id).await;
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
pub async fn interaction_event_handler(
    Extension(event): Extension<SlackInteractionEvent>,
    Extension(state): Extension<AppState>,
    Extension(config): Extension<AppConfig>,
) -> Result<String, AppError> {
    trace!("Received interaction event: {:?}", event);

    match event {
        SlackInteractionEvent::Shortcut(s) => match s.callback_id.as_ref() {
            "record_oss_hours" => {
                slack::open_oss_modal(&state, s.trigger_id).await;
                Ok("".to_string())
            }

            callback_id => Err(anyhow!("Unknown short callback ID {callback_id}").into()),
        },

        SlackInteractionEvent::ViewSubmission(event) => {
            let Some(view_state) = event.view.state_params.state else {
                return Err(anyhow!("View submission did not contain state").into());
            };

            let number_of_hours = view_state.input_value("number_of_hours")?;
            let url = view_state.input_value("url")?;
            let description = view_state.input_value("description")?;
            let country = view_state.select_value( "country")?;

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

            let parsed_url = Url::parse(&url).map_err(|_err| AppError::InputValidationError {
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

pub fn error_handler(
    err: Box<dyn std::error::Error + Send + Sync>,
    _client: Arc<SlackHyperClient>,
    _states: SlackClientEventsUserState,
) -> http::StatusCode {
    error!("{:#?}", err);
    http::StatusCode::BAD_REQUEST
}
