use crate::states::*;
use crate::*;
use basketball_betting_bot::{
    get_active_chat_status,
    utils::{
        change_active_chat_status, chat_is_known, get_bet_week, remove_chat, send_polls,
        show_complete_rankings, show_week_rankings, user_is_admin,
    },
};
use sqlx::postgres::PgPool;
use std::env;
use teloxide_macros::teloxide;

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
        .await
        .unwrap_or_default();
    }

    let ans = ans.as_str();

    // if the chat was not yet marked as active and they send a message other than start
    // we'll send them to the SetupState where they can
    if !get_active_chat_status(&pool, chat_id)
        .await
        .unwrap_or(false)
        && (ans != "/start" && ans != "/start@BasketballBettingBot")
    {
        cx.answer_str("Send /start to begin your season!").await?;
        return next(ReadyState);
    }

    match ans {
        "/start" | "/start@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            if get_active_chat_status(&pool, chat_id)
                .await
                .unwrap_or(false)
            {
                cx.answer_str("Looks like you've started your season already!")
                    .await?;
                return next(ReadyState);
            }
            change_active_chat_status(&pool, chat_id, true)
                .await
                .unwrap_or_default();
            cx.answer_str(r#"BasketballBettingBot sends you 11 NBA games to bet on each week, 10 good ones and one battle between the supreme tank commanders. The one who gets the most games right in a week gets one point.
You play against the other members of your group and the winner is the one who wins the most weeks."#).await?;
            cx.answer_str("Your season begins now!").await?;
            send_polls(&pool, chat_id, &cx.bot)
                .await
                .unwrap_or_default();
            dbg!("SEASONS STARTS");
            return next(ReadyState);
        }

        "/standings" | "/standings@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            show_week_rankings(&cx, &pool, chat_id, -1)
                .await
                .unwrap_or_default();
        }
        "/full_standings" | "/full_standings@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            show_complete_rankings(&cx, &pool, chat_id)
                .await
                .unwrap_or_default();
        }
        "/stop_season" | "/stop_season@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            if user_is_admin(chat_id, &cx).await.unwrap_or(false) {
                cx.answer_str(
                    "Send /end_my_season to end the season.\n
Afterwards you will get the standings of this week and the complete results table.\n
YOU CAN'T UNDO THIS ACTION AND ALL YOUR BETS AND RESULTS ARE LOST!",
                )
                .await?;
                return next(StopState);
            } else {
                cx.answer_str("Only the group admins can stop the season!")
                    .await?;
            }
        }
        "/week_standings" | "/week_standings@BasketballBettingBot" => {
            let chat_id = cx.update.chat_id();
            let bet_week = get_bet_week(&pool, chat_id).await;

            match bet_week {
                Err(e) => {
                    dbg!(e);
                    cx.answer_str("Sorry, could not send standings right now!")
                        .await?;
                }
                Ok(bet_week) => {
                    let max_week = bet_week.week_number;
                    match max_week {
                        1 => {
                            cx.answer_str("You haven't played more than one week yet!\nHere are the standings for that week:")
                                .await?;
                            show_week_rankings(&cx, &pool, chat_id, 1)
                                .await
                                .unwrap_or_default();
                        }
                        _ => {
                            let mut week_options = String::from("");
                            for week in 1..=max_week {
                                week_options.push_str(
                                    format!("/{week_number} ", week_number = week).as_str(),
                                )
                            }
                            cx.answer_str(format!(
                                "Click on the week that you want to show the results for!\n{week_options}",
                                week_options=week_options
                            ))
                            .await?;
                            return next(WeekInputState {
                                max_week_number: max_week,
                            });
                        }
                    }
                }
            }
        }
        "/sage" | "/sage@BasketballBettingBot" => {
            let photo = teloxide::types::InputFile::Url(
                "https://media.giphy.com/media/zLVTQRSiCm2a8kljMq/giphy.gif".to_string(),
            );

            match cx.answer_animation(photo).send().await {
                Ok(_) => (),
                Err(e) => {
                    dbg!(e);
                    cx.answer_str("Sorry, could not send the GIF, try again later!")
                        .await?;
                }
            }
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
        cx.answer_str("Only the group admins can stop the chat!")
            .await?;
        return next(ReadyState);
    }
    match ans.as_str() {
        "/end_my_season" => {
            show_week_rankings(&cx, &pool, chat_id, -1)
                .await
                .unwrap_or_default();
            show_complete_rankings(&cx, &pool, chat_id)
                .await
                .unwrap_or_default();
            remove_chat(&pool, chat_id).await.unwrap_or_default();
            cx.answer_str("SEASON ENDED").await?;
        }
        _ => {
            cx.answer_str("The season continues!").await?;
        }
    }
    next(ReadyState)
}

#[teloxide(subtransition)]
async fn send_week_results(
    state: WeekInputState,
    cx: TransitionIn,
    ans: String,
) -> TransitionOut<Dialogue> {
    dbg!("WeekInputState");
    let pool = PgPool::connect(
        &env::var("DATABASE_URL").expect("Could not find DATABASE_URL environment variable!"),
    )
    .await;

    if let Err(e) = pool {
        dbg!(e);
        return next(ReadyState);
    }
    let pool = pool.expect("Could not establish DB connection!");

    let week_number = ans.strip_prefix("/").unwrap_or("-1").parse::<i32>();
    let max_week = state.max_week_number;
    match week_number {
        Ok(week_number) => {
            if week_number <= max_week && week_number >= 1 {
                show_week_rankings(&cx, &pool, cx.update.chat_id(), week_number)
                    .await
                    .unwrap_or_default();
            } else {
                cx.answer_str("You haven't played that week number yet!")
                    .await?;
            }
        }
        _ => {
            cx.answer_str("Please enter a valid number").await?;
        }
    }
    next(ReadyState)
}
