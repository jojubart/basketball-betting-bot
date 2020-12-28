use basketball_betting_bot::{get_token, Error};
use std::collections::HashMap;
mod scrape;
use basketball_betting_bot::utils::*;
use chrono::{Datelike, Timelike};
use scrape::*;
use sqlx::postgres::PgPool;
use std::env;
use teloxide::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let scraped_months = get_relevant_months()?;
    dbg!(&scraped_months);
    scrape_teams().await?;
    for month in scraped_months {
        scrape_games(month).await?;
    }
    let token = get_token("../config.ini");
    #[allow(deprecated)]
    let bot = Bot::new(&token);
    let pool = PgPool::connect(
        &env::var("DATABASE_URL").expect("Could not find environment variable DATABASE_URL"),
    )
    .await
    .expect("Could not establish connection do database");

    if active_chats_exist(&pool).await? {
        let chats = sqlx::query!("SELECT DISTINCT id FROM chats WHERE is_active = True")
            .fetch_all(&pool)
            .await
            .unwrap_or_default();

        // don't send polls in the middle of the night in USA and Europe
        if chrono::Utc::now().hour() >= 19 {
            for chat_id in chats {
                let poll_sent_success = send_polls(&pool, chat_id.id, &bot).await;

                if let Err(e) = poll_sent_success {
                    eprintln!(
                        "ERROR {e}\nCould not send polls for chat_id {chat_id}",
                        e = e,
                        chat_id = chat_id.id
                    );
                }
            }
        }
        stop_poll(&pool, &bot).await?;
    }

    Ok(())
}

async fn active_chats_exist(pool: &PgPool) -> Result<bool, Error> {
    Ok(
        sqlx::query!("SELECT EXISTS(SELECT * FROM chats WHERE is_active = True)")
            .fetch_one(pool)
            .await
            .unwrap()
            .exists
            .unwrap(),
    )
}

fn get_relevant_months() -> Result<Vec<String>, Error> {
    let months_ids = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    let months_names = vec![
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];

    let months = months_ids
        .iter()
        .zip(months_names.into_iter())
        .collect::<HashMap<_, _>>();

    let current_month = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(3))
        .unwrap()
        .month();
    let month_in_9_days = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(9))
        .unwrap()
        .month();

    let mut relevant_months = vec![];

    relevant_months.push(months[&current_month].to_string());

    if month_in_9_days != current_month {
        relevant_months.push(months[&month_in_9_days].to_string());
    }

    Ok(relevant_months)
}
