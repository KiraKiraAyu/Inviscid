pub(crate) mod defs;
pub(crate) mod handlers;
pub(crate) mod registry;

pub use defs::*;
pub(crate) use handlers::attach_app_actions;
pub(crate) use registry::display_keystroke_for;
pub use registry::{KeybindingMeta, apply_configured_keys, bind_default_bindings, keybindings};
