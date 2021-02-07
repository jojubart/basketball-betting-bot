use crate::Error;
use chrono::prelude::*;
use chrono::Duration;
use num_traits::cast::ToPrimitive;
use redis::Commands;
use sqlx::{postgres::PgPool, query};
use teloxide::prelude::*;
use teloxide::KnownApiErrorKind;

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
pub fn east_coast_date_in_x_days(days: i64, past: bool) -> Result<chrono::NaiveDate, Error> {
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

pub async fn refresh_materialized_views(pool: &PgPool) -> anyhow::Result<()> {
    log::info!("Refreshing materialized views!");

    query!("REFRESH MATERIALIZED VIEW weekly_rankings")
        .execute(pool)
        .await?;

    Ok(())
}
pub async fn send_polls(
    pool: &PgPool,
    chat_id: i64,
    bot: &teloxide::Bot,
    games: &[Game],
) -> anyhow::Result<()> {
    log::info!(
        "{}",
        format!("Sending polls! (send_polls()), chat_id: {}", chat_id)
    );
    let bet_week = get_bet_week(pool, chat_id).await?;
    let tomorrow = east_coast_date_in_x_days(1, false)?;

    // if week_number is 0 it's the first time polls are sent to the chat
    // that means we have not entry yet for the chat in bet_weeks and want to send the polls for
    // the upcoming week right away
    // if today is the last day of a bet_week, we want to send out new polls for the upcoming week
    if bet_week.week_number == 0 || tomorrow > bet_week.end_date {
        let week_number = bet_week.week_number + 1;

        let bet_week_id = insert_bet_week(
            pool,
            chat_id,
            week_number,
            east_coast_date_in_x_days(1, false)?,
            east_coast_date_in_x_days(7, false)?,
            true,
        )
        .await?;

        for game in games {
            send_game(&pool, game.id, chat_id, game, &bot, bet_week_id).await?;
        }
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
    let row = query!(
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

async fn _update_bet_week(pool: &PgPool, bet_week_id: i32) -> anyhow::Result<()> {
    query!(
        r#"
        UPDATE bet_weeks
        SET polls_sent = True
        WHERE
        id = $1
        "#,
        bet_week_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_bet_week(pool: &PgPool, chat_id: i64) -> Result<BetWeek, Error> {
    let row = query!(
        r#"SELECT 
        id,
        week_number
        ,start_date
        ,end_date
        ,polls_sent
        FROM bet_weeks
        WHERE chat_id = $1
        AND end_date = (SELECT MAX(end_date) FROM bet_weeks where chat_id = $1)
        ORDER BY id ASC
        LIMIT 1
        "#,
        chat_id
    )
    .fetch_optional(pool)
    .await?;

    // if no week_number is found, it's the first time this chat is using the bot
    // in that case, we insert the initial bet_week values and start the week from today
    match row {
        Some(row) => {
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

async fn _get_number_of_games_for_chat(pool: &PgPool, chat_id: i64) -> anyhow::Result<i64> {
    let number_of_games = query!(
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
    if poll_is_in_db(&pool, game_id, chat_id).await? {
        eprintln!("entry already in polls table!");
        return Ok(());
    } else {
        let poll = bot
            .send_poll(
                chat_id,
                format!(
                    "{away_team} @ {home_team} \n{date_string}\n{time_string} ET",
                    home_team = game.home_team,
                    away_team = game.away_team,
                    date_string = game.date_string,
                    time_string = game.time_string
                ),
                vec![game.away_team.to_string(), game.home_team.to_string()],
            )
            .disable_notification(true)
            .is_anonymous(false)
            .send()
            .await;

        if let Ok(poll) = poll {
            let poll_id = poll.poll().expect("").id.to_owned();
            let local_id = poll.id;

            add_poll(&pool, poll_id, local_id, chat_id, game.id, bet_week_id).await?;
        } else {
            eprintln!(
                "POLL in chat {chat_id} could not be sent",
                chat_id = chat_id
            );
        }
    }
    Ok(())
}

pub async fn poll_is_in_db(pool: &PgPool, game_id: i32, chat_id: i64) -> Result<bool, Error> {
    query!(
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
    .exists
    .ok_or(Error::SQLxError(sqlx::Error::RowNotFound))
}

pub async fn poll_is_in_db_by_poll_id(pool: &PgPool, poll_id: String) -> Result<bool, Error> {
    query!(
        "SELECT EXISTS(SELECT id from polls WHERE id = $1);",
        poll_id
    )
    .fetch_one(pool)
    .await?
    .exists
    .ok_or(Error::SQLxError(sqlx::Error::RowNotFound))
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

    query!(
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

pub async fn get_games(
    pool: &PgPool,
    number_of_games: i64,
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
) -> anyhow::Result<Vec<Game>> {
    let games_raw = query!(
        r#"
   (SELECT * FROM (SELECT DISTINCT ON (home_team_id, away_team_id)
               game_id
               ,away_team_id
               ,away_team
               ,home_team_id
               ,home_team
               ,srs_sum
               ,date_time AT TIME ZONE 'EST' as date_time
               ,DATE(date_time AT TIME ZONE 'EST') AS date
               ,to_char(date_time AT TIME ZONE 'EST', 'YYYY-MM-DD') AS date_string
               ,to_char(date_time AT TIME ZONE 'EST', 'HH:MI AM TZ') AS time_string
               ,to_char(date_time AT TIME ZONE 'EST', 'YYYY-MM-DD HH:MI AM TZ') AS pretty_date_time
               ,game_quality
 
             FROM public.full_game_information
           WHERE DATE(date_time AT TIME ZONE 'EST') <= $1
           AND DATE(date_time AT TIME ZONE 'EST') >= $2
           AND (srs_home > 0 OR home_wins > home_losses)
           AND (srs_away > 0 OR away_wins > away_losses)
           --AND ABS(srs_home - srs_away) < 5
           ORDER BY away_team_id, home_team_id, game_id DESC)
 as tmp1
 ORDER BY  game_quality DESC
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
             ,TO_CHAR(date_time AT TIME ZONE 'EST', 'YYYY-MM-DD') AS date_string
             ,TO_CHAR(date_time AT TIME ZONE 'EST', 'HH:MI AM TZ') AS time_string
             ,TO_CHAR(date_time AT TIME ZONE 'EST', 'YYYY-MM-DD HH:MI AM TZ') AS pretty_date_time 
             ,game_quality
         FROM public.full_game_information 
         WHERE DATE(date_time AT TIME ZONE 'EST') <= $1 
         AND DATE(date_time AT TIME ZONE 'EST') >= $2 
         ORDER BY ((win_pct_away + win_pct_home)) ASC
         LIMIT 1 
         ) ORDER BY date_time ASC 

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
        let game: Game = Game {
            id: record.game_id.unwrap(),
            away_team_id: record.away_team_id.unwrap(),
            away_team: record.away_team.unwrap(),
            home_team_id: record.home_team_id.unwrap(),
            home_team: record.home_team.unwrap(),
            srs_sum: record.srs_sum.unwrap().to_f64().unwrap(),
            pretty_date_time: record.pretty_date_time.unwrap(),
            date_string: record.date_string.unwrap(),
            time_string: record.time_string.unwrap(),
        };
        games.push(game);
    }

    Ok(games)
}

pub fn set_last_updated(current_update: chrono::DateTime<FixedOffset>) -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://127.0.0.1/")?;
    let mut con = client.get_connection()?;

    let prev_update = con
        .get("last_updated".to_string())
        .unwrap_or_else(|_| "2000-01-01T01:00:00-05:00".to_string());

    let prev_update = chrono::DateTime::parse_from_rfc3339(&prev_update).unwrap();
    if prev_update < current_update {
        let _: () = con.set("last_updated", current_update.to_rfc3339())?;
    }

    Ok(())
}

pub fn get_duration_since_update() -> redis::RedisResult<String> {
    let client = redis::Client::open("redis://127.0.0.1/")?;
    let mut con = client.get_connection()?;

    let last_updated = con
        .get("last_updated".to_string())
        .unwrap_or_else(|_| "2000-01-01T01:00:00-05:00".to_string());
    let last_updated = chrono::DateTime::parse_from_rfc3339(&last_updated).unwrap();

    let time_since_update = chrono::Utc::now() - last_updated.with_timezone(&chrono::Utc);
    dbg!(chrono::Utc::now().with_timezone(&chrono::Utc));
    dbg!(last_updated.with_timezone(&chrono::Utc));
    dbg!(time_since_update.num_minutes());
    match time_since_update.num_minutes() {
        0..=59 => Ok(format!(
            "Last Update: {minutes}min ago",
            minutes = time_since_update.num_minutes()
        )),
        60..=119 => Ok("Last Update: 1 hour ago".to_string()),
        _ => Ok(format!(
            "Last Update: {hours} hours ago",
            hours = time_since_update.num_hours()
        )),
    }
}

pub fn cache_games(games: Vec<Game>) -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://127.0.0.1/")?;
    let mut con = client.get_connection()?;
    for (game_number, game) in games.into_iter().enumerate() {
        let _: () = con.hset_multiple(
            game_number,
            &[
                ("id", game.id.to_string()),
                ("away_team_id", game.away_team_id.to_string()),
                ("away_team", game.away_team.to_owned()),
                ("home_team_id", game.home_team_id.to_string()),
                ("home_team", game.home_team.to_owned()),
                ("srs_sum", game.srs_sum.to_string()),
                ("pretty_date_time", game.pretty_date_time.to_owned()),
                ("date_string", game.date_string.to_owned()),
                ("time_string", game.time_string.to_owned()),
            ],
        )?;
        let _: () = con.expire(game_number, 60 * 60 * 24)?;
    }
    Ok(())
}

pub fn cache_to_games() -> redis::RedisResult<Vec<Game>> {
    let client = redis::Client::open("redis://127.0.0.1/")?;
    let mut con = client.get_connection()?;

    let mut games: Vec<Game> = Vec::new();

    for game_number in 0..=10 {
        let game = Game {
            id: con.hget(game_number, "id")?,
            away_team_id: con.hget(game_number, "away_team_id")?,
            away_team: con.hget(game_number, "away_team")?,
            home_team_id: con.hget(game_number, "home_team_id")?,
            home_team: con.hget(game_number, "home_team")?,
            srs_sum: con.hget(game_number, "srs_sum")?,
            pretty_date_time: con.hget(game_number, "pretty_date_time")?,
            date_string: con.hget(game_number, "date_string")?,
            time_string: con.hget(game_number, "time_string")?,
        };
        games.push(game);
    }
    Ok(games)
}

async fn polls_exist(pool: &PgPool) -> Result<bool, Error> {
    query!("SELECT EXISTS(SELECT * FROM polls)")
        .fetch_one(pool)
        .await?
        .exists
        .ok_or(Error::SQLxError(sqlx::Error::RowNotFound))
}
pub async fn stop_poll(pool: &PgPool, bot: &teloxide::Bot) -> Result<(), Error> {
    if !polls_exist(pool).await? {
        return Ok(());
    }

    let polls_to_close = query!(
        r#"
        SELECT id, local_id, chat_id FROM polls
       WHERE game_id IN
       (SELECT id FROM games WHERE now() at time zone 'EST' >= date_time AT TIME ZONE 'EST')
       AND is_open = True;
        "#
    )
    .fetch_all(pool)
    .await?;

    for poll in polls_to_close {
        let chat_id = poll.chat_id.unwrap_or(-1);
        dbg!("Closing Poll:", &poll, chat_id);
        match bot.stop_poll(chat_id, poll.local_id.unwrap()).send().await {
            Ok(_)
            | Err(RequestError::ApiError {
                kind: teloxide::ApiErrorKind::Known(KnownApiErrorKind::ChatNotFound),
                ..
            }) => {
                query!(
                    r#"
        UPDATE polls SET is_open = False WHERE id = $1
        "#,
                    poll.id
                )
                .execute(pool)
                .await?;
                continue;
            }
            Err(e) => {
                dbg!(e);
                continue;
            }
        }
    }

    Ok(())
}

pub async fn show_all_bets_season(
    pool: &PgPool,
    cx: &UpdateWithCx<Message>,
    chat_id: i64,
) -> Result<(), Error> {
    let ranking_query = query!(
        "SELECT * from correct_bets_season WHERE chat_id = $1 ORDER BY rank_number ASC",
        chat_id
    )
    .fetch_all(pool)
    .await?;

    let mut rankings = String::from("Fraction of correct bets for the whole season\n(including the ongoing week)\n\nRank |          Name          |    Correct Bets\n--- --- --- --- --- --- --- --- --- --- ---\n",
            );

    for record in ranking_query {
        let first_name = record.first_name.unwrap_or_else(|| "X".to_string());
        let mut spacing = String::from("");

        if let len @ 0..=13 = first_name.len() {
            for _ in 0..(13 - len) {
                spacing.push('\t')
            }
        }
        rankings.push_str(
            &format!(
                "    {rank}    | {spacing} {first_name} {spacing} | \t\t\t\t\t\t{correct_bets_total}/{finished_games}\n",
                rank = record.rank_number.unwrap_or(-1),
                first_name = first_name,
                spacing = spacing,
                finished_games = record.finished_games.unwrap_or(-1),
                correct_bets_total = record.correct_bets_total.unwrap_or(-1)
            )
            .as_str(),
        );
    }

    rankings.push('\n');
    rankings.push_str(&get_duration_since_update().unwrap_or_default());

    cx.answer(&rankings).send().await?;

    Ok(())
}

pub async fn number_of_finished_games_week(
    pool: &PgPool,
    chat_id: i64,
    week_number: i32,
) -> Result<u8, Error> {
    let row = query!(
        r#"
        SELECT 
            count(*) AS finished_games
        FROM
            polls JOIN games ON games.id = polls.game_id
            JOIN bet_weeks ON bet_weeks.id = polls.bet_week_id
        WHERE
            home_points > 0
            AND away_points > 0
            AND bet_weeks.week_number = $1
            AND polls.chat_id = $2;
        "#,
        week_number,
        chat_id
    )
    .fetch_optional(pool)
    .await?;

    if let Some(row_result) = row {
        return Ok(row_result.finished_games.unwrap_or(-1) as u8);
    } else {
        return Ok(0);
    }
}

pub async fn user_is_admin(chat_id: i64, cx: &UpdateWithCx<Message>) -> Result<bool, Error> {
    let admins = cx
        .bot
        .get_chat_administrators(chat_id)
        .send()
        .await
        .unwrap_or_default();

    Ok(admins
        .iter()
        .map(|chat_member| chat_member.user.id)
        .any(|x| x == cx.update.from().unwrap().id))
}

pub async fn remove_chat(pool: &PgPool, chat_id: i64) -> Result<(), Error> {
    query!("DELETE FROM bets WHERE chat_id = $1", chat_id)
        .execute(pool)
        .await?;
    query!("DELETE FROM polls WHERE chat_id = $1", chat_id)
        .execute(pool)
        .await?;
    query!("DELETE FROM bet_weeks WHERE chat_id = $1", chat_id)
        .execute(pool)
        .await?;
    query!("DELETE FROM chats WHERE id = $1", chat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn change_active_chat_status(
    pool: &PgPool,
    chat_id: i64,
    new_status: bool,
) -> Result<(), Error> {
    query!(
        "UPDATE chats SET is_active = $1 WHERE id = $2",
        new_status,
        chat_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn chat_is_known(pool: &PgPool, chat_id: i64) -> Result<bool, Error> {
    let is_known = query!("SELECT EXISTS(SELECT * FROM chats WHERE id = $1)", chat_id)
        .fetch_one(pool)
        .await?;

    is_known
        .exists
        .ok_or(Error::SQLxError(sqlx::Error::RowNotFound))
}

pub async fn add_bet(
    pool: &PgPool,
    game_id: i32,
    chat_id: i64,
    user_id: i64,
    bet: i32,
    poll_id: String,
) -> Result<(), Error> {
    query!(
        r#"
        INSERT INTO bets(game_id, chat_id, user_id, bet, poll_id) VALUES 
        ($1, $2, $3, $4, $5)
        ON CONFLICT DO NOTHING;
        "#,
        game_id,
        chat_id,
        user_id,
        bet,
        poll_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn bet_to_team_id(pool: &PgPool, bet: i32, game_id: i32) -> Result<i32, Error> {
    // bet is 0 if first option was picked (the away team)
    // bet is 1 if second option was picked (the home team)
    match bet {
        0 => query!(
            r#"
            SELECT away_team FROM games WHERE id = $1;
            "#,
            game_id
        )
        .fetch_one(pool)
        .await?
        .away_team
        .ok_or(Error::SQLxError(sqlx::Error::RowNotFound)),
        1 => query!(
            r#"
            SELECT home_team FROM games WHERE id = $1;
            "#,
            game_id
        )
        .fetch_one(pool)
        .await?
        .home_team
        .ok_or(Error::SQLxError(sqlx::Error::RowNotFound)),
        _ => panic!("Could not convert bet to team_id!"),
    }
}

pub async fn get_chat_id_game_id_from_poll(
    pool: &PgPool,
    poll_id: String,
) -> Result<(i64, i32), Error> {
    dbg!(&poll_id);
    let row = query!(
        r#"
        SELECT chat_id, game_id FROM polls WHERE id = $1;
        "#,
        poll_id
    )
    .fetch_one(pool)
    .await?;

    Ok((row.chat_id.unwrap_or(-1), row.game_id.unwrap_or(-1)))
}

pub async fn user_is_in_db(pool: &PgPool, user_id: i64) -> Result<bool, Error> {
    query!("SELECT EXISTS(SELECT * FROM users WHERE id = $1)", user_id)
        .fetch_one(pool)
        .await?
        .exists
        .ok_or(Error::SQLxError(sqlx::Error::RowNotFound))
}

pub async fn add_user(
    pool: &PgPool,
    user_id: i64,
    first_name: String,
    last_name: String,
    username: String,
    language_code: String,
) -> Result<(), Error> {
    query!(
        r#"
            INSERT INTO users(id, first_name, last_name, username, language_code) VALUES
            ($1, $2, $3, $4, $5);
            "#,
        user_id,
        first_name,
        last_name,
        username,
        language_code,
    )
    .execute(pool)
    .await
    .unwrap_or_default();

    Ok(())
}

pub async fn show_complete_rankings(
    cx: &UpdateWithCx<Message>,
    pool: &PgPool,
    chat_id: i64,
) -> Result<(), Error> {
    let ranking_query = query!(
        r#"
        SELECT 
         first_name
         ,last_name
         ,username
         ,chat_id
         ,SUM(CASE WHEN rank_number = 1 THEN 1 ELSE 0 END)  as weeks_won
         ,RANK() OVER (partition by chat_id ORDER BY SUM(CASE WHEN rank_number = 1 THEN 1 ELSE 0 END) DESC )
        FROM weekly_rankings WHERE chat_id = $1 
    GROUP BY
    first_name
    ,last_name
    ,username
    ,chat_id
    ORDER BY weeks_won DESC;
        "#,
        chat_id


    ).fetch_all(pool).await?;

    let mut rankings = String::from(
        "Standings (including current week)\n\nRank |          Name          |    Weeks Won\n--- --- --- --- --- --- --- --- --- --- ---\n",
            );

    for record in ranking_query {
        let first_name = record.first_name.unwrap_or_else(|| "X".to_string());
        let mut spacing = String::from("");

        if let len @ 0..=13 = first_name.len() {
            for _ in 0..(13 - len) {
                spacing.push('\t')
            }
        }
        rankings.push_str(
            &format!(
                "    {rank}    | {spacing} {first_name} {spacing} | \t\t\t\t\t\t\t\t\t{weeks_won}\n",
                rank = record.rank.unwrap_or(-1),
                first_name = first_name,
                spacing = spacing,
                weeks_won = record.weeks_won.unwrap_or(-1)
            )
            .as_str(),
        );
    }

    rankings.push('\n');
    rankings.push_str(&get_duration_since_update().unwrap_or_default());

    cx.answer(&rankings).send().await?;

    Ok(())
}

pub async fn show_game_results(
    cx: &UpdateWithCx<Message>,
    pool: &PgPool,
    chat_id: i64,
    week_number: i32,
) -> Result<(), Error> {
    let started_games = query!(
        r#"
        SELECT game_id,away_team, away_points, home_team, home_points, date_time
        FROM full_game_information
        WHERE 
        NOW() AT TIME ZONE 'EST' >= date_time AT TIME ZONE 'EST'
        AND
        game_id IN (SELECT game_id from polls join bet_weeks ON polls.bet_week_id = bet_weeks.id WHERE polls.chat_id = $1 AND week_number=$2)
        ORDER BY date_time ASC
        "#,
        chat_id,
        week_number
    ).fetch_all(pool).await?;

    let tmp = started_games.get(0);
    if tmp.is_none() {
        cx.answer_str("Seems like no games have been played this week so far!")
            .await?;
        return Ok(());
    }

    let mut game_results = String::from("Game Results:\n");
    game_results.push('\n');

    for game in started_games {
        let game_id = game.game_id.unwrap_or_default();

        let away_team = game.away_team.unwrap_or_default();
        let home_team = game.home_team.unwrap_or_default();
        let away_points = game.away_points.unwrap_or_default();
        let home_points = game.home_points.unwrap_or_default();
        game_results.push_str(&format!(
            "{away_points} {away_team}\n{home_points} {home_team}\n\nCorrect Bet:\n",
            away_points = away_points,
            away_team = away_team,
            home_points = home_points,
            home_team = home_team
        ));

        let correct_bet_users = query!(
            r#"
            SELECT first_name from users
            WHERE users.id IN (SELECT user_id FROM correct_bets WHERE game_id = $1 AND chat_id=$2)
            "#,
            game_id,
            chat_id
        )
        .fetch_all(pool)
        .await?;

        for user in correct_bet_users {
            let first_name = user.first_name.unwrap_or_default();
            game_results.push_str(&format!("{}\n", first_name));
        }
        game_results.push('\n');
    }
    cx.answer(&game_results).send().await?;

    Ok(())
}

pub async fn show_week_rankings(
    cx: &UpdateWithCx<Message>,
    pool: &PgPool,
    chat_id: i64,
    week_number: i32,
) -> Result<(), Error> {
    let ranking_query = query!(
        r#"
        SELECT first_name
        ,last_name
        ,username
        ,correct_bets_week
        ,week_number
        ,rank_number
        FROM weekly_rankings
        WHERE
        chat_id = $1
        AND 
                week_number = CASE WHEN $2 = -1 THEN (SELECT MAX(week_number)
                                FROM weekly_rankings
                                WHERE chat_id = $1
                                AND start_date AT TIME ZONE 'EST' <= NOW() AT TIME ZONE 'EST' - INTERVAL '1 DAYS')
                                ELSE $2
                                END
        ORDER BY correct_bets_week DESC;

        "#,
        chat_id
        ,week_number
    )
    .fetch_all(pool);

    let ranking_query = ranking_query.await?;

    let week_number;
    let week_number_raw = &ranking_query.get(0);
    if week_number_raw.is_none() {
        cx.answer_str("You can see the standings a couple hours after your first game is finished.\nMake sure to answer at least one poll!").await?;
        return Ok(());
    } else {
        week_number = week_number_raw.unwrap().week_number.unwrap_or(-1);
    }

    let finished_games = number_of_finished_games_week(pool, chat_id, week_number).await?;
    let mut rankings = format!("Week {week_number}\nYou get one point for every correct bet\nSend /help to see more commands\n\n\nRank |          Name          |    Points\n--- --- --- --- --- --- --- --- --- --\n",
            week_number = week_number);

    for record in ranking_query {
        let first_name = record.first_name.unwrap_or_else(|| "X".to_string());
        let mut spacing = String::from("");

        if let len @ 0..=13 = first_name.len() {
            for _ in 0..(13 - len) {
                spacing.push('\t')
            }
        }
        rankings.push_str(
            &format!(
                "    {rank}    | {spacing} {first_name} {spacing} | \t\t\t\t{correct_bets_week}/{finished_games}\n",
                rank = record.rank_number.unwrap_or(-1),
                first_name = first_name,
                spacing = spacing,
                finished_games = finished_games,
                correct_bets_week = record.correct_bets_week.unwrap_or(-1)
            )
            .as_str(),
        );
    }

    rankings.push('\n');
    rankings.push_str(&get_duration_since_update().unwrap_or_default());

    cx.answer(&rankings).send().await?;

    Ok(())
}

fn _get_duration_since_update() -> String {
    let now = chrono::Utc::now();

    let yesterday = now - Duration::hours(24);
    let last_scrape = match now.time().hour() {
        0..=8 => Utc
            .ymd(yesterday.year(), yesterday.month(), yesterday.day())
            .and_hms(9, 30, 0),
        9 => match now.time().minute() {
            0..=29 => Utc
                .ymd(yesterday.year(), yesterday.month(), yesterday.day())
                .and_hms(9, 30, 0),
            _ => Utc
                .ymd(now.year(), now.month(), now.day())
                .and_hms(9, 30, 0),
        },
        _ => Utc
            .ymd(now.year(), now.month(), now.day())
            .and_hms(9, 30, 0),
    };

    let time_since_update = now - last_scrape;

    match time_since_update.num_minutes() {
        0..=59 => {
            format!(
                "Last Update: {minutes}min ago",
                minutes = time_since_update.num_minutes()
            )
        }
        60..=119 => "Last Update: 1 hour ago".to_string(),
        _ => format!(
            "Last Update: {hours} hours ago",
            hours = time_since_update.num_hours()
        ),
    }
}

#[derive(Debug)]
pub struct Game {
    id: i32,
    away_team_id: i32,
    away_team: String,
    home_team_id: i32,
    home_team: String,
    srs_sum: f64,
    pretty_date_time: String,
    date_string: String,
    time_string: String,
}

#[derive(Debug)]
pub struct BetWeek {
    pub id: i32,
    pub week_number: i32,
    pub start_date: chrono::NaiveDate,
    pub end_date: chrono::NaiveDate,
    pub polls_sent: bool,
}
