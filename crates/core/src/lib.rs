pub mod state;
pub mod flatten;
pub mod navigate;
pub mod render;
pub mod edit;
pub mod theme;
pub mod format;
pub mod action;

#[cfg(feature = "schema")]
pub mod schema;
#[cfg(feature = "schema")]
pub mod config;

// Prelude
pub mod prelude {
    pub use crate::state::{EditorState, UiNode, NodeType, EditMode, SchemaState};
    pub use crate::action::Action;
    pub use crate::format::Format;
    pub use crate::theme::Theme;
    pub use crate::render::SchemaEditor;
    pub use crate::navigate;
    #[cfg(feature = "schema")]
    pub use crate::config::CliseConfig;
}
