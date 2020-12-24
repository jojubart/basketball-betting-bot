use ini::Ini;
use lazy_static::lazy_static;
use sqlx::{postgres::PgPool, Done};
use std::convert::Infallible;
use std::env;
use std::thread::{self, sleep};
use std::time::Duration;
use teloxide::prelude::*;

use basketball_betting_bot::{east_coast_date_in_x_days, east_coast_date_today, get_token, Error};

lazy_static! {
    static ref BOT_TOKEN: String = get_token("config.ini");
}

// transitions.rs
// use teloxide::prelude::*;
//use super::states::*;
use teloxide_macros::teloxide;
mod utils;
use utils::send_polls;

#[teloxide(subtransition)]
async fn setup(state: SetupState, cx: TransitionIn, ans: String) -> TransitionOut<Dialogue> {
    dbg!("SETUP");
    let chat_id: i64 = cx.chat_id();
    let pool = PgPool::connect(
        &env::var("DATABASE_URL").expect("Could not find DATABASE_URL environment variable!"),
    )
    .await
    .expect("Could not establish connection to database");

    //let chat_is_known = sqlx::query!("SELECT * FROM chats WHERE id = $1", chat_id)
    //    .fetch_one(&pool)
    //    .await;

    //if let Err(error) = chat_is_known {
    //    println!("Error {:?}", error);
    //    sqlx::query!(
    //        "INSERT INTO chats(id) VALUES ($1) ON CONFLICT DO NOTHING",
    //        chat_id
    //    )
    //    .execute(&pool)
    //    .await;
    //}

    let admins = cx
        .bot
        .get_chat_administrators(cx.chat_id())
        .send()
        .await
        .unwrap_or(vec![]); // no admin present in non-group chats

    let chat_member = &cx
        .update
        .from()
        .expect("Could not get information of the user!")
        .first_name;
    //if get_active_chat_status(&pool, chat_id)
    //    .await
    //    .unwrap_or(false)
    //    == true
    //{
    //    match ans.as_str() {
    //        "/rankings" | "/rankings@BasketballBettingBot" => {
    //            let chat_id = cx.update.chat_id();
    //            dbg!("setup ranking");
    //            show_rankings(cx, &pool, chat_id).await;
    //        }
    //        _ => (),
    //    }
    //    return next(ReadyState);
    //}

    //cx.answer_str("Once you're ready to start the season, send '/start' into this chat and you can start betting on some games!").await?;
    match ans.as_str() {
        "/start" | "/start@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            change_active_chat_status(&pool, chat_id, true)
                .await
                .unwrap();
            cx.answer_str("Your season begins now!").await;
            send_polls(&pool, chat_id, &cx.bot).await.unwrap();
            dbg!("SEASONS STARTS");
            return next(ReadyState);
        }
        _ => {
            cx.answer_str("Send /start to begin your season!").await;
            return next(SetupState);
        }
    }

    next(SetupState)
}

#[teloxide(subtransition)]
async fn ready(state: ReadyState, cx: TransitionIn, ans: String) -> TransitionOut<Dialogue> {
    dbg!("READY");
    let pool = PgPool::connect(
        &env::var("DATABASE_URL").expect("Could not find DATABASE_URL environment variable!"),
    )
    .await
    .expect("Could not establish connection to database");

    let chat_id = cx.chat_id();
    let chat_is_known = sqlx::query!("SELECT * FROM chats WHERE id = $1", chat_id)
        .fetch_one(&pool)
        .await;

    if let Err(error) = chat_is_known {
        dbg!("{chat_id} is now added to known chats", chat_id);
        sqlx::query!(
            "INSERT INTO chats(id) VALUES ($1) ON CONFLICT DO NOTHING",
            chat_id
        )
        .execute(&pool)
        .await;
    }

    let ans = ans.as_str();

    if (get_active_chat_status(&pool, chat_id).await.unwrap() == false)
        && (ans != "/start" || ans != "/start@BasketballBettingBot")
    {
        cx.answer_str("Send /start to begin your season!").await;
        return next(SetupState);
    }

    match ans {
        "/start" | "/start@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            change_active_chat_status(&pool, chat_id, true)
                .await
                .unwrap();
            cx.answer_str("Your season begins now!").await;
            send_polls(&pool, chat_id, &cx.bot).await.unwrap();
            dbg!("SEASONS STARTS");
            return next(ReadyState);
        }

        "/rankings" | "/rankings@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            show_rankings(cx, &pool, chat_id).await;
        }
        _ => (),
    }

    next(ReadyState)
}

async fn get_active_chat_status(pool: &PgPool, chat_id: i64) -> Result<bool, Error> {
    Ok(
        sqlx::query!("SELECT is_active FROM chats WHERE id = $1", chat_id)
            .fetch_one(pool)
            .await?
            .is_active
            .unwrap(),
    )
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

// states.rs
//use derive_more;
use teloxide_macros::Transition;

use serde::{Deserialize, Serialize};

#[derive(Transition, derive_more::From, Serialize, Deserialize)]
pub enum Dialogue {
    Setup(SetupState),
    Ready(ReadyState),
}

impl Default for Dialogue {
    fn default() -> Self {
        Self::Ready(ReadyState)
    }
}

#[derive(Serialize, Deserialize)]
pub struct SetupState;

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
    let pool = PgPool::connect(
        &env::var("DATABASE_URL").expect("Could not find environment variable DATABASE_URL"),
    )
    .await
    .expect("Could not establish connection do database");

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
            cx.update.user.last_name.unwrap_or(String::from("")),
            cx.update.user.username.unwrap_or(String::from("")),
            cx.update.user.language_code.unwrap_or(String::from("en")),
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
    Ok(sqlx::query!(
        "SELECT EXISTS(SELECT id from polls WHERE id = $1);",
        poll_id
    )
    .fetch_one(pool)
    .await?
    .exists
    .unwrap())
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
        0 => Ok(sqlx::query!(
            r#"
            SELECT away_team FROM games WHERE id = $1;
            "#,
            game_id
        )
        .fetch_one(pool)
        .await?
        .away_team
        .unwrap()),
        1 => Ok(sqlx::query!(
            r#"
            SELECT home_team FROM games WHERE id = $1;
            "#,
            game_id
        )
        .fetch_one(pool)
        .await?
        .home_team
        .unwrap()),
        _ => panic!("Could not convert bet to team_id!"),
    }
}

async fn get_chat_id_game_id_from_poll(
    pool: &PgPool,
    poll_id: String,
) -> Result<((i64, i32)), Error> {
    dbg!(&poll_id);
    let row = sqlx::query!(
        r#"
        SELECT chat_id, game_id FROM polls WHERE id = $1;
        "#,
        poll_id
    )
    .fetch_one(pool)
    .await?;
    //.chat_id
    //.expect("Could not get chat_id, game_id from poll_id");

    Ok((row.chat_id.unwrap(), row.game_id.unwrap()))
}

async fn user_is_in_db(pool: &PgPool, user_id: i64) -> Result<bool, Error> {
    Ok(
        sqlx::query!("SELECT EXISTS(SELECT * FROM users WHERE id = $1)", user_id)
            .fetch_one(pool)
            .await?
            .exists
            .unwrap(),
    )
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

async fn show_rankings(
    cx: UpdateWithCx<Message>,
    pool: &PgPool,
    chat_id: i64,
) -> Result<(), Error> {
    let ranking_query = sqlx::query!(
        r#"
        SELECT first_name, last_name, username, points FROM rankings WHERE chat_id = $1 ORDER BY points DESC;
        "#,
        chat_id
    )
    .fetch_all(pool)
    .await?;

    let mut rank: i32 = 0;
    //let mut rankings = ranking_query
    //    .into_iter()
    //    .map(|record| format!("{}\n", record.first_name.unwrap().as_str()))
    //    .collect::<String>();
    let mut rankings = String::from("");

    for record in ranking_query {
        rankings.push_str(
            &format!(
                "{rank}. {first_name} {last_name} {username}\nPoints: {points}\n",
                rank = {
                    rank += 1;
                    rank
                },
                first_name = record.first_name.unwrap(),
                last_name = record.last_name.unwrap(),
                username = {
                    let username = record.username.unwrap();
                    match username.as_str() {
                        "" => format!(""),
                        _ => format!("@{}", username),
                    }
                },
                points = record.points.unwrap()
            )
            .as_str(),
        );
    }

    cx.answer_str(rankings).await;
    Ok(())
}
