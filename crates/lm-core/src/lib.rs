//! LumaMap kärna: datamodell, kommandon och ångra-historik. Ingen GPU, ingen video.

pub mod command;
pub mod collect;
pub mod cue;
pub mod i18n;
pub mod model;

pub use command::{Command, History};
pub use cue::{cue_start, fade_frame, FadeStep};
pub use model::*;
