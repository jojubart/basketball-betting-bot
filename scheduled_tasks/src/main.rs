use basketball_betting_bot::Error;
use std::collections::HashMap;
mod scrape;
use basketball_betting_bot::utils::*;
use chrono::{Datelike, Timelike, Utc};
use scrape::*;
use sqlx::postgres::PgPool;
use std::env;
use teloxide::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = PgPool::connect(
        &env::var("DATABASE_URL").expect("Could not find environment variable DATABASE_URL"),
    )
    .await
    .expect("Could not establish connection do database");

    let bot = Bot::builder().build();
    stop_poll(&pool, &bot).await?;
    refresh_materialized_views(&pool).await?;

    // do nothing if season is over
    if east_coast_date_in_x_days(0, false)?
        > chrono::NaiveDate::parse_from_str("2021-05-18", "%Y-%m-%d")?
    {
        return Ok(());
    }

    match Utc::now().hour() {
        0..=4 | 7..=9 | 20..=23 => scrape_games_live(&pool).await.unwrap(),
        5..=6 => {
            cache_games(
                get_games(
                    &pool,
                    10,
                    east_coast_date_in_x_days(1, false)?,
                    east_coast_date_in_x_days(7, false)?,
                )
                .await
                .unwrap_or_default(),
            )
            .unwrap_or_else(|error| {
                dbg!("Can't cache games!", error);
            });

            scrape_games_live(&pool).await.unwrap();
        }

        10..=11 => {
            let scraped_months = get_relevant_months()?;
            dbg!(&scraped_months);
            scrape_teams().await?;
            for month in scraped_months {
                scrape_games(month).await?;
            }
            cache_games(
                get_games(
                    &pool,
                    10,
                    east_coast_date_in_x_days(1, false)?,
                    east_coast_date_in_x_days(7, false)?,
                )
                .await
                .unwrap_or_default(),
            )
            .unwrap_or_else(|error| {
                dbg!("Can't cache games!", error);
            });
        }

        12..=17 => {}
        18..=19 => {
            if active_chats_exist(&pool).await? {
                let chats = sqlx::query!("SELECT DISTINCT id FROM chats WHERE is_active = True")
                    .fetch_all(&pool)
                    .await
                    .unwrap_or_default();

                // send message if season is over for the first time
                if east_coast_date_in_x_days(0, false)?
                    == chrono::NaiveDate::parse_from_str("2021-05-17", "%Y-%m-%d")?
                    && Utc::now().minute() < 30
                {
                    for chat_id in chats {
                        {
                            bot.send_message(chat_id.id, "Your NBA betting season is over! Check out the results /full_standings")
                                .send()
                                .await?;
                        }
                    }
                    return Ok(());
                }

                let mut games = cache_to_games().unwrap_or_default();
                if games.len() < 11 {
                    games = get_games(
                        &pool,
                        10,
                        east_coast_date_in_x_days(1, false)?,
                        east_coast_date_in_x_days(7, false)?,
                    )
                    .await
                    .unwrap_or_default();
                }

                for chat_id in chats {
                    let poll_sent_success = send_polls(&pool, chat_id.id, &bot, &games).await;

                    if let Err(e) = poll_sent_success {
                        eprintln!(
                            "ERROR {e}\nCould not send polls for chat_id {chat_id}",
                            e = e,
                            chat_id = chat_id.id
                        );
                    }
                }
            }
        }
        _ => {}
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
