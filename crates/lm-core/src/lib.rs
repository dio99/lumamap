//! LumaMap kärna: datamodell, kommandon och ångra-historik. Ingen GPU, ingen video.

pub mod command;
pub mod model;

pub use command::{Command, History};
pub use model::*;
