use std::time::SystemTime;

use anyhow::Context;
use reqwest::{header::HeaderValue, Body, Method, Request, Url};
use reqwest_middleware::ClientWithMiddleware;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{TokenInfo, ValueHelper};

fn generate_oauth_header(token_info: &TokenInfo) -> anyhow::Result<String> {
    let TokenInfo {
        client_token,
        client_secret,
        user_token,
        user_secret,
    } = token_info;
    let user_token = user_token.as_ref().map(|x| x.as_str());
    let user_secret = user_secret.as_ref().map(|x| x.as_str());
    let nonce = Uuid::new_v4();
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("failed to get system time")?
        .as_secs();
    Ok(format!("OAuth realm=\"Schoology API\",oauth_consumer_key=\"{}\",oauth_token=\"{}\",oauth_nonce=\"{}\",oauth_timestamp=\"{}\",oauth_signature_method=\"PLAINTEXT\",oauth_version=\"1.0\",oauth_signature=\"{}%26{}\"", client_token, user_token.unwrap_or(""), nonce, timestamp, client_secret, user_secret.unwrap_or("")))
}

pub async fn get(
    client: &ClientWithMiddleware,
    token_info: &TokenInfo,
    url: &str,
) -> anyhow::Result<Value> {
    Ok(client
        .execute(Request::get(url)?.into_schoology(token_info)?)
        .await?
        .json()
        .await?)
}

pub async fn get_raw(
    client: &ClientWithMiddleware,
    token_info: &TokenInfo,
    url: &str,
) -> anyhow::Result<Value> {
    Ok(client
        .execute(Request::get_raw(url)?.into_schoology(token_info)?)
        .await?
        .json()
        .await?)
}

pub trait SchoologyRequestHelper {
    fn get(url: &str) -> anyhow::Result<Self>
    where
        Self: Sized;
    fn get_raw(url: &str) -> anyhow::Result<Self>
    where
        Self: Sized;
    fn into_schoology(self, token_info: &TokenInfo) -> anyhow::Result<Self>
    where
        Self: Sized;
}

impl SchoologyRequestHelper for Request {
    fn get(url: &str) -> anyhow::Result<Self> {
        Self::get_raw(&format!("https://api.schoology.com/v1/{url}"))
    }

    fn get_raw(url: &str) -> anyhow::Result<Self> {
        let url = Url::parse(url)?;
        Ok(Self::new(Method::GET, url))
    }

    fn into_schoology(mut self, token_info: &TokenInfo) -> anyhow::Result<Self> {
        self.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&generate_oauth_header(token_info)?)?,
        );
        self.headers_mut()
            .insert("Accept", HeaderValue::from_static("application/json"));
        Ok(self)
    }
}
