use crate::{get_token, Error};
use chrono::Datelike;
use sqlx::{postgres::PgPool, types::BigDecimal};
use std::collections::HashMap;
use std::env;
use teloxide::prelude::*;

fn east_coast_date_today() -> Result<chrono::NaiveDate, Error> {
    let today_east_coast_delayed_format = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::hours(5))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();

    Ok(chrono::NaiveDate::parse_from_str(
        &today_east_coast_delayed_format,
        "%Y-%m-%d",
    )?)
}

/// past: describes if you want the day x days in the past (true) or in the future (false)
fn east_coast_date_in_x_days(days: i64, past: bool) -> Result<chrono::NaiveDate, Error> {
    let east_coast_datetime = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::hours(5))
        .unwrap();
    let east_coast_delayed_format = match past {
        true => east_coast_datetime
            .checked_sub_signed(chrono::Duration::days(days))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string(),
        false => east_coast_datetime
            .checked_add_signed(chrono::Duration::days(days))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string(),
    };

    Ok(chrono::NaiveDate::parse_from_str(
        &east_coast_delayed_format,
        "%Y-%m-%d",
    )?)
}
pub async fn send_polls(pool: &PgPool, chat_id: i64, bot: &teloxide::Bot) -> anyhow::Result<()> {
    let bet_week = get_bet_week(pool, chat_id).await?;
    let today = east_coast_date_today()?;

    dbg!(bet_week.polls_sent);
    dbg!(today >= bet_week.end_date);
    dbg!(!bet_week.polls_sent || today >= bet_week.end_date);

    // if week_number is 0 it's the first time polls are sent to the chat
    // that means we have not entry yet for the chat in bet_weeks and want to send the polls for
    // the upcoming week right away
    // if today is the last day of a bet_week, we want to send out new polls for the upcoming week
    if bet_week.week_number == 0 || today >= bet_week.end_date {
        let week_number = bet_week.week_number + 1;
        let number_of_games = get_number_of_games_for_chat(&pool, chat_id)
            .await
            .expect("Could not get number of games");

        let games = get_games(
            &pool,
            number_of_games,
            bet_week.start_date,
            bet_week.end_date,
        )
        .await
        .unwrap();

        let bet_week_id = insert_bet_week(
            pool,
            chat_id,
            week_number,
            east_coast_date_in_x_days(1, false).unwrap(),
            east_coast_date_in_x_days(7, false).unwrap(),
            true,
        )
        .await?;

        for game in &games {
            send_game(&pool, game.id, chat_id, game, &bot, bet_week_id).await?;
        }

        //update_bet_week(pool, bet_week.id).await?;
    }
    Ok(())
}

async fn insert_bet_week(
    pool: &PgPool,
    chat_id: i64,
    week_number: i32,
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
    polls_sent: bool,
) -> anyhow::Result<i32> {
    let row = sqlx::query!(
        r#"
        INSERT INTO bet_weeks(chat_id, week_number, start_date, end_date, polls_sent) VALUES 
        ($1, $2, $3, $4, $5)
        RETURNING id;
        "#,
        chat_id,
        week_number,
        start_date,
        end_date,
        polls_sent
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

async fn update_bet_week(pool: &PgPool, bet_week_id: i32) -> anyhow::Result<()> {
    dbg!("update_bet_week was called");
    sqlx::query!(
        r#"
        UPDATE bet_weeks
        SET polls_sent = True
        WHERE
        id = $1
        "#,
        bet_week_id
    )
    .execute(pool)
    .await;
    Ok(())
}

async fn get_bet_week(pool: &PgPool, chat_id: i64) -> Result<BetWeek, Error> {
    let row = sqlx::query!(
        r#"SELECT 
        id,
        week_number
        ,MAX(start_date) AS start_date
        ,MAX(end_date) AS end_date
        ,polls_sent
        FROM bet_weeks
        WHERE chat_id = $1
        GROUP BY week_number, id"#,
        chat_id
    )
    .fetch_optional(pool)
    .await?;

    // if no week_number is found, it's the first time this chat is using the bot
    // in that case, we insert the initial bet_week values and start the week from today
    match row {
        Some(row) => {
            dbg!("SOME PATH");
            dbg!(&row);

            return Ok(BetWeek {
                id: row.id,
                week_number: row.week_number.unwrap(),
                start_date: row.start_date.unwrap(),
                end_date: row.end_date.unwrap(),
                polls_sent: row.polls_sent.unwrap(),
            });
        }
        None => {
            let week_number = 0;
            let start_date = east_coast_date_in_x_days(1, false)?;
            let end_date = east_coast_date_in_x_days(7, false)?;
            let polls_sent = false;
            dbg!("NONE PATH");
            dbg!(start_date, end_date);

            //let bet_week_id =
            //   insert_bet_week(pool, chat_id, week_number, start_date, end_date, false)
            //      .await
            //     .unwrap();

            return Ok(BetWeek {
                id: -1,
                week_number,
                start_date,
                end_date,
                polls_sent,
            });
        }
    }
}

async fn get_number_of_games_for_chat(pool: &PgPool, chat_id: i64) -> anyhow::Result<i64> {
    let number_of_games = sqlx::query!(
        "SELECT number_of_games FROM full_chat_information WHERE chat_id = $1",
        chat_id
    )
    .fetch_one(pool)
    .await
    .unwrap()
    .number_of_games;

    Ok(number_of_games.unwrap() as i64)
}

async fn send_game(
    pool: &PgPool,
    game_id: i32,
    chat_id: i64,
    game: &Game,
    bot: &teloxide::Bot,
    bet_week_id: i32,
) -> anyhow::Result<()> {
    if poll_is_in_db(&pool, game_id, chat_id)
        .await
        .expect("Database Error: poll_is_in_db")
    {
        dbg!("entry already in polls table!");
        return Ok(());
    } else {
        let poll = bot
            .send_poll(
                chat_id,
                format!(
                    "{away_team} @ {home_team} \n{pretty_time} ET",
                    home_team = game.home_team,
                    away_team = game.away_team,
                    pretty_time = game.pretty_time
                ),
                vec![game.away_team.to_string(), game.home_team.to_string()],
            )
            .disable_notification(true)
            .is_anonymous(false)
            .send()
            .await
            .expect("could not send out poll!");
        let poll_id = poll.poll().expect("").id.to_owned();
        let local_id = poll.id;

        add_poll(&pool, poll_id, local_id, chat_id, game.id, bet_week_id).await?;
    }
    Ok(())
}

async fn poll_is_in_db(pool: &PgPool, game_id: i32, chat_id: i64) -> Result<bool, Error> {
    let is_in_poll_table = sqlx::query!(
        r#"
        SELECT EXISTS(
            SELECT * 
            FROM polls
            WHERE game_id = $1
            AND chat_id = $2
        ) AS exists
        ;
        "#,
        game_id,
        chat_id
    )
    .fetch_one(pool)
    .await?
    .exists;

    Ok(is_in_poll_table.unwrap())
}

async fn add_poll(
    pool: &PgPool,
    poll_id: String,
    local_id: i32,
    chat_id: i64,
    game_id: i32,
    bet_week_id: i32,
) -> anyhow::Result<()> {
    let date_east_coast = east_coast_date_today()?;

    sqlx::query!(
        r#"
        INSERT INTO polls(id,local_id, chat_id, game_id, poll_sent_date, bet_week_id) VALUES 
        ($1, $2, $3, $4, $5, $6);
        "#,
        poll_id,
        local_id,
        chat_id,
        game_id,
        date_east_coast,
        bet_week_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn get_games(
    pool: &PgPool,
    number_of_games: i64,
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
) -> anyhow::Result<Vec<Game>> {
    // All I wanted to do was getting the current east coast date (not datetime) based on utc team
    // and now this monstrosity is here. Please let me know the proper way to do this. Please.

    let games_raw = sqlx::query!(
        r#"
        SELECT * FROM (
        (SELECT 
            game_id
            ,away_team_id
            ,away_team
            ,home_team_id
            ,home_team
            ,srs_sum
            ,date_time AT TIME ZONE 'EST' as date_time
            ,DATE(date_time AT TIME ZONE 'EST') AS date
            ,to_char(date_time AT TIME ZONE 'EST', 'YYYY-MM-DD HH24:MI TZ') AS pretty_time
        FROM full_game_information
        WHERE DATE(date_time AT TIME ZONE 'EST') <= $1
        AND DATE(date_time AT TIME ZONE 'EST') >= $2
        ORDER BY srs_sum DESC
        LIMIT $3)

        UNION 
        
        (SELECT
            game_id
            ,away_team_id
            ,away_team
            ,home_team_id
            ,home_team
            ,srs_sum
            ,date_time AT TIME ZONE 'EST' as date_time
            ,DATE(date_time AT TIME ZONE 'EST') AS date
            ,to_char(date_time AT TIME ZONE 'EST', 'YYYY-MM-DD HH24:MI TZ') AS pretty_time
        FROM full_game_information
        WHERE DATE(date_time AT TIME ZONE 'EST') <= $1
        AND DATE(date_time AT TIME ZONE 'EST') >= $2
        ORDER BY srs_sum ASC
        LIMIT 1
        )) 
         AS games
        ORDER BY date ASC
        ;

        "#,
        // date a week from now in East Coast time
        end_date,
        // tomorrow's date in East Coast time
        start_date,
        number_of_games
    )
    .fetch_all(pool)
    .await?;

    let mut games = Vec::new();
    for record in games_raw {
        dbg!(&record);
        let game: Game = Game {
            id: record.game_id.unwrap(),
            away_team_id: record.away_team_id.unwrap(),
            away_team: record.away_team.unwrap(),
            home_team_id: record.home_team_id.unwrap(),
            home_team: record.home_team.unwrap(),
            srs_sum: record.srs_sum.unwrap(),
            date_time: record.date_time.unwrap(),
            pretty_time: record.pretty_time.unwrap(),
        };
        games.push(game);
    }

    Ok(games)
}

async fn polls_exist(pool: &PgPool) -> Result<bool Error> {
    Ok(sqlx::query!("SELECT EXISTS(SELECT * FROM polls)")
        .fetch_one(pool)
        .await?
        .exists)
}
pub async fn stop_poll(pool: &PgPool, bot: &teloxide::Bot) -> Result<(), Error> {
    if !polls_exist(pool).await? {return Ok(())}
    let polls_to_close = sqlx::query!(
        r#"
        SELECT id, local_id, chat_id FROM polls
       WHERE game_id IN
       (SELECT id FROM games WHERE now() at time zone 'EST' >= date_time)
       AND is_open = True;

        "#
    )
    .fetch_all(pool)
    .await?;

    for poll in polls_to_close {
        let sp = bot
            .stop_poll(poll.chat_id.unwrap(), poll.local_id.unwrap())
            .send()
            .await?;

        sqlx::query!(
            r#"
        UPDATE polls SET is_open = False WHERE id = $1
        "#,
            poll.id
        )
        .execute(pool)
        .await?;
        dbg!(poll);
    }

    Ok(())
}

#[derive(Debug)]
pub struct Game {
    id: i32,
    away_team_id: i32,
    away_team: String,
    home_team_id: i32,
    home_team: String,
    srs_sum: BigDecimal,
    date_time: chrono::NaiveDateTime,
    pretty_time: String,
}

#[derive(Debug)]
struct BetWeek {
    id: i32,
    week_number: i32,
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
    polls_sent: bool,
}
