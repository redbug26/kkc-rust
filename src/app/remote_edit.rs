use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteEditKind {
    Sftp,
    Imap,
    Smb,
}

impl RemoteEditKind {
    /// All protocol choices in menu order.
    pub fn all() -> &'static [Self] {
        &[Self::Sftp, Self::Imap, Self::Smb]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Sftp => "SFTP",
            Self::Imap => "IMAP",
            Self::Smb => "SMB",
        }
    }

    /// UI accent colour (R, G, B).
    pub fn color_rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Sftp => (121, 214, 255),
            Self::Imap => (181, 238, 170),
            Self::Smb => (255, 165, 80),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Sftp => " Add SFTP Server ",
            Self::Imap => " Add IMAP Server ",
            Self::Smb => " Add SMB Server ",
        }
    }

    pub fn field_labels(self) -> [&'static str; 6] {
        match self {
            Self::Sftp => ["Name", "Host", "User", "Port", "Path", "Identity"],
            Self::Imap => ["Name", "Host", "User", "Port", "Mailbox", "Password"],
            Self::Smb => ["Name", "Host", "User", "Workgroup", "Share", "Password"],
        }
    }

    pub fn validation_message(self) -> &'static str {
        match self {
            Self::Sftp => "SFTP name is required",
            Self::Imap => "IMAP name, host and user are required",
            Self::Smb => "SMB name and host are required",
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
        let fields = match kind {
            RemoteEditKind::Sftp => [
                String::new(),
                String::new(),
                String::new(),
                "22".into(),
                "~".into(),
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
            RemoteKind::Imap(imap) => (
                RemoteEditKind::Imap,
                [
                    profile.name.clone(),
                    imap.host.clone(),
                    imap.user.clone(),
                    imap.port.map(|p| p.to_string()).unwrap_or_default(),
                    imap.path.clone().unwrap_or_default(),
                    imap.password.clone().unwrap_or_default(),
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
        };
        Self {
            kind,
            input_cursor: fields[Self::NAME].len(),
            fields,
            cursor: 0,
            edit_original_name: Some(profile.name.clone()),
            share_picker: None,
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
        Some(match self.kind {
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
            RemoteEditKind::Imap => {
                let host = self.fields[Self::HOST].trim();
                let user = self.fields[Self::USER].trim();
                if host.is_empty() || user.is_empty() {
                    return None;
                }
                RemoteProfile {
                    name: name.to_string(),
                    source: RemoteSource::UserToml,
                    kind: RemoteKind::Imap(crate::remote::ImapProfile {
                        host: host.to_string(),
                        user: user.to_string(),
                        port,
                        path: trim_opt(&self.fields[Self::PATH]),
                        password: trim_opt(&self.fields[Self::SECRET]),
                    }),
                }
            }
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
        })
    }
}


fn trim_opt(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
