use std::sync::Arc;

use axum::Extension;
use slack_morphism::prelude::*;
use tracing::*;

use crate::persistence::Persistence;
use crate::request_handlers::{
    command_event_handler, error_handler, install_cancel_handler, install_error_handler,
    install_success_handler, interaction_event_handler, push_event_handler,
    test_oauth_install_function,
};
use crate::{AppConfig, AppState};

pub async fn start(config: AppConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config_extension = Extension(config.clone());
    let token_value: SlackApiTokenValue = config.slack_test_token.clone().into();
    let api_token: SlackApiToken = SlackApiToken::new(token_value);

    let client: Arc<SlackHyperClient> =
        Arc::new(SlackClient::new(SlackClientHyperConnector::new()));

    let app_state = AppState {
        client: client.clone(),
        api_token,
        persistence: Persistence::new(&config).await?,
    };

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Starting server: {}", addr);

    let oauth_listener_config = SlackOAuthListenerConfig::new(
        config.slack_client_id.into(),
        config.slack_client_secret.into(),
        config.slack_bot_scope,
        config.slack_redirect_host,
    );

    let listener_environment: Arc<SlackHyperListenerEnvironment> = Arc::new(
        SlackClientEventsListenerEnvironment::new(client.clone()).with_error_handler(error_handler),
    );
    let signing_secret: SlackSigningSecret = config.slack_signing_secret.into();

    let listener: SlackEventsAxumListener<SlackHyperHttpsConnector> =
        SlackEventsAxumListener::new(listener_environment.clone());

    // build our application route with OAuth nested router and Push/Command/Interaction events
    let app = axum::routing::Router::new()
        .nest(
            "/auth",
            listener.oauth_router("/auth", &oauth_listener_config, test_oauth_install_function),
        )
        .route("/installed", axum::routing::get(install_success_handler))
        .route("/cancelled", axum::routing::get(install_cancel_handler))
        .route("/error", axum::routing::get(install_error_handler))
        .route(
            "/push",
            axum::routing::post(push_event_handler).layer(
                listener
                    .events_layer(&signing_secret)
                    .with_event_extractor(SlackEventsExtractors::push_event()),
            ),
        )
        .route(
            "/command",
            axum::routing::post(command_event_handler).layer(
                listener
                    .events_layer(&signing_secret)
                    .with_event_extractor(SlackEventsExtractors::command_event()),
            ),
        )
        .route(
            "/interactivity",
            axum::routing::post(interaction_event_handler).layer(
                listener
                    .events_layer(&signing_secret)
                    .with_event_extractor(SlackEventsExtractors::interaction_event()),
            ),
        )
        .layer(Extension(app_state))
        .layer(config_extension);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}
