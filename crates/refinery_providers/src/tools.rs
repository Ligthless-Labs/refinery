//! Canonical tool names and per-provider mappings.
//!
//! Users specify canonical names (e.g. `web_fetch`), and each provider
//! translates them to its native tool names.

/// Map a canonical tool name to Claude's native tool name.
#[must_use]
pub fn claude_tool(canonical: &str) -> Option<&'static str> {
    match canonical {
        "web_fetch" => Some("WebFetch"),
        "web_search" => Some("WebSearch"),
        "file_read" => Some("Read"),
        "file_write" => Some("Edit"),
        "shell" => Some("Bash"),
        _ => None,
    }
}

/// Map a canonical tool name to Gemini's native tool name.
#[must_use]
pub fn gemini_tool(canonical: &str) -> Option<&'static str> {
    match canonical {
        "web_fetch" | "web_search" => Some("web_search"),
        "file_read" => Some("read_file"),
        "file_write" => Some("edit_file"),
        "shell" => Some("shell"),
        _ => None,
    }
}

/// Map a canonical tool name to Codex's native tool name.
///
/// Codex uses flag-based tool control rather than named tools.
/// Returns the config key to enable the capability.
#[must_use]
pub fn codex_tool(canonical: &str) -> Option<&'static str> {
    match canonical {
        "web_fetch" | "web_search" => Some("web_search"),
        _ => None,
    }
}

/// Resolve canonical tool names to provider-native names.
///
/// Returns `(resolved, unknown)` — resolved native names and unrecognized canonical names.
pub fn resolve<F>(canonical_names: &[String], mapper: F) -> (Vec<String>, Vec<String>)
where
    F: Fn(&str) -> Option<&'static str>,
{
    let mut resolved = Vec::new();
    let mut unknown = Vec::new();

    for name in canonical_names {
        match mapper(name) {
            Some(native) => {
                let native_str = native.to_string();
                if !resolved.contains(&native_str) {
                    resolved.push(native_str);
                }
            }
            None => unknown.push(name.clone()),
        }
    }

    (resolved, unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_maps_web_fetch() {
        assert_eq!(claude_tool("web_fetch"), Some("WebFetch"));
        assert_eq!(claude_tool("shell"), Some("Bash"));
        assert_eq!(claude_tool("unknown"), None);
    }

    #[test]
    fn gemini_maps_web_fetch_and_web_search_to_same() {
        assert_eq!(gemini_tool("web_fetch"), Some("web_search"));
        assert_eq!(gemini_tool("web_search"), Some("web_search"));
    }

    #[test]
    fn resolve_deduplicates() {
        let names = vec!["web_fetch".to_string(), "web_search".to_string()];
        let (resolved, unknown) = resolve(&names, gemini_tool);
        // Both map to "web_search", should deduplicate
        assert_eq!(resolved, vec!["web_search".to_string()]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn resolve_reports_unknown() {
        let names = vec!["web_fetch".to_string(), "teleport".to_string()];
        let (resolved, unknown) = resolve(&names, claude_tool);
        assert_eq!(resolved, vec!["WebFetch".to_string()]);
        assert_eq!(unknown, vec!["teleport".to_string()]);
    }
}
