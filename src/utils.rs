use crate::Error;
use sqlx::{postgres::PgPool, types::BigDecimal};
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

async fn _update_bet_week(pool: &PgPool, bet_week_id: i32) -> anyhow::Result<()> {
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
    .await?;
    Ok(())
}

pub async fn get_bet_week(pool: &PgPool, chat_id: i64) -> Result<BetWeek, Error> {
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
    if poll_is_in_db(&pool, game_id, chat_id).await? {
        eprintln!("entry already in polls table!");
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
    sqlx::query!(
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
    sqlx::query!(
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
            ,to_char(date_time AT TIME ZONE 'EST', 'YYYY-MM-DD HH:MI AM TZ') AS pretty_time
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
            ,to_char(date_time AT TIME ZONE 'EST', 'YYYY-MM-DD HH:MI AM TZ') AS pretty_time
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

async fn polls_exist(pool: &PgPool) -> Result<bool, Error> {
    sqlx::query!("SELECT EXISTS(SELECT * FROM polls)")
        .fetch_one(pool)
        .await?
        .exists
        .ok_or(Error::SQLxError(sqlx::Error::RowNotFound))
}
pub async fn stop_poll(pool: &PgPool, bot: &teloxide::Bot) -> Result<(), Error> {
    if !polls_exist(pool).await? {
        return Ok(());
    }
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
        let chat_id = poll.chat_id.unwrap_or(-1);
        match bot.stop_poll(chat_id, poll.local_id.unwrap()).send().await {
            Ok(_) => (),
            Err(_) => continue,
        }

        sqlx::query!(
            r#"
        UPDATE polls SET is_open = False WHERE id = $1
        "#,
            poll.id
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn show_all_bets_season(
    pool: &PgPool,
    cx: &UpdateWithCx<Message>,
    chat_id: i64,
) -> Result<(), Error> {
    let ranking_query = sqlx::query!(
        "SELECT * from correct_bets_season WHERE chat_id = $1 ORDER BY rank_number ASC",
        chat_id
    )
    .fetch_all(pool)
    .await?;

    let mut rankings = String::from("All Bets (include the ongoing week)\n\nRank |          Name          |    Correct Bets\n--- --- --- --- --- --- --- --- --- --- ---\n",
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

    cx.answer(&rankings).send().await?;

    Ok(())
}

pub async fn number_of_finished_games_week(
    pool: &PgPool,
    chat_id: i64,
    week_number: i32,
) -> Result<u8, Error> {
    let row = sqlx::query!(
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
    sqlx::query!("DELETE FROM bets WHERE chat_id = $1", chat_id)
        .execute(pool)
        .await?;
    sqlx::query!("DELETE FROM polls WHERE chat_id = $1", chat_id)
        .execute(pool)
        .await?;
    sqlx::query!("DELETE FROM bet_weeks WHERE chat_id = $1", chat_id)
        .execute(pool)
        .await?;
    sqlx::query!("DELETE FROM chats WHERE id = $1", chat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn change_active_chat_status(
    pool: &PgPool,
    chat_id: i64,
    new_status: bool,
) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE chats SET is_active = $1 WHERE id = $2",
        new_status,
        chat_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn chat_is_known(pool: &PgPool, chat_id: i64) -> Result<bool, Error> {
    let is_known = sqlx::query!("SELECT EXISTS(SELECT * FROM chats WHERE id = $1)", chat_id)
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
    sqlx::query!(
        r#"
        INSERT INTO bets(game_id, chat_id, user_id, bet, poll_id) VALUES 
        ($1, $2, $3, $4, $5);
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
        0 => sqlx::query!(
            r#"
            SELECT away_team FROM games WHERE id = $1;
            "#,
            game_id
        )
        .fetch_one(pool)
        .await?
        .away_team
        .ok_or(Error::SQLxError(sqlx::Error::RowNotFound)),
        1 => sqlx::query!(
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
    let row = sqlx::query!(
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
    sqlx::query!("SELECT EXISTS(SELECT * FROM users WHERE id = $1)", user_id)
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
    sqlx::query!(
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
    let ranking_query = sqlx::query!(
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
        "Standings (include the ongoing week)\n\nRank |          Name          |    Weeks Won\n--- --- --- --- --- --- --- --- --- --- ---\n",
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

    cx.answer(&rankings).send().await?;

    Ok(())
}

pub async fn show_week_rankings(
    cx: &UpdateWithCx<Message>,
    pool: &PgPool,
    chat_id: i64,
    week_number: i32,
) -> Result<(), Error> {
    let ranking_query = sqlx::query!(
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
                week_number = (SELECT MAX(week_number)
                                FROM weekly_rankings
                                WHERE chat_id = $1
                                AND start_date AT TIME ZONE 'EST' <= NOW() AT TIME ZONE 'EST' - INTERVAL '1 DAYS')
        ORDER BY correct_bets_week DESC;

        "#,
        chat_id
    )
    .fetch_all(pool);

    if week_number != -1 {
        #[allow(unused_variables)] // variable is not unused - suppress warning
        let ranking_query = sqlx::query!(
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
                week_number = $2
        ORDER BY correct_bets_week DESC;

        "#,
            chat_id,
            week_number
        )
        .fetch_all(pool);
    }

    let ranking_query = ranking_query.await?;

    let week_number;
    let week_number_raw = &ranking_query.get(0);
    if week_number_raw.is_none() {
        cx.answer_str("You can see the standings tomorrow after your first round of games is finished.\nAlso, make sure to answer at least one poll to see the standings!").await?;
        return Ok(());
    } else {
        week_number = week_number_raw.unwrap().week_number.unwrap_or(-1);
    }

    let finished_games = number_of_finished_games_week(pool, chat_id, week_number).await?;
    let mut rankings = format!("Week {week_number}\nYou get one point for every correct bet\n\nRank |          Name          |    Points\n--- --- --- --- --- --- --- --- --- --\n",
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

    cx.answer(&rankings).send().await?;

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
pub struct BetWeek {
    pub id: i32,
    pub week_number: i32,
    pub start_date: chrono::NaiveDate,
    pub end_date: chrono::NaiveDate,
    pub polls_sent: bool,
}
