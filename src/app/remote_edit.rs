use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteEditKind {
    Sftp,
    Smb,
    RemotePlugin {
        plugin_id: String,
        display_name: String,
        scheme: String,
    },
}

impl RemoteEditKind {
    /// All protocol choices in menu order.
    pub fn all() -> Vec<Self> {
        let mut out = vec![Self::Sftp, Self::Smb];
        let mut remote_plugins = discover_remote_plugin_choices();
        remote_plugins.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        out.extend(
            remote_plugins
                .into_iter()
                .map(|(plugin_id, display_name, scheme)| Self::RemotePlugin {
                    plugin_id,
                    display_name,
                    scheme,
                }),
        );
        out
    }

    pub fn name(&self) -> String {
        match self {
            Self::Sftp => "SFTP".to_string(),
            Self::Smb => "SMB".to_string(),
            Self::RemotePlugin { display_name, .. } => display_name.clone(),
        }
    }

    /// UI accent colour (R, G, B).
    pub fn color_rgb(&self) -> (u8, u8, u8) {
        match self {
            Self::Sftp => (121, 214, 255),
            Self::Smb => (255, 165, 80),
            Self::RemotePlugin { .. } => (141, 222, 150),
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

    pub fn field_labels(&self) -> [&'static str; 6] {
        match self {
            Self::Sftp => ["Name", "Host", "User", "Port", "Path", "Identity"],
            Self::Smb => ["Name", "Host", "User", "Workgroup", "Share", "Password"],
            Self::RemotePlugin { .. } => [
                "Name",
                "Config JSON",
                "Path",
                "Auth input",
                "",
                "",
            ],
        }
    }

    pub fn validation_message(&self) -> &'static str {
        match self {
            Self::Sftp => "SFTP name is required",
            Self::Smb => "SMB name and host are required",
            Self::RemotePlugin { .. } => "Remote plugin name and valid config JSON are required",
        }
    }

    pub fn plugin_id(&self) -> Option<&str> {
        match self {
            Self::RemotePlugin { plugin_id, .. } => Some(plugin_id.as_str()),
            _ => None,
        }
    }

}

#[derive(Debug, Clone)]
pub struct RemoteEditState {
    pub kind: RemoteEditKind,
    pub fields: [String; 6],
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

impl RemoteEditState {
    pub const NAME: usize = 0;
    pub const HOST: usize = 1;
    pub const USER: usize = 2;
    pub const PORT: usize = 3;
    pub const PATH: usize = 4;
    pub const SECRET: usize = 5;
    pub const SAVE: usize = 6;
    pub const CANCEL: usize = 7;

    pub fn new(kind: RemoteEditKind) -> Self {
        let fields = match &kind {
            RemoteEditKind::Sftp => [
                String::new(),
                String::new(),
                String::new(),
                "22".into(),
                "~".into(),
                String::new(),
            ],
            RemoteEditKind::RemotePlugin { display_name, .. } => [
                display_name.clone(),
                "{}".into(),
                "/".into(),
                String::new(),
                String::new(),
                String::new(),
            ],
            _ => Default::default(),
        };
        let input_cursor = fields[Self::NAME].len();
        Self {
            kind,
            fields,
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
                [
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
                [
                    profile.name.clone(),
                    smb.host.clone(),
                    smb.user.clone().unwrap_or_default(),
                    smb.workgroup.clone().unwrap_or_default(),
                    smb.share.clone().unwrap_or_default(),
                    smb.password.clone().unwrap_or_default(),
                ],
            ),
            RemoteKind::RemotePlugin(plugin) => {
                let display_name = discover_remote_plugin_choices()
                    .into_iter()
                    .find(|(id, _, _)| *id == plugin.plugin_id)
                    .map(|(_, name, _)| name)
                    .unwrap_or_else(|| plugin.plugin_id.clone());
                (
                    RemoteEditKind::RemotePlugin {
                        plugin_id: plugin.plugin_id.clone(),
                        display_name,
                        scheme: plugin.scheme.clone(),
                    },
                    [
                        profile.name.clone(),
                        plugin.config_json.clone(),
                        plugin.path.clone().unwrap_or_else(|| "/".to_string()),
                        String::new(),
                        String::new(),
                        String::new(),
                    ],
                )
            }
        };
        Self {
            kind,
            input_cursor: fields[Self::NAME].len(),
            fields,
            cursor: 0,
            edit_original_name: Some(profile.name.clone()),
            share_picker: None,
            plugin_auth_session_json: None,
            plugin_auth_enabled: false,
        }
    }

    pub fn current_value(&self) -> Option<&String> {
        self.fields.get(self.cursor)
    }

    pub fn current_value_mut(&mut self) -> Option<&mut String> {
        self.fields.get_mut(self.cursor)
    }

    pub fn sync_cursor(&mut self) {
        self.input_cursor = self.current_value().map(|s| s.len()).unwrap_or(0);
    }

    pub fn build_profile(&self) -> Option<RemoteProfile> {
        let name = self.fields[Self::NAME].trim();
        if name.is_empty() {
            return None;
        }
        let port = if self.fields[Self::PORT].trim().is_empty() {
            None
        } else {
            self.fields[Self::PORT].trim().parse::<u16>().ok()
        };
        Some(match &self.kind {
            RemoteEditKind::Sftp => RemoteProfile {
                name: name.to_string(),
                source: RemoteSource::UserToml,
                kind: RemoteKind::Sftp(crate::remote::SftpProfile {
                    host: trim_opt(&self.fields[Self::HOST]),
                    user: trim_opt(&self.fields[Self::USER]),
                    port,
                    path: trim_opt(&self.fields[Self::PATH]),
                    identity_file: trim_opt(&self.fields[Self::SECRET]),
                }),
            },
            RemoteEditKind::Smb => {
                let host = self.fields[Self::HOST].trim();
                if host.is_empty() {
                    return None;
                }
                RemoteProfile {
                    name: name.to_string(),
                    source: RemoteSource::UserToml,
                    kind: RemoteKind::Smb(crate::remote::SmbProfile {
                        host: host.to_string(),
                        user: trim_opt(&self.fields[Self::USER]),
                        workgroup: trim_opt(&self.fields[Self::PORT]),
                        share: trim_opt(&self.fields[Self::PATH]),
                        password: trim_opt(&self.fields[Self::SECRET]),
                        path: None,
                    }),
                }
            }
            RemoteEditKind::RemotePlugin { plugin_id, scheme, .. } => {
                let config_json = self.fields[Self::HOST].trim();
                if config_json.is_empty() {
                    return None;
                }
                if serde_json::from_str::<serde_json::Value>(config_json).is_err() {
                    return None;
                }
                let path = trim_opt(&self.fields[Self::USER]);
                RemoteProfile {
                    name: name.to_string(),
                    source: RemoteSource::UserToml,
                    kind: RemoteKind::RemotePlugin(crate::remote::RemotePluginProfile {
                        plugin_id: plugin_id.clone(),
                        scheme: scheme.clone(),
                        config_json: config_json.to_string(),
                        path,
                    }),
                }
            }
        })
    }

    pub fn is_remote_plugin(&self) -> bool {
        matches!(&self.kind, RemoteEditKind::RemotePlugin { .. })
    }

    pub fn plugin_config_json(&self) -> Option<&str> {
        if self.is_remote_plugin() {
            Some(self.fields[Self::HOST].trim())
        } else {
            None
        }
    }

    pub fn plugin_auth_input(&self) -> Option<&str> {
        if self.is_remote_plugin() {
            Some(self.fields[Self::PORT].trim())
        } else {
            None
        }
    }
}

fn discover_remote_plugin_choices() -> Vec<(String, String, String)> {
    let Ok(plugins_dir) = crate::plugins::plugins_dir() else {
        return Vec::new();
    };
    let manifests = crate::remote_plugins::discover_remote_rust_plugin_manifests(&plugins_dir)
        .unwrap_or_default();

    manifests
        .into_iter()
        .map(|manifest| (manifest.id.clone(), manifest.name, manifest.id))
        .collect()
}


fn trim_opt(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
