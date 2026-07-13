use crate::format::Format;

/// Actions requested by the widget to the host application
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Action {
    /// No action needed
    Noop,
    /// Request schema download
    RequestSchemaFetch { filename: String },
    /// Request saving data
    Save { format: Format },
    /// Request saving data and quitting
    SaveAndQuit { format: Format },
    /// Request quitting the widget (e.g., ESC key pressed)
    Quit,
}
