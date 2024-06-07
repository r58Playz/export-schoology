#![allow(clippy::too_many_arguments)]
use std::path::PathBuf;

use anyhow::Context;
use reqwest::Request;
use reqwest_middleware::ClientWithMiddleware;
use serde_json::Value;

use crate::{api_helpers::SchoologyRequestHelper, Map, MapHelper, TokenInfo};

pub async fn export_school(
    export_path: PathBuf,
    client: &ClientWithMiddleware,
    token_info: &TokenInfo,
    school_id: i64,
) -> anyhow::Result<()> {
    let info = client
        .execute(Request::get(&format!("schools/{school_id}"))?.into_schoology(token_info)?)
        .await?
        .json::<Map>()
        .await?;

    tokio::fs::write(
        export_path.join("info.json"),
        serde_json::to_string_pretty(&info)?,
    )
    .await?;

    tokio::fs::write(
        export_path.join("picture.png"),
        client
            .get(
                info.get_string("picture_url")
                    .context("failed to get school/building picture url")?,
            )
            .send()
            .await
            .context("failed to request school/building picture url")?
            .bytes()
            .await?,
    )
    .await
    .context("failed to save school/building picture")?;

    Ok(())
}

pub async fn export_user(
    export_path: PathBuf,
    client: &ClientWithMiddleware,
    token_info: &TokenInfo,
    user_id: i64,
) -> anyhow::Result<Map> {
    tokio::fs::create_dir(&export_path)
        .await
        .context("failed to create user export dir")?;

    let user_info = client
        .execute(Request::get(&format!("users/{}", user_id))?.into_schoology(token_info)?)
        .await
        .context("failed to request user info")?
        .json::<Map>()
        .await?;

    tokio::fs::write(
        export_path.join("user_info.json"),
        serde_json::to_string_pretty(&user_info)?,
    )
    .await?;

    tokio::fs::write(
        export_path.join("user_image.png"),
        client
            .get(
                user_info
                    .get_string("picture_url")
                    .context("failed to get user picture url")?,
            )
            .send()
            .await
            .context("failed to request user picture url")?
            .bytes()
            .await?,
    )
    .await
    .context("failed to save user picture")?;

    Ok(user_info)
}

pub async fn export_attachments(
    export_path_mapper: &dyn Fn(String) -> PathBuf,
    client: &ClientWithMiddleware,
    token_info: &TokenInfo,
    info: &Value,
) -> anyhow::Result<()> {
    if let Some(file_attachments) = info
        .get("attachments")
        .and_then(|x| x.get("files"))
        .and_then(|x| x.get_array("file"))
    {
        for attachment in file_attachments {
            let download_url = attachment
                .get_string("download_path")
                .context("failed to get file attachment download path")?;
            let file_name = attachment
                .get_string("filename")
                .context("failed to get file attachment name")?;
            tokio::fs::write(
                export_path_mapper(file_name),
                client
                    .execute(Request::get_raw(&download_url)?.into_schoology(token_info)?)
                    .await
                    .context("failed to request file attachment")?
                    .bytes()
                    .await?,
            )
            .await
            .context("failed to save file attachment")?;
        }
    }
    Ok(())
}
