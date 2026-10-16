//! Shared control contract and portable audio/control logic for Echocord.
pub mod audio;
pub mod controller;
pub mod limiter;
pub mod model;
#[cfg(windows)]
pub mod process;
pub mod protocol;
pub mod storage;

pub mod mix_bus;
pub mod mixing;

pub mod playback;

pub mod discord;
