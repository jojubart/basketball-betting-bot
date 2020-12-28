use lazy_static::lazy_static;
use sqlx::postgres::PgPool;
use std::convert::Infallible;
use std::env;
use teloxide::prelude::*;
use teloxide::requests::RequestWithFile;

use basketball_betting_bot::{get_token, Error, get_active_chat_status };
use utils::{number_of_finished_games_week};

lazy_static! {
    static ref BOT_TOKEN: String = get_token("../config.ini");
}

// transitions.rs
mod transitions;
use teloxide::prelude::*;
use teloxide_macros::teloxide;
mod utils;
use utils::send_polls;

#[teloxide(subtransition)]
async fn ready(_state: ReadyState, cx: TransitionIn, ans: String) -> TransitionOut<Dialogue> {
    dbg!("READY");
    let pool = PgPool::connect(
        &env::var("DATABASE_URL").expect("Could not find DATABASE_URL environment variable!"),
    )
    .await;

    if let Err(e) = pool {
        dbg!(e);
        return next(ReadyState);
    }

    let pool = pool.unwrap();

    let chat_id = cx.chat_id();
    let chat_is_known = chat_is_known(&pool, chat_id).await.unwrap_or(false);
    if !chat_is_known {
        sqlx::query!(
            "INSERT INTO chats(id) VALUES ($1) ON CONFLICT DO NOTHING",
            chat_id
        )
        .execute(&pool)
        .await;
    }

    let ans = ans.as_str();

    // if the chat was not yet marked as active and they send a message other than start
    // we'll send them to the SetupState where they can 
    if !get_active_chat_status(&pool, chat_id).await.unwrap_or(false)
        && (ans != "/start" && ans != "/start@BasketballBettingBot")
    {
        cx.answer_str("Send /start to begin your season!").await?;
        return next(ReadyState);
    }

    match ans {
        "/start" | "/start@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            if get_active_chat_status(&pool, chat_id).await.unwrap_or(false) {
                cx.answer_str("Looks like you've started your season already!").await?;
                return next(ReadyState);
            }
            change_active_chat_status(&pool, chat_id, true)
                .await;
            cx.answer_str(r#"BasketballBettingBot sends you 11 NBA games to bet on each week, 10 good ones and one battle between the supreme tank commanders. The one who gets the most games right in a week gets one point.
You play against the other members of your group and the winner is the one who wins the most weeks."#).await?;
            cx.answer_str("Your season begins now!").await?;
            send_polls(&pool, chat_id, &cx.bot).await;
            dbg!("SEASONS STARTS");
            return next(ReadyState);
        }

        "/standings" | "/standings@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            //show_rankings(cx, &pool, chat_id).await;
            show_week_rankings(&cx, &pool, chat_id).await;
        }
        "/full_standings" | "/full_standings@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            show_complete_rankings(&cx, &pool, chat_id).await;
        }
        "/stop_season" | "/stop_season@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            if user_is_admin(chat_id, &cx).await.unwrap_or(false) {
                cx.answer_str("Send /end_my_season to end the season.\n
Afterwards you will get the standings of this week and the complete results table.\n
YOU CAN'T UNDO THIS ACTION AND ALL YOUR BETS AND RESULTS ARE LOST!").await?;
                return next(StopState);
            } else {
                cx.answer_str("Only the group admins can stop the season!").await?;
            }
        }
        "/sage" | "/sage@BasketballBettingBot" => {
            let photo = teloxide::types::InputFile::Url(
                "https://media.giphy.com/media/zLVTQRSiCm2a8kljMq/giphy.gif".to_string(),
            );

            cx.answer_animation(photo).send().await;
        }
        "/help" | "/help@BasketballBettingBot" => {
            cx.answer_str(r#"BasketballBettingBot sends you 11 NBA games to bet on each week, 10 good ones and one battle between the supreme tank commanders. The one who gets the most games right in a week gets one point.
You play against the other members of your group and the winner is the one who wins the most weeks.
Once everyone who wants to participate is in this group, send /start to begin if you haven't done so already!

/standings to see who's the GOAT bettor this week.
/sage to cleanse the energy of this chat"#).await?;
        }
        _ => (),
    }

    next(ReadyState)
}

#[teloxide(subtransition)]
async fn stop_season(_state: StopState, cx: TransitionIn, ans: String) -> TransitionOut<Dialogue> { 
    let pool = PgPool::connect(
        &env::var("DATABASE_URL").expect("Could not find DATABASE_URL environment variable!"),
    )
    .await;

    if let Err(e) = pool {
        dbg!(e);
        return next(ReadyState);
    }
    let pool = pool.expect("Could not establish DB connection!");
    

    dbg!("StopState");
    let chat_id = cx.update.chat_id();
    if !user_is_admin(chat_id, &cx).await.unwrap_or(false) {
        cx.answer_str("Only the group admins can stop the chat!").await?;
        return next(ReadyState);
    }
    match ans.as_str() {
        "/end_my_season" => {
            show_week_rankings(&cx, &pool, chat_id).await;
            show_complete_rankings(&cx, &pool, chat_id).await;
            remove_chat(&pool, chat_id).await;
            cx.answer_str("SEASON ENDED").await?;
        }
        _ => {cx.answer_str("The season continues!").await?;
    }}
    next(ReadyState)
}

async fn user_is_admin(chat_id: i64, cx: &UpdateWithCx<Message>) -> Result<bool, Error> {
    let admins = cx.bot.get_chat_administrators(chat_id).send().await.unwrap_or_default();

    Ok(admins.iter().map(|chat_member| chat_member.user.id)
                 .any(|x| x == cx.update.from().unwrap().id))

}

async fn remove_chat(pool: &PgPool, chat_id: i64) -> Result<(), Error> {

    sqlx::query!("DELETE FROM bets WHERE chat_id = $1", chat_id).execute(pool).await?;
    sqlx::query!("DELETE FROM polls WHERE chat_id = $1", chat_id).execute(pool).await?;
    sqlx::query!("DELETE FROM bet_weeks WHERE chat_id = $1", chat_id).execute(pool).await?;
    sqlx::query!("DELETE FROM chats WHERE id = $1", chat_id).execute(pool).await?;
    Ok(())
}

async fn change_active_chat_status(
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

async fn chat_is_known(pool: &PgPool, chat_id: i64) -> Result<bool, Error> {
    let is_known = sqlx::query!("SELECT EXISTS(SELECT * FROM chats WHERE id = $1)", chat_id)
        .fetch_one(pool)
        .await?;

    is_known.exists.ok_or(Error::SQLxError(sqlx::Error::RowNotFound))



}

// states.rs
//use derive_more;
mod states;
use teloxide_macros::Transition;

use serde::{Deserialize, Serialize};

#[derive(Transition, derive_more::From, Serialize, Deserialize)]
pub enum Dialogue {
    Stop(StopState),
    Ready(ReadyState),
}

impl Default for Dialogue {
    fn default() -> Self {
        Self::Ready(ReadyState)
    }
}

#[derive(Serialize, Deserialize)]
pub struct StopState;

#[derive(Serialize, Deserialize)]
pub struct ReadyState;

// main.rs

type In = DialogueWithCx<Message, Dialogue, Infallible>;
#[tokio::main]
async fn main() {
    run().await;
}

async fn run() {
    teloxide::enable_logging!();
    log::info!("Starting the bot!");

    let bot = Bot::new(BOT_TOKEN.to_owned());

    Dispatcher::new(bot)
        .messages_handler(DialogueDispatcher::new(
            |DialogueWithCx { cx, dialogue }: In| async move {
                let dialogue = dialogue.expect("std::convert::Infallible");
                handle_message(cx, dialogue)
                    .await
                    .expect("Something wrong with the bot!")
            },
        ))
        .poll_answers_handler(|rx: DispatcherHandlerRx<teloxide::types::PollAnswer>| {
            rx.for_each_concurrent(None, |poll_answer| async move {
                let pool = PgPool::connect(
                    &env::var("DATABASE_URL")
                        .expect("Could not find environment variable DATABASE_URL"),
                )
                .await
                .expect("Could not establish connection do database");

                handle_poll_answer(poll_answer, &pool).await;
            })
        })
        .dispatch()
        .await;
}

async fn handle_message(cx: UpdateWithCx<Message>, dialogue: Dialogue) -> TransitionOut<Dialogue> {
    match cx.update.text_owned() {
        None => {
            //cx.answer_str("Send me a text message").await?;
            next(dialogue)
        }
        Some(ans) => dialogue.react(cx, ans).await,
    }
}

async fn handle_poll_answer(
    cx: UpdateWithCx<teloxide::types::PollAnswer>,
    pool: &PgPool,
) -> Result<(), Error> {
    println!("{:?}", cx.update.option_ids);
    // check if it's a poll that the bot sent
    // is probably unnecessary, since per the official docs poll answers not sent by the bot itself
    // are ignored. Since this could change in the future, I'm gonna play it safe.
    if !poll_is_in_db(pool, cx.update.poll_id.clone())
        .await
        .expect("could not get poll_id!")
    {
        return Ok(());
    }
    let (chat_id, game_id) = get_chat_id_game_id_from_poll(pool, cx.update.poll_id.clone())
        .await
        .expect("Could not get chat_id");

    if !user_is_in_db(pool, cx.update.user.id as i64)
        .await
        .expect("Could not determine if user is in database")
    {
        dbg!("adding user to db");
        add_user(
            pool,
            cx.update.user.id as i64,
            cx.update.user.first_name,
            cx.update.user.last_name.unwrap_or_else(|| "".to_string()),
            cx.update.user.username.unwrap_or_else(|| "".to_string()),
            cx.update.user.language_code.unwrap_or_else(|| "en".to_string()),
            chat_id,
        )
        .await?;
    }

    let bet = bet_to_team_id(pool, cx.update.option_ids[0], game_id)
        .await
        .expect("Could not convert bet to team_id");
    dbg!(bet);

    add_bet(
        pool,
        game_id,
        chat_id,
        cx.update.user.id as i64,
        bet,
        cx.update.poll_id,
    )
    .await?;

    Ok(())
}

async fn poll_is_in_db(pool: &PgPool, poll_id: String) -> Result<bool, Error> {
    sqlx::query!(
        "SELECT EXISTS(SELECT id from polls WHERE id = $1);",
        poll_id
    )
    .fetch_one(pool)
    .await?
    .exists
    .ok_or(Error::SQLxError(sqlx::Error::RowNotFound))
}

async fn add_bet(
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

async fn bet_to_team_id(pool: &PgPool, bet: i32, game_id: i32) -> Result<i32, Error> {
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
        .ok_or(Error::SQLxError(sqlx::Error::RowNotFound))
        ,
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

async fn get_chat_id_game_id_from_poll(
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

async fn user_is_in_db(pool: &PgPool, user_id: i64) -> Result<bool, Error> {
        sqlx::query!("SELECT EXISTS(SELECT * FROM users WHERE id = $1)", user_id)
            .fetch_one(pool)
            .await?
            .exists
            .ok_or(Error::SQLxError(sqlx::Error::RowNotFound))

}

async fn add_user(
    pool: &PgPool,
    user_id: i64,
    first_name: String,
    last_name: String,
    username: String,
    language_code: String,
    chat_id: i64,
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
    .await;

    sqlx::query!(
        r#"
            INSERT INTO points(chat_id, user_id) VALUES
            ($1, $2)
            "#,
        chat_id,
        user_id
    )
    .execute(pool)
    .await;

    Ok(())
}

//async fn show_rankings(
//    cx: UpdateWithCx<Message>,
//    pool: &PgPool,
//    chat_id: i64,
//) -> Result<(), Error> {
//    let ranking_query = sqlx::query!(
//        r#"
//        SELECT first_name, last_name, username, points FROM rankings WHERE chat_id = $1 ORDER BY points DESC;
//        "#,
//        chat_id
//    )
//    .fetch_all(pool)
//    .await?;
//
//    let mut rank: i32 = 0;
//    //let mut rankings = ranking_query
//    //    .into_iter()
//    //    .map(|record| format!("{}\n", record.first_name.unwrap().as_str()))
//    //    .collect::<String>();
//    let mut rankings = String::from("Rank | Name | Correct Bets\n--- | --- | ---");
//
//    for record in ranking_query {
//        rankings.push_str(
//            &format!(
//                "{rank}. {first_name} {last_name} {username}\nPoints: {points}\n",
//                rank = {
//                    rank += 1;
//                    rank
//                },
//                first_name = record.first_name.unwrap(),
//                last_name = record.last_name.unwrap(),
//                username = {
//                    let username = record.username.unwrap();
//                    match username.as_str() {
//                        "" => format!(""),
//                        _ => format!("@{}", username),
//                    }
//                },
//                points = record.points.unwrap()
//            )
//            .as_str(),
//        );
//    }
//
//    dbg!(&rankings);
//    cx.answer(&rankings)
//        //.parse_mode(teloxide::types::ParseMode::MarkdownV2)
//        .send()
//        .await;
//    cx.answer_str(rankings).await;
//    Ok(())
//}
async fn show_complete_rankings(cx: &UpdateWithCx<Message>, pool: &PgPool, chat_id: i64) -> Result<(), Error> {
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

    let mut rankings = 
        String::from("Standings (include the ongoing week)\n\nRank |          Name          |    Weeks Won\n--- --- --- --- --- --- --- --- --- --- ---\n",
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
                //     last_name = record.last_name.unwrap(),
                //     username = {
                //         let username = record.username.unwrap();
                //         match username.as_str() {
                //             "" => format!(""),
                //             _ => format!("@{}", username),
                //         }
                //     },
                weeks_won = record.weeks_won.unwrap_or(-1)
            )
            .as_str(),
        );
    }

    cx.answer(&rankings).send().await?;

   Ok(())



}

async fn show_week_rankings(
    cx: &UpdateWithCx<Message>,
    pool: &PgPool,
    chat_id: i64,
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
    .fetch_all(pool)
    .await?;

    let finished_games = number_of_finished_games_week(pool, chat_id).await?;


    let week_number;
    let week_number_raw = &ranking_query.get(0);
    if week_number_raw.is_none() {
        cx.answer_str("You can see the standings tomorrow after your first round of games is finished.\nAlso, make sure to answer at least one poll to see the standings!").await?;
        return Ok(())
    } else {
         week_number = week_number_raw.unwrap().week_number.unwrap_or(-1);
    }
    let mut rankings = 
        format!("Week {week_number}\nYou get one point for every correct bet\n\nRank |          Name          |    Points\n--- --- --- --- --- --- --- --- --- --\n",
            week_number = 
            week_number);

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
                //     last_name = record.last_name.unwrap(),
                //     username = {
                //         let username = record.username.unwrap();
                //         match username.as_str() {
                //             "" => format!(""),
                //             _ => format!("@{}", username),
                //         }
                //     },
                correct_bets_week = record.correct_bets_week.unwrap_or(-1)
            )
            .as_str(),
        );
    }

    cx.answer(&rankings).send().await?;

    Ok(())
}
