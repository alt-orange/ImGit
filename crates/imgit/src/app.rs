//! The application layer (`src/app/application.*`, `toasts.*`): the repository tabs, the modals, the settings,
//! the profiles, the terminal sessions, the toasts and the frame that draws them around the panels of
//! [`crate::ui::panels`]. See [`application`] for the layout of its submodules.

pub mod action;
pub mod application;
pub mod background;
pub mod clone;
pub mod credentials;
pub mod dialog;
pub mod frame;
pub mod frame_impl;
pub mod git_ops;
pub mod menu_bar;
pub mod persistence;
pub mod settings;
pub mod shortcuts;
pub mod tabs;
pub mod terminal;
pub mod toasts;

pub use application::Application;
