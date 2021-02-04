use basketball_betting_bot::{
    utils::{
        add_bet, add_user, bet_to_team_id, get_chat_id_game_id_from_poll, poll_is_in_db_by_poll_id,
        user_is_in_db,
    },
    Error,
};
use sqlx::postgres::PgPool;
use std::{convert::Infallible, env};
use teloxide::prelude::*;

use crate::states::Dialogue;

type In = DialogueWithCx<Message, Dialogue, Infallible>;

pub async fn run() {
    // create a new bot: env variable TELOXIDE_TOKEN must be set (bot token)
    let bot = Bot::builder().build();

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

                handle_poll_answer(poll_answer, &pool)
                    .await
                    .unwrap_or_default();
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
    dbg!(&cx.update.option_ids);
    // check if it's a poll that the bot sent
    // is probably unnecessary, since per the official docs poll answers not sent by the bot itself
    // are ignored. Since this could change in the future, I'm gonna play it safe.
    if !poll_is_in_db_by_poll_id(pool, cx.update.poll_id.clone())
        .await
        .expect("could not get poll_id!")
    {
        dbg!("poll_id not found!", cx.update.poll_id);
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
            cx.update
                .user
                .language_code
                .unwrap_or_else(|| "en".to_string()),
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
