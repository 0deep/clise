#![allow(clippy::too_many_arguments)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::manual_strip)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::double_ended_iterator_last)]
#![allow(clippy::manual_map)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_checked_ops)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::op_ref)]
#![allow(clippy::len_zero)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::single_match)]

pub mod action;
/// Comment parsing, classification, and preservation logic.
pub mod comment;
pub mod edit;
pub mod flatten;
pub mod format;
pub mod navigate;
pub mod node;
pub mod render;
pub mod schema_util;
pub mod state;
pub mod theme;
pub mod tooltip;
pub mod util;

#[cfg(test)]
mod diagnostic;

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
    pub use crate::state::{EditMode, EditorState, HitResult, NodeType, SchemaState, UiNode};
    pub use crate::theme::Theme;
}
