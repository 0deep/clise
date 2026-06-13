pub mod action;
pub mod edit;
pub mod flatten;
pub mod format;
pub mod navigate;
pub mod render;
pub mod state;
pub mod theme;

#[cfg(feature = "schema")]
pub mod config;
#[cfg(feature = "schema")]
pub mod schema;

// Prelude
pub mod prelude {
    pub use crate::action::Action;
    #[cfg(feature = "schema")]
    pub use crate::config::CliseConfig;
    pub use crate::format::Format;
    pub use crate::navigate;
    pub use crate::render::SchemaEditor;
    pub use crate::state::{EditMode, EditorState, NodeType, SchemaState, UiNode};
    pub use crate::theme::Theme;
}
