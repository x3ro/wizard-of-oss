use std::sync::Arc;

use anyhow::{Context, Result};
use redis::AsyncCommands;
use slack_morphism::SlackUserId;

use crate::errors::AppError;
use crate::AppConfig;

#[derive(Clone, Debug)]
pub struct Persistence {
    redis: Arc<redis::Client>,
}

impl Persistence {
    pub async fn new(config: &AppConfig) -> Result<Self> {
        let redis = Arc::new(redis::Client::open(config.redis_url.clone())?);

        // Make sure that we can connect to the redis instance before proceeding
        redis
            .get_async_connection()
            .await
            .with_context(|| "Failed to start server, redis connection failed")?;

        Ok(Persistence { redis })
    }

    async fn get_redis_connection(&self) -> Result<redis::aio::Connection, AppError> {
        self.redis.get_async_connection().await.map_err(|err| {
            tracing::error!("Failed to acquire a redis connection: {err}");
            err.into()
        })
    }

    pub async fn get_default_country(&self, user_id: SlackUserId) -> Option<String> {
        let Some(mut conn) = self.get_redis_connection().await.ok() else {
            return None;
        };
        conn.get::<_, String>(user_id.0.clone()).await.ok()
    }

    pub async fn set_default_country(
        &self,
        user_id: SlackUserId,
        country: String,
    ) -> Result<(), AppError> {
        let mut conn = self.get_redis_connection().await?;
        conn.set(user_id.0, country).await.map_err(|err| err.into())
    }
}
