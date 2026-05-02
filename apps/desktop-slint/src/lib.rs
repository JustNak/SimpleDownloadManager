#![deny(unsafe_code)]

slint::include_modules!();

pub mod controller;
pub mod ipc;
pub mod runtime;
pub mod shell;
pub mod smoke;
pub mod update;
