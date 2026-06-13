pub mod format;
pub mod validate;
pub mod schema;
pub mod init;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "clise",
    about = "TUI JSON Schema Editor and CLI Utility",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// File to edit or process (runs TUI editor if no subcommand is given)
    pub file: Option<String>,

    /// Format to use (json, jsonc, yaml, toml)
    #[arg(short, long)]
    pub format: Option<String>,

    /// Schema URL to bind directly in TUI editor mode
    #[arg(long)]
    pub schema: Option<String>,

    /// Match file against schema catalog with a custom filename/pattern
    #[arg(short = 'm', long)]
    pub catalog_match: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Format a JSON/JSONC/YAML/TOML file or stdin stream
    Format {
        /// Input file to format (omit or use '-' for stdin)
        file: Option<String>,
        /// Target format to convert/format to (json, jsonc, yaml, toml)
        #[arg(short, long)]
        to: Option<String>,
        /// Write formatted output back to the file instead of stdout
        #[arg(short, long)]
        write: bool,
    },
    /// Validate JSON/YAML/TOML against a JSON Schema
    Validate {
        /// Target file to validate
        file: String,
        /// Schema URL or local file path to validate against
        #[arg(short, long)]
        schema: Option<String>,
        /// Suppress details and only output exit code
        #[arg(short, long)]
        quiet: bool,
        /// Match file against schema catalog with a custom filename/pattern
        #[arg(short = 'm', long)]
        catalog_match: Option<String>,
    },
    /// Manage schema catalogs and mapping configuration
    Schema {
        #[command(subcommand)]
        cmd: SchemaCommands,
    },
    /// Initialize a new file with skeleton template based on schema
    Init {
        /// Target file to initialize
        file: String,
        /// Schema URL or local file path to base the template on
        #[arg(short, long)]
        schema: Option<String>,
        /// Force overwrite if file already exists
        #[arg(short, long)]
        force: bool,
        /// Match file against schema catalog with a custom filename/pattern
        #[arg(short = 'm', long)]
        catalog_match: Option<String>,
    },
    /// Generate shell completion script (hidden)
    #[command(hide = true)]
    GenerateCompletion {
        /// Shell to generate completion for (bash, zsh, fish, powershell, elvish)
        shell: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SchemaCommands {
    /// List active schema mappings
    List,
    /// Show the schema mapping URL for a specific file
    Show {
        file: String,
    },
    /// Map a file pattern to a schema URL
    Map {
        pattern: String,
        schema_url: String,
        /// Custom schema name
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Clean the schema cache directory
    Clean,
}
