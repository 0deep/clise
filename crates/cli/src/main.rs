#![allow(clippy::needless_borrow)]
#![allow(clippy::double_ended_iterator_last)]
#![allow(clippy::bool_assert_comparison)]

mod commands;

use clise_core::prelude::*;
use clise_core::schema::SchemaFetcher;
use crossterm::{
    event::{Event, EventStream},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend, widgets::Block};
use std::io::{self, IsTerminal, Read};
use std::sync::Arc;
use tokio::sync::Mutex;

use clap::Parser;
use commands::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        match command {
            Commands::Format { file, to, write } => {
                commands::format::run(file, to, write)?;
            }
            Commands::Validate {
                file,
                schema,
                quiet,
                catalog_match,
            } => {
                commands::validate::run(file, schema, quiet, catalog_match).await?;
            }
            Commands::Schema { cmd } => {
                commands::schema::run(cmd).await?;
            }
            Commands::Init {
                file,
                schema,
                force,
                catalog_match,
            } => {
                commands::init::run(file, schema, force, cli.format, catalog_match).await?;
            }
            Commands::GenerateCompletion { shell } => {
                use clap::CommandFactory;
                use clap_complete::{Shell, generate};
                use std::str::FromStr;

                let mut cmd = Cli::command();
                let shell_enum = match Shell::from_str(&shell.to_lowercase()) {
                    Ok(s) => s,
                    Err(_) => {
                        eprintln!(
                            "Error: Invalid shell '{}'. Supported: bash, zsh, fish, powershell, elvish",
                            shell
                        );
                        std::process::exit(1);
                    }
                };

                if shell_enum == Shell::Bash {
                    let mut buf = Vec::new();
                    generate(shell_enum, &mut cmd, "clise", &mut buf);
                    let mut completion = String::from_utf8(buf).unwrap_or_default();

                    let old_clise_block = r#"        clise)
            opts="-f -m -h -V --format --schema --catalog-match --help --version [FILE] format validate schema init generate-completion"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi"#;

                    let new_clise_block = r#"        clise)
            opts="-f -m -h -V --format --schema --catalog-match --help --version format validate schema init"
            if [[ ${cur} == -* ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            if [[ ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=()
                local ext
                for ext in json jsonc toml yaml yml conf; do
                    while IFS= read -r line; do
                        if [[ -n "$line" ]]; then
                            COMPREPLY+=("$line")
                        fi
                    done < <(compgen -f -X "!*.$ext" -- "${cur}")
                done
                return 0
            fi"#;

                    completion = completion.replace(old_clise_block, new_clise_block);
                    completion = completion.replace("-o default clise", "-o plusdirs clise");
                    completion = completion.replace("            COMPREPLY=( $(compgen -W \"${opts}\" -- \"${cur}\") )\n            return 0\n            ;;", "            ;;\n");

                    use std::io::Write;
                    let _ = std::io::stdout().write_all(completion.as_bytes());
                } else {
                    generate(shell_enum, &mut cmd, "clise", &mut std::io::stdout());
                }
            }
        }
    } else {
        // Default behavior: edit file in TUI
        run_tui(cli.file, cli.format, cli.schema, cli.catalog_match).await?;
    }

    Ok(())
}

async fn run_tui(
    file_path: Option<String>,
    format_override: Option<String>,
    schema_override: Option<String>,
    catalog_match: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Set panic hook to restore terminal
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, crossterm::cursor::Show);
        default_hook(panic_info);
    }));

    let explicit_format = format_override.map(|s| match s.to_lowercase().as_str() {
        "json" => Format::Json,
        "jsonc" => Format::Jsonc,
        "toml" => Format::Toml,
        "yaml" | "yml" => Format::Yaml,
        other => {
            eprintln!(
                "Error: Unsupported format '{}'. Supported: json, jsonc, toml, yaml",
                other
            );
            std::process::exit(1);
        }
    });

    let (json_data, filename, format, original_content) = if let Some(path) = file_path {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error reading file '{}': {}", path, e);
                std::process::exit(1);
            }
        };
        let format = if let Some(fmt) = explicit_format {
            fmt
        } else {
            clise_core::format::detect(&path, &content)
        };
        let data = match clise_core::format::parse(&content, format) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Invalid file format in '{}': {}", path, e);
                std::process::exit(1);
            }
        };
        (data, Some(path), format, Some(content))
    } else if !std::io::stdin().is_terminal() {
        // Read from stdin
        let mut content = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut content) {
            eprintln!("Error reading from stdin: {}", e);
            std::process::exit(1);
        }
        let format = if let Some(fmt) = explicit_format {
            fmt
        } else {
            clise_core::format::detect("", &content)
        };
        let data = match clise_core::format::parse(&content, format) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Invalid input format from stdin: {}", e);
                std::process::exit(1);
            }
        };
        (data, None, format, Some(content))
    } else {
        eprintln!("Error: No file specified and no stdin provided.");
        std::process::exit(1);
    };

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run TUI app
    let result = run_app(
        &mut terminal,
        json_data,
        filename,
        format,
        original_content,
        schema_override,
        catalog_match,
    )
    .await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen,)?;
    terminal.show_cursor()?;

    if let Ok(Some(output_content)) = &result {
        use std::io::Write;
        let _ = std::io::stdout().write_all(output_content.as_bytes());
        let _ = std::io::stdout().write_all(b"\n");
        let _ = std::io::stdout().flush();
    }

    result.map(|_| ())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    json_data: serde_json::Value,
    filename: Option<String>,
    format: Format,
    original_content: Option<String>,
    schema_override: Option<String>,
    catalog_match: Option<String>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let terminal_size = terminal.size()?;
    let mut editor_state = EditorState::new(json_data, format, filename.clone(), original_content);
    // In the demo, adjust expansion based on height excluding borders and status bar (roughly height - 3)
    editor_state.auto_adjust_expansion(terminal_size.height.saturating_sub(3) as usize);

    let state = Arc::new(Mutex::new(editor_state));
    let theme = Theme::default();
    let config = Arc::new(Mutex::new(CliseConfig::load_or_init()));

    // Schema fetcher task for the current file - Start ASAP
    if let Some(fname) = filename.clone() {
        let state_clone = state.clone();
        let config_clone = config.clone();
        let cat_match = catalog_match.clone();
        tokio::spawn(async move {
            fetch_schema_with_logic(state_clone, config_clone, fname, schema_override, cat_match)
                .await;
        });
    }

    let mut events = EventStream::new();
    let mut tick_interval = tokio::time::interval(std::time::Duration::from_millis(250));
    let mut has_saved = false;

    loop {
        {
            let mut s = state.lock().await;
            terminal.draw(|f| {
                let area = f.area();
                let title = format!(
                    " {} {} ",
                    s.filename.as_deref().unwrap_or("Untitled"),
                    if let Some(name) = &s.loaded_schema_name {
                        format!("({})", name)
                    } else if s.schema_state == SchemaState::Loading {
                        "(Loading...)".to_string()
                    } else {
                        "".to_string()
                    }
                );
                let widget = SchemaEditor::new(&theme).block(Block::bordered().title(title));
                f.render_stateful_widget(widget, area, &mut *s);
            })?;
        }

        tokio::select! {
            _ = tick_interval.tick() => {
                // Just trigger a redraw
            }
            Some(Ok(event)) = events.next() => {
                if let Event::Key(key) = event {
                    let action = {
                        let mut s = state.lock().await;
                        s.handle_key_event(key)
                    };

                    match action {
                        Action::Quit => break,
                        Action::Save { format } => {
                            let mut s = state.lock().await;
                            let filename = s.filename.clone();
                            if let Some(path) = filename {
                                match clise_core::format::serialize_annotated(&s.nodes, s.root, format) {
                                    Ok(_serialized) => {
                                        match std::fs::write(&path, &_serialized) {
                                            Ok(_) => {
                                                s.set_status(format!("Saved to {}", path));
                                                s.on_save();
                                            }
                                            Err(e) => s.set_status(format!("Save error: {}", e)),
                                        }
                                    }
                                    Err(e) => s.set_status(format!("Serialization error: {}", e)),
                                }
                            } else {
                                match clise_core::format::serialize_annotated(&s.nodes, s.root, format) {
                                    Ok(_serialized) => {
                                        s.set_status("Saved (will output on exit)".to_string());
                                        s.on_save();
                                        has_saved = true;
                                    }
                                    Err(e) => s.set_status(format!("Serialization error: {}", e)),
                                }
                            }
                        }
                        Action::SaveAndQuit { format } => {
                            let mut s = state.lock().await;
                            let filename = s.filename.clone();
                            if let Some(path) = filename {
                                match clise_core::format::serialize_annotated(&s.nodes, s.root, format) {
                                    Ok(_serialized) => {
                                        match std::fs::write(&path, &_serialized) {
                                            Ok(_) => {
                                                break;
                                            }
                                            Err(e) => s.set_status(format!("Save error: {}", e)),
                                        }
                                    }
                                    Err(e) => s.set_status(format!("Serialization error: {}", e)),
                                }
                            } else {
                                match clise_core::format::serialize_annotated(&s.nodes, s.root, format) {
                                    Ok(_serialized) => {
                                        s.on_save();
                                        has_saved = true;
                                        break;
                                    }
                                    Err(e) => s.set_status(format!("Serialization error: {}", e)),
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let s = state.lock().await;
    let output_text = if filename.is_none() && has_saved {
        match clise_core::format::serialize_annotated(&s.nodes, s.root, format) {
            Ok(txt) => Some(txt),
            Err(_) => Some(s.active_value().to_string()),
        }
    } else {
        None
    };

    Ok(output_text)
}

async fn fetch_schema_with_logic(
    state: Arc<Mutex<EditorState>>,
    config: Arc<Mutex<CliseConfig>>,
    fname: String,
    schema_override: Option<String>,
    catalog_match: Option<String>,
) {
    let fetcher = match SchemaFetcher::new() {
        Ok(f) => f,
        Err(e) => {
            let mut s = state.lock().await;
            s.schema_state = SchemaState::Error(e.to_string());
            return;
        }
    };

    {
        let mut s = state.lock().await;
        s.schema_state = SchemaState::Loading;
    }

    // 0. Check schema override first
    if let Some(url) = schema_override {
        match fetcher.fetch_schema(&url).await {
            Ok(schema) => {
                let mut s = state.lock().await;
                s.schema = Some(schema);
                s.loaded_schema_name = Some("Command Line Override".to_string());
                s.schema_state = SchemaState::Loaded;
            }
            Err(e) => {
                let mut s = state.lock().await;
                s.schema_state =
                    SchemaState::Error(format!("Override schema download error: {}", e));
            }
        }
        return;
    }

    // 1. Check config (prioritize catalog_match if provided)
    let match_target = catalog_match.as_deref().unwrap_or(&fname);
    let config_mapping = {
        let c = config.lock().await;
        c.get_mapping(match_target)
    };

    if let Some(mapping) = config_mapping {
        match fetcher.fetch_schema(&mapping.url).await {
            Ok(schema) => {
                let mut s = state.lock().await;
                s.schema = Some(schema);
                s.loaded_schema_name = Some(mapping.name.clone());
                s.schema_state = SchemaState::Loaded;
            }
            Err(e) => {
                let mut s = state.lock().await;
                s.schema_state = SchemaState::Error(format!("Custom schema download error: {}", e));
            }
        }
        return;
    }

    // 2. Fallback to catalog matching
    match fetcher.fetch_catalog().await {
        Ok(catalog) => {
            let fetcher_arc = Arc::new(fetcher);

            // Try to find schema for the current file in the FULL catalog
            if let Some(entry) = SchemaFetcher::find_schema_url(&catalog, match_target) {
                let url = entry.url.clone();
                let name = entry.name.clone();
                match fetcher_arc.fetch_schema(&url).await {
                    Ok(schema) => {
                        let mut s = state.lock().await;
                        s.schema = Some(schema);
                        s.loaded_schema_name = Some(name.clone());
                        s.schema_state = SchemaState::Loaded;
                    }
                    Err(e) => {
                        let mut s = state.lock().await;
                        s.schema_state =
                            SchemaState::Error(format!("Schema download error: {}", e));
                    }
                }
            } else {
                let mut s = state.lock().await;
                s.schema_state = SchemaState::None;
            }

            // Optional: Background pre-fetch other relevant schemas in the same directory
            let project_root = std::path::Path::new(&fname)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let relevant = SchemaFetcher::find_relevant_schemas(&catalog, project_root);
            for entry in relevant {
                let f = Arc::clone(&fetcher_arc);
                let url = entry.url.clone();
                tokio::spawn(async move {
                    let _ = f.fetch_schema(&url).await;
                });
            }
        }
        Err(e) => {
            let mut s = state.lock().await;
            s.schema_state = SchemaState::Error(e.to_string());
        }
    }
}
