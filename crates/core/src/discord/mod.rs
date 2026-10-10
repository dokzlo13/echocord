//! Shared gateway ownership and independently disposable Discord room drivers.
pub mod correlation;
pub mod protocol;

#[cfg(windows)]
pub mod gateway;
#[cfg(windows)]
pub mod voice;
#[cfg(any(windows, test))]
pub mod worker;

pub mod control;

#[cfg(windows)]
mod permissions;

#[cfg(any(windows, test))]
mod work;
