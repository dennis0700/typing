//! 应用状态层：全局状态机与事件分发，是 UI 回调的唯一入口。

mod app_controller;
mod curriculum_state;
mod practice_state;
mod profile_session;

pub use app_controller::*;
pub use curriculum_state::*;
pub use practice_state::*;
pub use profile_session::*;
