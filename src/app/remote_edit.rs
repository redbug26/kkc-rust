use super::*;
use ratatui_textarea::{CursorMove, TextArea};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteEditKind {
    Sftp,
    Smb,
    RemotePlugin {
        plugin_id: String,
        display_name: String,
        scheme: String,
        config_fields: Vec<crate::remote_plugins::RemoteRustConfigField>,
    },
}

impl RemoteEditKind {
    /// All protocol choices in menu order.
    pub fn all() -> Vec<Self> {
        let mut out = vec![Self::Sftp, Self::Smb];
        let mut remote_plugins = discover_remote_plugin_choices();
        remote_plugins.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        out.extend(remote_plugins.into_iter().map(
            |(plugin_id, display_name, scheme, config_fields)| Self::RemotePlugin {
                plugin_id,
                display_name,
                scheme,
                config_fields,
            },
        ));
        out
    }

    pub fn name(&self) -> String {
        match self {
            Self::Sftp => "SFTP".to_string(),
            Self::Smb => "SMB".to_string(),
            Self::RemotePlugin { display_name, .. } => display_name.clone(),
        }
    }

    pub fn title(&self) -> String {
        match self {
            Self::Sftp => " Add SFTP Server ".to_string(),
            Self::Smb => " Add SMB Server ".to_string(),
            Self::RemotePlugin { display_name, .. } => {
                format!(" Add {} Connection ", display_name)
            }
        }
    }

    pub fn field_labels(&self) -> Vec<String> {
        match self {
            Self::Sftp => vec!["Name", "Host", "User", "Port", "Path", "Identity"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            Self::Smb => vec!["Name", "Host", "User", "Workgroup", "Share", "Password"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            Self::RemotePlugin { config_fields, .. } => {
                let mut labels = vec!["Name".to_string()];
                labels.extend(config_fields.iter().map(|field| field.label.clone()));
                labels.push("Path".to_string());
                labels.push("Auth input".to_string());
                labels
            }
        }
    }

    pub fn validation_message(&self) -> String {
        match self {
            Self::Sftp => "SFTP name is required".to_string(),
            Self::Smb => "SMB name and host are required".to_string(),
            Self::RemotePlugin { .. } => {
                "Remote plugin name and required configuration fields are required".to_string()
            }
        }
    }

    pub fn plugin_id(&self) -> Option<&str> {
        match self {
            Self::RemotePlugin { plugin_id, .. } => Some(plugin_id.as_str()),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct RemoteEditState {
    pub kind: RemoteEditKind,
    pub fields: Vec<String>,
    pub textarea: TextArea<'static>,
    pub cursor: usize,
    pub input_cursor: usize,
    /// Original name when editing an existing profile (for rename support).
    pub edit_original_name: Option<String>,
    /// Fetched share list for SMB connections (populated on F5), with cursor.
    pub share_picker: Option<(Vec<String>, usize)>,
    /// Session returned by remote plugin auth_start, consumed by auth_complete.
    pub plugin_auth_session_json: Option<String>,
    /// Authentication shortcuts are only available when creating from F7:Add.
    pub plugin_auth_enabled: bool,
}

impl std::fmt::Debug for RemoteEditState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteEditState")
            .field("kind", &self.kind)
            .field("fields", &self.fields)
            .field("cursor", &self.cursor)
            .field("input_cursor", &self.input_cursor)
            .field("edit_original_name", &self.edit_original_name)
            .field("share_picker", &self.share_picker)
            .field("plugin_auth_session_json", &self.plugin_auth_session_json)
            .field("plugin_auth_enabled", &self.plugin_auth_enabled)
            .finish()
    }
}

impl RemoteEditState {
    pub const NAME: usize = 0;
    pub const HOST: usize = 1;
    pub const USER: usize = 2;
    pub const PORT: usize = 3;
    pub const PATH: usize = 4;
    pub const SECRET: usize = 5;

    pub fn new(kind: RemoteEditKind) -> Self {
        let fields = match &kind {
            RemoteEditKind::Sftp => vec![
                String::new(),
                String::new(),
                String::new(),
                "22".into(),
                "~".into(),
                String::new(),
            ],
            RemoteEditKind::Smb => vec![
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ],
            RemoteEditKind::RemotePlugin {
                display_name,
                config_fields,
                ..
            } => {
                let mut fields = vec![display_name.clone()];
                fields.extend(
                    config_fields
                        .iter()
                        .map(|field| field.default_value.clone()),
                );
                fields.push("/".into());
                fields.push(String::new());
                fields
            }
        };
        let input_cursor = fields[Self::NAME].len();
        let textarea = Self::make_textarea(&fields[Self::NAME]);
        Self {
            kind,
            fields,
            textarea,
            cursor: 0,
            input_cursor,
            edit_original_name: None,
            share_picker: None,
            plugin_auth_session_json: None,
            plugin_auth_enabled: true,
        }
    }

    pub fn from_profile(profile: &RemoteProfile) -> Self {
        let (kind, fields) = match &profile.kind {
            RemoteKind::Sftp(sftp) => (
                RemoteEditKind::Sftp,
                vec![
                    profile.name.clone(),
                    sftp.host.clone().unwrap_or_default(),
                    sftp.user.clone().unwrap_or_default(),
                    sftp.port.map(|p| p.to_string()).unwrap_or_default(),
                    sftp.path.clone().unwrap_or_default(),
                    sftp.identity_file.clone().unwrap_or_default(),
                ],
            ),
            RemoteKind::Smb(smb) => (
                RemoteEditKind::Smb,
                vec![
                    profile.name.clone(),
                    smb.host.clone(),
                    smb.user.clone().unwrap_or_default(),
                    smb.workgroup.clone().unwrap_or_default(),
                    smb.share.clone().unwrap_or_default(),
                    smb.password.clone().unwrap_or_default(),
                ],
            ),
            RemoteKind::RemotePlugin(plugin) => {
                let discovered = discover_remote_plugin_choices();
                let (display_name, config_fields) = discovered
                    .into_iter()
                    .find(|(id, _, _, _)| *id == plugin.plugin_id)
                    .map(|(_, name, _, fields)| (name, fields))
                    .unwrap_or_else(|| (plugin.plugin_id.clone(), Vec::new()));
                let mut fields = vec![profile.name.clone()];
                fields.extend(load_remote_plugin_config_values(
                    &plugin.config_json,
                    &config_fields,
                ));
                fields.push(plugin.path.clone().unwrap_or_else(|| "/".to_string()));
                fields.push(String::new());
                (
                    RemoteEditKind::RemotePlugin {
                        plugin_id: plugin.plugin_id.clone(),
                        display_name,
                        scheme: plugin.scheme.clone(),
                        config_fields,
                    },
                    fields,
                )
            }
        };
        Self {
            kind,
            input_cursor: fields[Self::NAME].len(),
            textarea: Self::make_textarea(&fields[Self::NAME]),
            fields,
            cursor: 0,
            edit_original_name: Some(profile.name.clone()),
            share_picker: None,
            plugin_auth_session_json: None,
            plugin_auth_enabled: false,
        }
    }

    pub fn make_textarea(initial: impl AsRef<str>) -> TextArea<'static> {
        let mut textarea = TextArea::new(vec![initial.as_ref().to_string()]);
        textarea.move_cursor(CursorMove::End);
        textarea
    }

    pub fn textarea_text(&self) -> &str {
        self.textarea
            .lines()
            .first()
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn commit_textarea(&mut self) {
        if self.cursor < self.input_count() {
            let value = self.textarea_text().to_string();
            if let Some(slot) = self.fields.get_mut(self.cursor) {
                *slot = value;
            }
            self.input_cursor = self.textarea.cursor().1;
        }
    }

    pub fn focus_field_at_end(&mut self, cursor: usize) {
        self.commit_textarea();
        self.cursor = cursor.min(self.cancel_index());
        if self.cursor < self.input_count() {
            let value = self.fields[self.cursor].clone();
            self.textarea = Self::make_textarea(value);
            self.input_cursor = self.textarea.cursor().1;
        }
    }

    pub fn focus_field_at_column(&mut self, cursor: usize, column: usize) {
        self.focus_field_at_end(cursor);
        if self.cursor < self.input_count() {
            let col = column.min(self.fields[self.cursor].chars().count());
            self.textarea
                .move_cursor(CursorMove::Jump(0, col.min(u16::MAX as usize) as u16));
            self.input_cursor = self.textarea.cursor().1;
        }
    }

    pub fn set_field_text(&mut self, cursor: usize, value: impl Into<String>) {
        if cursor >= self.input_count() {
            return;
        }
        let value = value.into();
        self.fields[cursor] = value.clone();
        if self.cursor == cursor {
            self.textarea = Self::make_textarea(value);
            self.input_cursor = self.textarea.cursor().1;
        }
    }

    pub fn sync_cursor(&mut self) {
        self.focus_field_at_end(self.cursor);
    }

    pub fn field_value_offset(&self, cursor: usize) -> u16 {
        let label = self
            .kind
            .field_labels()
            .get(cursor)
            .cloned()
            .unwrap_or_default();
        let label_width = UnicodeWidthStr::width(format!("{label}:").as_str()).max(8);
        (1 + label_width).min(u16::MAX as usize) as u16
    }

    pub fn input_count(&self) -> usize {
        self.fields.len()
    }

    pub fn save_index(&self) -> usize {
        self.input_count()
    }

    pub fn cancel_index(&self) -> usize {
        self.input_count() + 1
    }

    pub fn path_field_index(&self) -> usize {
        match &self.kind {
            RemoteEditKind::RemotePlugin { config_fields, .. } => 1 + config_fields.len(),
            _ => Self::PATH,
        }
    }

    pub fn auth_field_index(&self) -> Option<usize> {
        match &self.kind {
            RemoteEditKind::RemotePlugin { config_fields, .. } => Some(2 + config_fields.len()),
            _ => None,
        }
    }

    pub fn is_remote_plugin_config_cursor(&self) -> bool {
        match &self.kind {
            RemoteEditKind::RemotePlugin { config_fields, .. } => {
                self.cursor >= 1 && self.cursor < 1 + config_fields.len()
            }
            _ => false,
        }
    }

    pub fn set_remote_plugin_config_json(&mut self, config_json: &str) -> bool {
        let RemoteEditKind::RemotePlugin { config_fields, .. } = &self.kind else {
            return false;
        };
        let values = load_remote_plugin_config_values(config_json, config_fields);
        for (offset, value) in values.into_iter().enumerate() {
            let idx = 1 + offset;
            if let Some(slot) = self.fields.get_mut(idx) {
                *slot = value;
            }
        }
        true
    }

    pub fn build_profile(&self) -> Option<RemoteProfile> {
        let mut fields = self.fields.clone();
        if self.cursor < fields.len() {
            fields[self.cursor] = self.textarea_text().to_string();
        }

        let name = fields[Self::NAME].trim();
        if name.is_empty() {
            return None;
        }
        Some(match &self.kind {
            RemoteEditKind::Sftp => {
                let port = if fields[Self::PORT].trim().is_empty() {
                    None
                } else {
                    fields[Self::PORT].trim().parse::<u16>().ok()
                };
                RemoteProfile {
                    name: name.to_string(),
                    source: RemoteSource::UserToml,
                    kind: RemoteKind::Sftp(crate::remote::SftpProfile {
                        host: trim_opt(&fields[Self::HOST]),
                        user: trim_opt(&fields[Self::USER]),
                        port,
                        path: trim_opt(&fields[Self::PATH]),
                        identity_file: trim_opt(&fields[Self::SECRET]),
                    }),
                }
            }
            RemoteEditKind::Smb => {
                let host = fields[Self::HOST].trim();
                if host.is_empty() {
                    return None;
                }
                RemoteProfile {
                    name: name.to_string(),
                    source: RemoteSource::UserToml,
                    kind: RemoteKind::Smb(crate::remote::SmbProfile {
                        host: host.to_string(),
                        user: trim_opt(&fields[Self::USER]),
                        workgroup: trim_opt(&fields[Self::PORT]),
                        share: trim_opt(&fields[Self::PATH]),
                        password: trim_opt(&fields[Self::SECRET]),
                        path: None,
                    }),
                }
            }
            RemoteEditKind::RemotePlugin {
                plugin_id,
                scheme,
                config_fields,
                ..
            } => {
                let parsed = build_remote_plugin_config_value(&fields, config_fields);
                if validate_remote_plugin_config_json(&parsed, config_fields).is_err() {
                    return None;
                }
                let path = trim_opt(&fields[self.path_field_index()]);
                RemoteProfile {
                    name: name.to_string(),
                    source: RemoteSource::UserToml,
                    kind: RemoteKind::RemotePlugin(crate::remote::RemotePluginProfile {
                        plugin_id: plugin_id.clone(),
                        scheme: scheme.clone(),
                        config_json: parsed.to_string(),
                        path,
                    }),
                }
            }
        })
    }

    pub fn is_remote_plugin(&self) -> bool {
        matches!(&self.kind, RemoteEditKind::RemotePlugin { .. })
    }

    pub fn plugin_config_json(&self) -> Option<String> {
        let RemoteEditKind::RemotePlugin { config_fields, .. } = &self.kind else {
            return None;
        };
        let mut fields = self.fields.clone();
        if self.cursor < fields.len() {
            fields[self.cursor] = self.textarea_text().to_string();
        }
        Some(build_remote_plugin_config_value(&fields, config_fields).to_string())
    }

    pub fn plugin_auth_input(&self) -> Option<String> {
        let idx = self.auth_field_index()?;
        if self.cursor == idx {
            Some(self.textarea_text().trim().to_string())
        } else {
            self.fields.get(idx).map(|value| value.trim().to_string())
        }
    }
}

fn discover_remote_plugin_choices() -> Vec<(
    String,
    String,
    String,
    Vec<crate::remote_plugins::RemoteRustConfigField>,
)> {
    let Ok(plugins_dir) = crate::plugins::plugins_dir() else {
        crate::viewer::debug_log("remote-edit: no plugins_dir available");
        return Vec::new();
    };
    crate::viewer::debug_log(&format!(
        "remote-edit: discovering remote plugin choices from {}",
        plugins_dir.display()
    ));
    let loaded =
        crate::remote_plugins::discover_remote_rust_plugins(&plugins_dir).unwrap_or_else(|err| {
            crate::viewer::debug_log(&format!(
                "remote-edit: loaded remote plugin discovery failed: {err}"
            ));
            Vec::new()
        });
    crate::viewer::debug_log(&format!(
        "remote-edit: loaded remote plugin count={} ids=[{}]",
        loaded.len(),
        loaded
            .iter()
            .map(|plugin| plugin.id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    ));

    let manifests = crate::remote_plugins::discover_remote_rust_plugin_manifests(&plugins_dir)
        .unwrap_or_else(|err| {
            crate::viewer::debug_log(&format!(
                "remote-edit: remote manifest discovery failed: {err}"
            ));
            Vec::new()
        });
    crate::viewer::debug_log(&format!(
        "remote-edit: remote manifest count={} ids=[{}]",
        manifests.len(),
        manifests
            .iter()
            .map(|manifest| manifest.id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    ));

    let mut choices = loaded
        .into_iter()
        .map(|plugin| (plugin.id, plugin.name, plugin.scheme, plugin.config_fields))
        .collect::<Vec<_>>();

    let loaded_ids = choices
        .iter()
        .map(|(plugin_id, _, _, _)| plugin_id.clone())
        .collect::<std::collections::HashSet<_>>();
    for manifest in manifests {
        if loaded_ids.contains(&manifest.id) {
            continue;
        }
        // Keep degraded entries visible so users can diagnose plugin load issues.
        choices.push((
            manifest.id.clone(),
            format!("{} (library not loaded)", manifest.name),
            manifest.id,
            Vec::new(),
        ));
    }

    crate::viewer::debug_log(&format!(
        "remote-edit: final remote choices count={} ids=[{}]",
        choices.len(),
        choices
            .iter()
            .map(|(plugin_id, _, _, _)| plugin_id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    ));

    choices
}

fn trim_opt(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn validate_remote_plugin_config_json(
    parsed: &serde_json::Value,
    config_fields: &[crate::remote_plugins::RemoteRustConfigField],
) -> Result<(), ()> {
    if !parsed.is_object() {
        return Err(());
    }

    for field in config_fields.iter().filter(|field| field.required) {
        let Some(value) = parsed.get(&field.key) else {
            return Err(());
        };
        if matches!(value, serde_json::Value::Null) {
            return Err(());
        }
        if let serde_json::Value::String(text) = value
            && text.trim().is_empty()
        {
            return Err(());
        }
    }

    Ok(())
}

fn build_remote_plugin_config_value(
    fields: &[String],
    config_fields: &[crate::remote_plugins::RemoteRustConfigField],
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (offset, field) in config_fields.iter().enumerate() {
        let value = fields
            .get(1 + offset)
            .cloned()
            .unwrap_or_else(|| field.default_value.clone());
        map.insert(field.key.clone(), serde_json::Value::String(value));
    }
    serde_json::Value::Object(map)
}

fn load_remote_plugin_config_values(
    config_json: &str,
    config_fields: &[crate::remote_plugins::RemoteRustConfigField],
) -> Vec<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(config_json).ok();
    config_fields
        .iter()
        .map(|field| {
            parsed
                .as_ref()
                .and_then(|value| value.get(&field.key))
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| field.default_value.clone())
        })
        .collect()
}
