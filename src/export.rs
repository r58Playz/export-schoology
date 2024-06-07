#![allow(clippy::too_many_arguments)]
use std::path::PathBuf;

use anyhow::{anyhow, Context};
use async_recursion::async_recursion;
use log::{debug, error, info, warn};
use reqwest::Request;
use reqwest_middleware::ClientWithMiddleware;
use serde_json::Value;

use crate::{api_helpers::SchoologyRequestHelper, TokenInfo, ValueHelper};

pub async fn export_school(
    export_path: PathBuf,
    client: &ClientWithMiddleware,
    token_info: &TokenInfo,
    school_id: i64,
) -> anyhow::Result<()> {
    info!("exporting school/building {}", school_id);
    let info = client
        .execute(Request::get(&format!("schools/{school_id}"))?.into_schoology(token_info)?)
        .await?
        .json::<Value>()
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
) -> anyhow::Result<Value> {
    info!("exporting user {}", user_id);
    tokio::fs::create_dir(&export_path)
        .await
        .context("failed to create user export dir")?;

    let user_info = client
        .execute(Request::get(&format!("users/{}", user_id))?.into_schoology(token_info)?)
        .await
        .context("failed to request user info")?
        .json::<Value>()
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
    export_path_mapper: &(dyn Fn(String) -> PathBuf + Sync + Send),
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
            info!("exporting attachment {:?}", file_name);
            tokio::fs::write(
                export_path_mapper(file_name.replace("/", "_")),
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

#[async_recursion]
pub async fn export_directory(
    export_path: PathBuf,
    client: &ClientWithMiddleware,
    token_info: &TokenInfo,
    directory_info: &Value,
) -> anyhow::Result<()> {
    tokio::fs::create_dir(&export_path).await?;
    let Some(items) = directory_info.get_array("folder-item") else {
        return Ok(());
    };
    for item in items {
        let item_title = item
            .get_string("title")
            .context("failed to get item title")?;
        info!("exporting item {:?}", item_title);

        let item_url = item
            .get_string("location")
            .context("failed to get item url")?;
        let item_directory = export_path.join(item_title.replace("/", "_"));

        match item
            .get_string("type")
            .context("failed to get item type")?
            .as_str()
        {
            "folder" => {
                let folder_info = client
                    .execute(Request::get_raw(&item_url)?.into_schoology(token_info)?)
                    .await
                    .context("failed to request folder")?
                    .json::<Value>()
                    .await?;
                export_directory(item_directory, client, token_info, &folder_info).await?;
            }
            "page" => {
                let page_info = client
                    .execute(
                        Request::get_raw(&(item_url + "?with_attachments=TRUE&richtext=1"))?
                            .into_schoology(token_info)?,
                    )
                    .await
                    .context("failed to request page")?
                    .json::<Value>()
                    .await?;
                tokio::fs::create_dir(&item_directory).await?;
                tokio::fs::write(
                    item_directory.join("page.html"),
                    page_info
                        .get_string("body")
                        .context("failed to get page body")?,
                )
                .await?;
                export_attachments(
                    &|file_name| item_directory.join(format!("attachment_{file_name}")),
                    client,
                    token_info,
                    &page_info,
                )
                .await?;
            }
            "document" => {
                let document_info = client
                    .execute(
                        Request::get_raw(&(item_url + "?with_attachments=TRUE&richtext=1"))?
                            .into_schoology(token_info)?,
                    )
                    .await
                    .context("failed to get document info")?
                    .json::<Value>()
                    .await?;

                tokio::fs::create_dir(&item_directory).await?;
                tokio::fs::write(
                    item_directory.join("info.json"),
                    serde_json::to_string_pretty(&document_info)?,
                )
                .await?;

                export_attachments(
                    &|file_name| item_directory.join(format!("attachment_{file_name}")),
                    client,
                    token_info,
                    &document_info,
                )
                .await?;
            }
            "assignment" => {
                let assignment_info = client
                    .execute(
                        Request::get_raw(&format!("{item_url}?with_attachments=TRUE&richtext=1"))?
                            .into_schoology(token_info)?,
                    )
                    .await
                    .context("failed to get assignment info")?
                    .json::<Value>()
                    .await?;
                tokio::fs::create_dir(&item_directory).await?;
                tokio::fs::write(
                    item_directory.join("info.json"),
                    serde_json::to_string_pretty(&assignment_info)?,
                )
                .await?;

                let assignment_submissions = client
                    .execute(
                        Request::get_raw(
                            &(item_url.replace("assignments", "submissions")
                                + "?with_attachments=TRUE&all_revisions=TRUE"),
                        )?
                        .into_schoology(token_info)?,
                    )
                    .await
                    .context("failed to request assignment submissions")?
                    .json::<Value>()
                    .await?;

                for revision in assignment_submissions
                    .get_array("revision")
                    .context("failed to get assignment submissions")?
                {
                    let revision_id = revision
                        .get_int("revision_id")
                        .context("failed to get assignment submission revision id")?;
                    info!("exporting revision {}", revision_id);

                    let revision_directory =
                        item_directory.join(format!("revision_{}", revision_id));

                    tokio::fs::create_dir(&revision_directory).await?;
                    tokio::fs::write(
                        revision_directory.join("info.json"),
                        serde_json::to_string_pretty(&revision)?,
                    )
                    .await?;

                    export_attachments(
                        &|file_name| revision_directory.join(file_name),
                        client,
                        token_info,
                        &revision,
                    )
                    .await?;
                }

                let assignment_grade = client
                    .execute(
                        Request::get_raw(
                            &(item_url.replace("assignments/", "grades?assignment_id=")),
                        )?
                        .into_schoology(token_info)?,
                    )
                    .await
                    .context("failed to request assignment grade")?
                    .json::<Value>()
                    .await.context("abc")?;

                tokio::fs::write(
                    item_directory.join("grade.json"),
                    serde_json::to_string_pretty(&assignment_grade)?,
                )
                .await?;
            }
            x => {
                error!("item: {:#?}", item);
                return Err(anyhow!("unknown type {:?}", x));
            }
        }
    }
    Ok(())
}
