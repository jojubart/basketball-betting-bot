use teloxide_macros::Transition;

use serde::{Deserialize, Serialize};

#[derive(Transition, derive_more::From, Serialize, Deserialize)]
pub enum Dialogue {
    Stop(StopState),
    Ready(ReadyState),
    WeekResults(WeekInputState),
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

#[derive(Serialize, Deserialize)]
pub struct WeekInputState {
    pub max_week_number: i32,
}
