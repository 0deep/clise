use std::path::{Path, PathBuf};
use std::fs;
use std::time::{SystemTime, Duration};
use std::sync::OnceLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SchemaError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Network error: {0:?}")]
    Reqwest(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Cache error: {0}")]
    Cache(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SchemaCatalog {
    pub version: u32,
    pub schemas: Vec<SchemaEntry>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SchemaEntry {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "fileMatch")]
    pub file_match: Option<Vec<String>>,
    pub url: String,
}

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub struct SchemaFetcher {
    cache_dir: PathBuf,
    client: &'static reqwest::Client,
}

impl SchemaFetcher {
    pub fn new() -> Result<Self, SchemaError> {
        let cache_dir = directories::ProjectDirs::from("", "", "clise")
            .map(|dirs| dirs.cache_dir().to_path_buf())
            .ok_or_else(|| SchemaError::Cache("Could not determine cache directory".to_string()))?;

        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir)?;
            fs::create_dir_all(cache_dir.join("schemas"))?;
        }

        let client = CLIENT.get_or_init(|| {
            reqwest::Client::builder()
                .user_agent("clise/0.1.0")
                .build()
                .expect("Failed to build reqwest client")
        });

        Ok(Self {
            cache_dir,
            client,
        })
    }

    pub async fn fetch_catalog(&self) -> Result<SchemaCatalog, SchemaError> {
        let catalog_path = self.cache_dir.join("catalog.json");
        
        if let Some(catalog) = self.read_cache::<SchemaCatalog>(&catalog_path)? {
            return Ok(catalog);
        }

        let url = "https://www.schemastore.org/api/json/catalog.json";
        let catalog: SchemaCatalog = self.client.get(url).send().await?.json().await?;
        
        self.write_cache(&catalog_path, &catalog)?;
        Ok(catalog)
    }

    pub async fn fetch_schema(&self, url: &str) -> Result<Value, SchemaError> {
        if url.is_empty() {
            return Err(SchemaError::Cache("Empty schema URL provided".to_string()));
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(SchemaError::Cache(format!("Invalid schema URL (must be absolute): {}", url)));
        }

        // Use hash of URL as filename for caching
        let hash = format!("{:x}", md5::compute(url));
        let schema_path = self.cache_dir.join("schemas").join(format!("{}.json", hash));

        if let Some(schema) = self.read_cache::<Value>(&schema_path)? {
            return Ok(schema);
        }

        let schema: Value = self.client.get(url).send().await?.json().await?;
        self.write_cache(&schema_path, &schema)?;
        Ok(schema)
    }

    fn read_cache<T: for<'de> Deserialize<'de>>(&self, path: &Path) -> Result<Option<T>, SchemaError> {
        if !path.exists() {
            return Ok(None);
        }

        let metadata = fs::metadata(path)?;
        let modified = metadata.modified()?;
        let elapsed = SystemTime::now().duration_since(modified).unwrap_or(Duration::from_secs(0));

        if elapsed > Duration::from_secs(30 * 24 * 60 * 60) {
            return Ok(None);
        }

        let content = fs::read_to_string(path)?;
        Ok(Some(serde_json::from_str(&content)?))
    }

    fn write_cache<T: Serialize>(&self, path: &Path, data: &T) -> Result<(), SchemaError> {
        let content = serde_json::to_string(data)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn find_schema_url(catalog: &SchemaCatalog, path_str: &str) -> Option<SchemaEntry> {
        let path = Path::new(path_str);
        let filename = path.file_name()?.to_str()?;

        // First pass: look for exact matches on filename
        for entry in &catalog.schemas {
            if let Some(matches) = &entry.file_match {
                for m in matches {
                    if m == filename {
                        return Some(entry.clone());
                    }
                }
            }
        }

        // Second pass: look for wildcard matches on the provided path
        for entry in &catalog.schemas {
            if let Some(matches) = &entry.file_match {
                for m in matches {
                    if match_glob(m, path_str) {
                        return Some(entry.clone());
                    }
                }
            }
        }

        // Third pass (Heuristic fallback): Try suffix matching for '-' or '_'
        if let Some(last_sep_idx) = filename.rfind(|c| c == '-' || c == '_') {
            let suffix_variant = &filename[last_sep_idx + 1..];
            if !suffix_variant.is_empty() && suffix_variant != filename {
                for entry in &catalog.schemas {
                    if let Some(matches) = &entry.file_match {
                        for m in matches {
                            if match_glob(m, suffix_variant) {
                                return Some(entry.clone());
                            }
                        }
                    }
                }
            }
        }

        // Fourth pass (Heuristic fallback): Try prefix matching for multiple dots (e.g. tsconfig.dev.json -> tsconfig.json)
        let parts: Vec<&str> = filename.split('.').collect();
        if parts.len() > 2 {
            let extension = parts.last().unwrap();
            let base_prefix = parts[0];
            let prefix_variant = format!("{}.{}", base_prefix, extension);
            for entry in &catalog.schemas {
                if let Some(matches) = &entry.file_match {
                    for m in matches {
                        if match_glob(m, &prefix_variant) {
                            return Some(entry.clone());
                        }
                    }
                }
            }
        }

        None
    }

    /// Scans the given directory and finds all relevant schema entries from the catalog.
    pub fn find_relevant_schemas(catalog: &SchemaCatalog, dir_path: &Path) -> Vec<SchemaEntry> {
        let mut relevant = Vec::new();
        let entries = match fs::read_dir(dir_path) {
            Ok(e) => e,
            Err(_) => return relevant,
        };

        let mut files = Vec::new();
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        files.push(name.to_string());
                    }
                }
            }
        }

        for schema_entry in &catalog.schemas {
            if let Some(matches) = &schema_entry.file_match {
                'match_loop: for m in matches {
                    for f in &files {
                        // Use filename only for relevant scan as we don't have relative paths easily
                        if match_glob(m, f) {
                            relevant.push(schema_entry.clone());
                            break 'match_loop;
                        }
                    }
                }
            }
        }
        relevant
    }
}

pub fn match_glob(pattern: &str, target: &str) -> bool {
    // Normalize path separators
    let pattern = pattern.replace('\\', "/");
    let target = target.replace('\\', "/");

    if pattern == target {
        return true;
    }

    // If pattern has path segments, target must also match the path structure.
    if pattern.contains('/') {
        if pattern.starts_with("**/") {
            let suffix = &pattern[3..];
            if !suffix.contains('/') {
                // Pattern is like "**/filename.json" or "**/tsconfig*.json"
                let target_filename = target.split('/').last().unwrap_or(&target);
                return match_filename_glob(suffix, target_filename);
            } else {
                // Pattern is like "**/cassettes/*.json"
                let target_parts: Vec<&str> = target.split('/').collect();
                let suffix_parts: Vec<&str> = suffix.split('/').collect();
                
                if target_parts.len() < suffix_parts.len() {
                    return false;
                }
                
                let target_tail = &target_parts[target_parts.len() - suffix_parts.len()..];
                for (p, t) in suffix_parts.iter().zip(target_tail.iter()) {
                    if !match_filename_glob(p, t) {
                        return false;
                    }
                }
                return true;
            }
        }
        
        // Exact path match (could be improved to handle intermediate wildcards)
        let pattern_parts: Vec<&str> = pattern.split('/').collect();
        let target_parts: Vec<&str> = target.split('/').collect();
        
        if pattern_parts.len() != target_parts.len() {
            return false;
        }
        
        for (p, t) in pattern_parts.iter().zip(target_parts.iter()) {
            if !match_filename_glob(p, t) {
                return false;
            }
        }
        return true;
    }

    // No path segments in pattern: match against filename only
    let target_filename = target.split('/').last().unwrap_or(&target);
    match_filename_glob(&pattern, target_filename)
}

fn match_filename_glob(pattern: &str, target: &str) -> bool {
    if pattern == target || pattern == "*" || pattern == "**" {
        return true;
    }
    
    if !pattern.contains('*') {
        return pattern == target;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    
    // Check start and end
    if !pattern.starts_with('*') && !target.starts_with(parts[0]) {
        return false;
    }
    if !pattern.ends_with('*') && !target.ends_with(parts.last().unwrap_or(&"")) {
        return false;
    }

    let mut last_pos = 0;
    for part in parts {
        if part.is_empty() {
            continue;
        }
        if let Some(pos) = target[last_pos..].find(part) {
            last_pos += pos + part.len();
        } else {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_glob() {
        assert!(match_glob("package.json", "package.json"));
        assert!(match_glob("*.json", "test.json"));
        assert!(match_glob("docker-compose.yml", "docker-compose.yml"));
        assert!(match_glob("*.docker-compose.yml", "my.docker-compose.yml"));
        assert!(match_glob(".*rc", ".eslintrc"));
        assert!(match_glob("config.*.json", "config.dev.json"));
        
        // Path handling
        assert!(match_glob("**/skill.json", "skill.json"));
        assert!(match_glob("**/skill.json", "dir/skill.json"));
        assert!(match_glob("**/cassettes/*.json", "cassettes/test.json"));
        assert!(match_glob("**/cassettes/*.json", "project/cassettes/test.json"));
        
        // Negative matches
        assert!(!match_glob("package.json", "package.js"));
        assert!(!match_glob("*.json", "test.yaml"));
        assert!(!match_glob("**/cassettes/*.json", "tsconfig.json"));
        assert!(!match_glob("**/cassettes/*.json", "dir/tsconfig.json"));
        
        // tsconfig case
        assert!(match_glob("tsconfig*.json", "tsconfig.json"));
        assert!(match_glob("tsconfig*.json", "tsconfig.release.json"));
    }

    #[test]
    fn test_find_schema_url_with_path() {
        let catalog = SchemaCatalog {
            version: 1,
            schemas: vec![
                SchemaEntry {
                    name: "package.json".to_string(),
                    description: None,
                    file_match: Some(vec!["**/package.json".to_string()]),
                    url: "https://example.com/package.json".to_string(),
                },
                SchemaEntry {
                    name: "EasyVCR".to_string(),
                    description: None,
                    file_match: Some(vec!["**/cassettes/*.json".to_string()]),
                    url: "https://example.com/easyvcr.json".to_string(),
                },
                SchemaEntry {
                    name: "tsconfig.json".to_string(),
                    description: None,
                    file_match: Some(vec!["**/tsconfig*.json".to_string()]),
                    url: "https://example.com/tsconfig.json".to_string(),
                },
                SchemaEntry {
                    name: "compose.yml".to_string(),
                    description: None,
                    file_match: Some(vec!["**/compose.yml".to_string(), "**/docker-compose.yml".to_string()]),
                    url: "https://example.com/compose.json".to_string(),
                }
            ],
        };

        // tsconfig.json should match tsconfig schema, not EasyVCR
        let matched = SchemaFetcher::find_schema_url(&catalog, "tsconfig.json").unwrap();
        assert_eq!(matched.name, "tsconfig.json");

        // cassettes/test.json should match EasyVCR
        let matched = SchemaFetcher::find_schema_url(&catalog, "cassettes/test.json").unwrap();
        assert_eq!(matched.name, "EasyVCR");

        // Heuristic: my-compose.yml should match compose.yml
        let matched_compose = SchemaFetcher::find_schema_url(&catalog, "my-compose.yml").unwrap();
        assert_eq!(matched_compose.name, "compose.yml");

        // Heuristic: tsconfig.dev.json should match tsconfig.json
        let matched_tsconfig = SchemaFetcher::find_schema_url(&catalog, "tsconfig.dev.json").unwrap();
        assert_eq!(matched_tsconfig.name, "tsconfig.json");
    }
}

