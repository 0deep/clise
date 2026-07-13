use clise_core::format::{Format, detect, parse};
use std::fs;
use std::io::{self, Read};

pub fn run(
    file: Option<String>,
    to: Option<String>,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read content from file or stdin
    let (content, _is_stdin, filepath) = match file.as_deref() {
        None | Some("-") => {
            if write {
                eprintln!("Error: Cannot use -w/--write with stdin");
                std::process::exit(1);
            }
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            (buffer, true, None)
        }
        Some(path) => {
            let content = fs::read_to_string(path)?;
            (content, false, Some(path))
        }
    };

    // 2. Detect input format
    let input_format = if let Some(ref path) = filepath {
        detect(path, &content)
    } else {
        detect("stdin", &content)
    };

    // 3. Determine target format
    let target_format = if let Some(t) = to {
        match t.to_lowercase().as_str() {
            "json" => Format::Json,
            "jsonc" => Format::Jsonc,
            "yaml" | "yml" => Format::Yaml,
            "toml" => Format::Toml,
            other => {
                eprintln!("Error: Unsupported format '{}'", other);
                std::process::exit(1);
            }
        }
    } else {
        input_format
    };

    // 4. Parse input content
    let parsed_val = match parse(&content, input_format) {
        Ok(val) => val,
        Err(e) => {
            eprintln!("Error parsing input as {:?}: {}", input_format, e);
            std::process::exit(1);
        }
    };

    // 5. Serialize with target format, preserving comments if possible
    let (nodes, root) = if input_format == target_format {
        match clise_core::format::parse_annotated(&content, target_format) {
            Ok(pair) => pair,
            Err(_) => clise_core::format::value_to_annotated(&parsed_val),
        }
    } else {
        clise_core::format::value_to_annotated(&parsed_val)
    };
    let formatted = match clise_core::format::serialize_annotated(&nodes, root, target_format) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error formatting output as {:?}: {}", target_format, e);
            std::process::exit(1);
        }
    };

    // 6. Write out
    if write {
        if let Some(path) = filepath {
            let output_path = if input_format != target_format {
                let ext = match target_format {
                    Format::Json => "json",
                    Format::Jsonc => "jsonc",
                    Format::Yaml => "yaml",
                    Format::Toml => "toml",
                    _ => "json",
                };
                std::path::Path::new(path).with_extension(ext)
            } else {
                std::path::PathBuf::from(path)
            };
            fs::write(output_path, formatted)?;
        }
    } else {
        print!("{}", formatted);
    }

    Ok(())
}
