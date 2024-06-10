use std::{
    path::PathBuf,
    sync::Arc,
    time::{Instant, SystemTime},
};

use anyhow::Context;
use api_helpers::{get, get_raw, SchoologyRequestHelper};
use export::{export_attachments, export_directory, export_school, export_user};
use http::Extensions;
use log::{debug, info};
use reqwest::{Client, Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde_json::{json, Value};
use tokio::io::{stdin, AsyncBufReadExt, BufReader};

mod api_helpers;
mod export;

trait ValueHelper {
    fn get_string(&self, key: &str) -> Option<String>;
    fn get_int(&self, key: &str) -> Option<i64>;
    fn get_array(&self, key: &str) -> Option<Vec<Value>>;
}

impl ValueHelper for Value {
    fn get_string(&self, key: &str) -> Option<String> {
        self.get(key)
            .and_then(|x| x.as_str())
            .map(|x| x.to_string())
    }

    fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|x| x.as_i64())
    }

    fn get_array(&self, key: &str) -> Option<Vec<Value>> {
        self.get(key).and_then(|x| x.as_array()).cloned()
    }
}

struct LoggingMiddleware;

#[async_trait::async_trait]
impl Middleware for LoggingMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        debug!("Fetching \"{}\"", req.url());
        next.run(req, extensions).await
    }
}

struct TokenInfo {
    pub client_token: String,
    pub client_secret: String,
    pub user_token: Option<String>,
    pub user_secret: Option<String>,
}

impl TokenInfo {
    pub fn new(
        app_token: String,
        app_secret: String,
        user_token: String,
        user_secret: String,
    ) -> Self {
        Self {
            client_token: app_token,
            client_secret: app_secret,
            user_token: Some(user_token),
            user_secret: Some(user_secret),
        }
    }

    pub fn new_no_user(app_token: String, app_secret: String) -> Self {
        Self {
            client_token: app_token,
            client_secret: app_secret,
            user_token: None,
            user_secret: None,
        }
    }
}

async fn login(
    client: &ClientWithMiddleware,
    domain: &str,
    app_token: &str,
    app_secret: &str,
) -> anyhow::Result<(String, String)> {
    let token_resp =
        client
            .execute(Request::get("oauth/request_token")?.into_schoology(
                &TokenInfo::new_no_user(app_token.to_string(), app_secret.to_string()),
            )?)
            .await?
            .text()
            .await?;

    let mut token_split = token_resp.split('&').map(|x| x.split('=').nth(1));

    let request_token = token_split
        .next()
        .flatten()
        .context("failed to get request token from answer")?;
    let request_secret = token_split
        .next()
        .flatten()
        .context("failed to get request secret from answer")?;

    info!(
        "https://{domain}/oauth/authorize?oauth_callback=example.com&oauth_token={request_token}"
    );
    info!("open the above url and press ENTER once authorized");
    BufReader::new(stdin())
        .read_line(&mut String::new())
        .await?;

    let token_resp = client
        .execute(
            Request::get("oauth/access_token")?.into_schoology(&TokenInfo::new(
                app_token.to_string(),
                app_secret.to_string(),
                request_token.to_string(),
                request_secret.to_string(),
            ))?,
        )
        .await?
        .text()
        .await?;

    let mut token_split = token_resp.split('&').map(|x| x.split('=').nth(1));

    let client_token = token_split
        .next()
        .flatten()
        .context("failed to get client token from answer")?;
    let client_secret = token_split
        .next()
        .flatten()
        .context("failed to get client secret from answer")?;

    Ok((client_token.to_string(), client_secret.to_string()))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let start = Instant::now();

    let client = Client::new();
    let policy = ExponentialBackoff::builder().build_with_max_retries(10);
    let client = ClientBuilder::new(client)
        .with(LoggingMiddleware)
        .with(RetryTransientMiddleware::new_with_policy(policy))
        .build();
    let client = Arc::new(client);

    let creds =
        tokio::fs::read_to_string(std::env::args().nth(1).context("path to creds not found")?)
            .await
            .context("failed to read creds file")?;
    let mut creds = creds.split('\n');

    let domain = creds.next().context("no schoology domain")?;
    let client_token = creds.next().context("no app token")?;
    let client_secret = creds.next().context("no app secret")?;
    let user_token = creds.next();
    let user_secret = creds.next();

    let (user_token, user_secret) = if let Some(user_creds) =
        user_token.and_then(|x| user_secret.map(|y| (x.to_string(), y.to_string())))
    {
        user_creds
    } else {
        let creds = login(&client, domain, client_token, client_secret).await?;
        debug!("creds: {:?}", creds);
        creds
    };
    let token_info = TokenInfo::new(
        client_token.to_string(),
        client_secret.to_string(),
        user_token,
        user_secret,
    );

    let export_dir = PathBuf::from(format!(
        "export_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_millis()
    ));
    tokio::fs::create_dir(&export_dir)
        .await
        .context("failed to create export dir")?;

    let export_school_dir = export_dir.join("school");
    tokio::fs::create_dir(&export_school_dir)
        .await
        .context("failed to create export school dir")?;

    let export_building_dir = export_dir.join("building");
    tokio::fs::create_dir(&export_building_dir)
        .await
        .context("failed to create export building dir")?;

    let export_updates_dir = export_dir.join("updates");
    tokio::fs::create_dir(&export_updates_dir)
        .await
        .context("failed to create export updates dir")?;

    let export_messages_dir = export_dir.join("messages");
    tokio::fs::create_dir(&export_messages_dir)
        .await
        .context("failed to create export messages dir")?;

    let export_users_dir = export_dir.join("users");
    tokio::fs::create_dir(&export_users_dir)
        .await
        .context("failed to create export users dir")?;

    let export_courses_dir = export_dir.join("courses");
    tokio::fs::create_dir(&export_courses_dir)
        .await
        .context("failed to create export courses dir")?;

    let uid = client
        .execute(Request::get("app-user-info")?.into_schoology(&token_info)?)
        .await
        .context("failed to request uid")?
        .json::<Value>()
        .await?
        .get_int("api_uid")
        .context("failed to get uid")?;

    info!("logged in as user {}", uid);

    tokio::fs::write(export_users_dir.join("self"), uid.to_string()).await?;

    let mut exported_users: Vec<i64> = Vec::new();
    let user_info = export_user(
        export_users_dir.join(uid.to_string()),
        &client,
        &token_info,
        uid,
    )
    .await?;
    exported_users.push(uid);
    macro_rules! export_user {
        ($uid:ident) => {
            if !exported_users.contains(&$uid) {
                export_user(
                    export_users_dir.join($uid.to_string()),
                    &client,
                    &token_info,
                    $uid,
                )
                .await
                .context("failed to export user")?;
                exported_users.push($uid);
            }
        };
    }

    let school_id = user_info
        .get_int("school_id")
        .context("failed to get school id")?;

    export_school(export_school_dir, &client, &token_info, school_id).await?;

    let building_id = user_info
        .get_int("building_id")
        .context("failed to get building id")?;

    export_school(export_building_dir, &client, &token_info, building_id).await?;

    let mut updates_url = "https://api.schoology.com/v1/recent/?extended&options&start=0&limit=50&created_offset=0&with_attachments=TRUE&richtext=1".to_string();
    let mut updates_cnt = 0;
    loop {
        info!("exporting updates ({})", updates_cnt);
        let update_info = get_raw(&client, &token_info, &updates_url)
            .await
            .context("failed to request update info")?;

        for update in update_info
            .get_array("update")
            .context("failed to get update info")?
        {
            let update_id = update.get_int("id").context("failed to get update id")?;

            let update_user_id = update
                .get_int("uid")
                .context("failed to get update user id")?;
            export_user!(update_user_id);

            for comment in update
                .get_array("comments")
                .context("failed to get update comments")?
            {
                let comment_user_id = comment
                    .get_int("uid")
                    .context("failed to get update comment user id")?;
                export_user!(comment_user_id);
            }

            export_attachments(
                &|file_name| export_updates_dir.join(format!("update_{update_id}_{file_name}")),
                &client,
                &token_info,
                &update,
            )
            .await?;
        }
        tokio::fs::write(
            export_updates_dir.join(format!("updates_{updates_cnt}.json")),
            serde_json::to_string_pretty(&update_info)?,
        )
        .await?;

        updates_cnt += 1;
        if let Some(next_link) = update_info.get("links").and_then(|x| x.get_string("next")) {
            updates_url = next_link
        } else {
            break;
        }
    }

    let mut messages_url = "https://api.schoology.com/v1/messages/inbox?extended&options&start=0&limit=50&created_offset=0&with_attachments=TRUE&richtext=1".to_string();
    let mut parsed_sent_messages = false;
    let mut messages_cnt = 0;
    loop {
        info!("exporting messages ({})", messages_cnt);
        let messages_info = get_raw(&client, &token_info, &messages_url)
            .await
            .context("failed to request messages info")?;

        for message in messages_info
            .get_array("message")
            .context("failed to get messages info")?
        {
            let message_id = message.get_int("id").context("failed to get message id")?;

            let message_url = message
                .get("links")
                .and_then(|x| x.get_string("self"))
                .context("failed to get message url")?;

            let message_info = client
                .execute(Request::get_raw(&message_url)?.into_schoology(&token_info)?)
                .await
                .context("failed to request message info")?
                .json::<Value>()
                .await?;

            tokio::fs::write(
                export_messages_dir.join(format!("message_{message_id}.json")),
                serde_json::to_string_pretty(&message_info)?,
            )
            .await?;

            export_attachments(
                &|file_name| export_messages_dir.join(format!("message_{message_id}_{file_name}")),
                &client,
                &token_info,
                &message,
            )
            .await?;

            if let Some(update_user_id) = message.get_int("author_id") {
                export_user!(update_user_id);
            }
        }
        tokio::fs::write(
            export_messages_dir.join(format!("messages_{messages_cnt}.json")),
            serde_json::to_string_pretty(&messages_info)?,
        )
        .await?;

        messages_cnt += 1;
        if let Some(next_link) = messages_info
            .get("links")
            .and_then(|x| x.get_string("next"))
        {
            messages_url = next_link
        } else if !parsed_sent_messages {
            messages_url = "https://api.schoology.com/v1/messages/sent?extended&options&start=0&limit=50&created_offset=0&with_attachments=TRUE&richtext=1".to_string();
            parsed_sent_messages = true;
        } else {
            break;
        }
    }

    let courses = get(
        &client,
        &token_info,
        &format!("users/{uid}/sections?include_past=1"),
    )
    .await
    .context("failed to request courses")?;

    tokio::fs::write(
        export_courses_dir.join("info.json"),
        serde_json::to_string_pretty(&courses)?,
    )
    .await?;

    let courses_list = courses
        .get_array("section")
        .context("failed to get courses")?;

    debug!(
        "courses to export: {:?}",
        courses_list
            .iter()
            .map(|x| x.get_string("id").unwrap_or_default())
            .collect::<Vec<_>>()
    );

    for course in courses_list {
        let course_id = course.get_string("id").context("failed to get course id")?; // ???
        let course_dir = export_courses_dir.join(&course_id);
        tokio::fs::create_dir(&course_dir).await?;

        info!("exporting course {}", course_id);

        let course_info_url = course
            .get("links")
            .and_then(|x| x.get_string("self"))
            .context("failed to get course url")?;
        let course_info = client
            .execute(Request::get_raw(&course_info_url)?.into_schoology(&token_info)?)
            .await
            .context("failed to get course info")?
            .json::<Value>()
            .await?;
        tokio::fs::write(
            course_dir.join("info.json"),
            serde_json::to_string_pretty(&course_info)?,
        )
        .await?;

        let course_banner_url = course_info
            .get_string("profile_url")
            .context("failed to get course banner url")?;
        tokio::fs::write(
            course_dir.join("banner.png"),
            client
                .execute(Request::get_raw(&course_banner_url)?.into_schoology(&token_info)?)
                .await
                .context("failed to request course banner")?
                .bytes()
                .await?,
        )
        .await?;

        let course_grades_info = client
            .execute(
                Request::get(&format!("users/{uid}/grades/?section_id={course_id}"))?
                    .into_schoology(&token_info)?,
            )
            .await
            .context("failed to get course grades")?
            .json::<Value>()
            .await?;
        tokio::fs::write(
            course_dir.join("grades.json"),
            serde_json::to_string_pretty(&course_grades_info)?,
        )
        .await?;

        let course_files_root = course_dir.join("files");

        let course_files_info = client
            .execute(
                Request::get(&format!("courses/{course_id}/folder/0"))?
                    .into_schoology(&token_info)?,
            )
            .await
            .context("failed to request course files")?
            .json::<Value>()
            .await?;

        export_directory(course_files_root, &client, &token_info, &course_files_info)
            .await
            .context("failed to export course files")?;
    }

    let end = Instant::now();

    info!(
        "Exported in {}",
        humantime::format_duration(end.duration_since(start))
    );

    Ok(())
}
