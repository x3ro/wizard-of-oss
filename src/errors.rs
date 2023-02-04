use axum::response::IntoResponse;
use axum::Json;
use http::StatusCode;
use serde_json::json;
use tracing::{debug, error};

pub enum AppError {
    InternalServerError(anyhow::Error),
    InputValidationError { field_name: String, message: String },
}

impl From<anyhow::Error> for AppError {
    fn from(inner: anyhow::Error) -> Self {
        AppError::InternalServerError(inner)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        use AppError::*;

        let (status, body) = match self {
            InternalServerError(inner) => {
                error!("Returning error response: {:?}", inner);

                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({
                        "error": "Something went wrong. See logs for details.",
                    }),
                )
            }

            InputValidationError {
                field_name,
                message,
            } => {
                debug!("Rejecting submission because of validation error. {field_name}: {message}");

                (
                    StatusCode::OK,
                    json!({
                        "response_action": "errors",
                        "errors": {
                            field_name: message,
                        }
                    }),
                )
            }
        };

        (status, Json(body)).into_response()
    }
}
