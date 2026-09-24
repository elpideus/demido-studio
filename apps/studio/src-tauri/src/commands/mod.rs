//! Tauri commands: the only surface the UI can call. Each one is a thin wrapper over the
//! services in `AppState`; the logic lives in those services.

pub mod app;
pub mod chats;
pub mod market;
pub mod models;
pub mod providers;
pub mod skills;

use std::sync::Arc;

use crate::state::AppState;

pub type St<'a> = tauri::State<'a, Arc<AppState>>;
