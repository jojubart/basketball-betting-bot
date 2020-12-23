use derive_more;
use teloxide_macros::Transition;

use serde::{Deserialize, Serialize};

#[derive(Transition, derive_more::From, Serialize, Deserialize)]
pub enum Dialogue {
    Setup(SetupState),
    Ready(ReadyState),
}

// does it make sense to include a default?
impl Default for Dialogue {
    fn default() -> Self {
        Self::Ready(ReadyState)
    }
}

#[derive(Serialize, Deserialize)]
pub struct SetupState;

#[derive(Serialize, Deserialize)]
pub struct ReadyState;

//use teloxide_macros::Transition;
//
//use serde::{Deserialize, Serialize};
//
//#[derive(Transition, From, Serialize, Deserialize)]
//pub enum Dialogue {
//    Start(StartState),
//    HaveNumber(HaveNumberState),
//}
//
//impl Default for Dialogue {
//    fn default() -> Self {
//        Self::Start(StartState)
//    }
//}
//
//#[derive(Serialize, Deserialize)]
//pub struct StartState;
//
//#[derive(Serialize, Deserialize)]
//pub struct HaveNumberState {
//    pub number: i32,
//}
