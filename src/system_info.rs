use crate::app::App;
use serde::Deserialize;
use std::path::PathBuf;

const CREDITS_JSON: &str = include_str!("../credits.json");

pub fn render_system_info(app: &App) -> String {
    let mut out = String::new();

    push_header(&mut out, "KKC");
    push_kv(&mut out, "Name", env!("CARGO_PKG_NAME"));
    push_kv(&mut out, "Version", env!("CARGO_PKG_VERSION"));
    push_kv(&mut out, "Description", env!("CARGO_PKG_DESCRIPTION"));
    push_kv(&mut out, "Authors", env!("CARGO_PKG_AUTHORS"));
    push_kv(&mut out, "Target", std::env::consts::OS);
    push_kv(&mut out, "Arch", std::env::consts::ARCH);
    push_kv(
        &mut out,
        "Executable",
        std::env::current_exe()
            .map(display_path)
            .unwrap_or_else(|err| format!("<unavailable: {err}>")),
    );

    push_header(&mut out, "Paths");
    push_result_path(&mut out, "Config", crate::config::config_path());
    push_result_path(&mut out, "State", crate::config::state_path());
    push_result_path(&mut out, "Data dir", crate::config::data_dir());
    push_result_path(&mut out, "Plugins dir", crate::plugins::plugins_dir());
    push_result_path(
        &mut out,
        "Terminal cache",
        crate::config::terminal_cache_path(),
    );
    push_kv(
        &mut out,
        "Debug log",
        crate::viewer::debug_log_path()
            .map(display_path)
            .unwrap_or_else(|| "<not initialized>".into()),
    );
    push_kv(
        &mut out,
        "Store index",
        crate::plugins::store_index_path().display().to_string(),
    );
    push_kv(
        &mut out,
        "Current dir",
        std::env::current_dir()
            .map(display_path)
            .unwrap_or_else(|err| format!("<unavailable: {err}>")),
    );
    push_kv(&mut out, "Active panel", app.active_panel().display_path());
    push_kv(&mut out, "Other panel", app.other_panel().display_path());

    push_header(&mut out, "Runtime");
    push_kv(
        &mut out,
        "Panel view",
        format!("{:?}", app.config.panel_view_type),
    );
    push_kv(
        &mut out,
        "Left tabs",
        app.left_panel_tab_count().to_string(),
    );
    push_kv(
        &mut out,
        "Right tabs",
        app.right_panel_tab_count().to_string(),
    );
    push_kv(
        &mut out,
        "Quick preview",
        if app.quick_preview.is_some() {
            "on"
        } else {
            "off"
        },
    );
    push_kv(
        &mut out,
        "Debug logging",
        if app.config.debug_log { "on" } else { "off" },
    );

    let crates = credits_crates(CREDITS_JSON);
    push_header(&mut out, "Crates");
    push_kv(&mut out, "Credited crates", crates.len().to_string());
    for krate in crates {
        out.push_str("  ");
        out.push_str(&krate.name);
        out.push(' ');
        out.push_str(&krate.version);
        if !krate.authors.is_empty() {
            out.push_str("  by ");
            out.push_str(&krate.authors.join(", "));
        }
        out.push('\n');
        if !krate.description.is_empty() {
            out.push_str("    ");
            out.push_str(&single_line(&krate.description));
            out.push('\n');
        }
        if let Some(url) = krate.repository.as_deref().or(krate.homepage.as_deref()) {
            out.push_str("    ");
            out.push_str(url);
            out.push('\n');
        }
    }

    out
}

fn push_header(out: &mut String, title: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(title);
    out.push('\n');
    out.push_str(&"-".repeat(title.len()));
    out.push('\n');
}

fn push_kv(out: &mut String, key: &str, value: impl AsRef<str>) {
    out.push_str(&format!("{key:<16} {}\n", value.as_ref()));
}

fn push_result_path(out: &mut String, key: &str, path: anyhow::Result<PathBuf>) {
    match path {
        Ok(path) => push_kv(out, key, display_path(path)),
        Err(err) => push_kv(out, key, format!("<unavailable: {err}>")),
    }
}

fn display_path(path: PathBuf) -> String {
    path.display().to_string()
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct CrateInfo {
    name: String,
    version: String,
    authors: Vec<String>,
    description: String,
    repository: Option<String>,
    homepage: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreditsFile {
    crates: Vec<CrateInfo>,
}

fn credits_crates(json: &str) -> Vec<CrateInfo> {
    let Ok(mut packages) = serde_json::from_str::<CreditsFile>(json).map(|credits| credits.crates)
    else {
        return Vec::new();
    };
    packages.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));
    packages
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_crates_extracts_package_metadata() {
        let json = r#"
{
  "crates": [
    {
      "name": "local",
      "version": "0.1.0",
      "authors": [],
      "description": "Local crate",
      "repository": null,
      "homepage": null
    },
    {
      "name": "a",
      "version": "1.2.3",
      "authors": ["Ada"],
      "description": "A test crate",
      "repository": "https://example.test/a",
      "homepage": null
    }
  ]
}
"#;

        let packages = credits_crates(json);
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "a");
        assert_eq!(packages[0].version, "1.2.3");
        assert_eq!(packages[0].authors, vec!["Ada"]);
        assert_eq!(
            packages[0].repository.as_deref(),
            Some("https://example.test/a")
        );
        assert_eq!(packages[1].name, "local");
        assert!(packages[1].authors.is_empty());
    }
}
