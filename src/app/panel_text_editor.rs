use anyhow::Result;
use ratatui_textarea::TextArea;
use std::fs;
use std::path::PathBuf;

use super::{ActivePanel, App, AppMode, ConfirmAction, ConfirmDialog};

pub struct PanelTextEditorState {
    pub textarea: TextArea<'static>,
    pub path: Option<PathBuf>,
    pub wrap: bool,
    saved_text: String,
}

impl PanelTextEditorState {
    fn configure_textarea(textarea: &mut TextArea<'static>) {
        let line_number_style = textarea.style();
        textarea.set_line_number_style(line_number_style);
    }

    pub fn from_file(path: PathBuf) -> Result<Self> {
        let bytes = fs::read(&path)?;
        let text = String::from_utf8_lossy(&bytes)
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let lines = text
            .split('\n')
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let mut textarea = TextArea::from(lines);
        Self::configure_textarea(&mut textarea);
        Ok(Self {
            textarea,
            path: Some(path),
            wrap: false,
            saved_text: text,
        })
    }

    pub fn scratch() -> Self {
        let mut textarea = TextArea::from(vec![String::new()]);
        Self::configure_textarea(&mut textarea);
        Self {
            textarea,
            path: None,
            wrap: false,
            saved_text: String::new(),
        }
    }

    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "scratch.txt".to_string());
        if self.current_text() != self.saved_text {
            format!("{name} *")
        } else {
            name
        }
    }

    pub fn current_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn is_modified(&self) -> bool {
        self.current_text() != self.saved_text
    }

    pub fn save(&mut self) -> Result<()> {
        let Some(path) = self.path.clone() else {
            anyhow::bail!("Scratch buffer has no backing file");
        };
        let text = self.current_text();
        fs::write(&path, text.as_bytes())?;
        self.saved_text = text;
        Ok(())
    }
}

impl App {
    pub fn panel_text_editor_side(&self) -> Option<ActivePanel> {
        self.panel_text_editor_side
    }

    pub fn close_panel_text_editor(&mut self) {
        self.panel_text_editor = None;
        self.panel_text_editor_side = None;
        self.panel_text_editor_active = false;
    }

    pub fn panel_text_editor_modified(&self) -> bool {
        self.panel_text_editor
            .as_ref()
            .map(PanelTextEditorState::is_modified)
            .unwrap_or(false)
    }

    pub fn request_close_panel_text_editor(&mut self) -> Result<()> {
        self.close_panel_text_editor_or_confirm();
        Ok(())
    }

    pub fn close_panel_text_editor_or_confirm(&mut self) -> bool {
        if self.panel_text_editor.is_none() {
            return true;
        }

        if !self.panel_text_editor_modified() {
            self.close_panel_text_editor();
            return true;
        }

        self.mode = AppMode::Confirm(ConfirmDialog {
            title: None,
            message: None,
            action: ConfirmAction::CloseTextEditorUnsaved,
            macro_name: Some("confirm_text_editor_unsaved"),
            active_button: crate::app::ConfirmButton::Primary,
        });
        false
    }

    pub fn open_panel_text_editor_with_path(
        &mut self,
        path: PathBuf,
        side: ActivePanel,
    ) -> Result<()> {
        if !self.close_panel_text_editor_or_confirm() {
            return Ok(());
        }
        self.close_quick_preview();
        self.close_file_id_view();
        let editor = PanelTextEditorState::from_file(path)?;
        self.panel_text_editor = Some(editor);
        self.panel_text_editor_side = Some(side);
        // Keep focus coherent with restored active panel.
        self.panel_text_editor_active = self.active == side;
        Ok(())
    }

    pub fn open_panel_text_editor(&mut self) -> Result<()> {
        if self.panel_text_editor.is_some() {
            self.close_panel_text_editor_or_confirm();
            return Ok(());
        }

        self.close_quick_preview();
        self.close_file_id_view();

        let side = self.active;
        let editor = if self.active_panel().is_remote_view() {
            PanelTextEditorState::scratch()
        } else if let Some(entry) = self.active_panel().current_entry() {
            if !entry.is_dir
                && entry.name != ".."
                && entry.name != "[disconnect]"
                && !entry.cloud_only
            {
                PanelTextEditorState::from_file(entry.path.clone())?
            } else {
                PanelTextEditorState::scratch()
            }
        } else {
            PanelTextEditorState::scratch()
        };

        self.panel_text_editor = Some(editor);
        self.panel_text_editor_side = Some(side);
        self.panel_text_editor_active = true;
        Ok(())
    }

    pub fn save_panel_text_editor(&mut self) -> Result<()> {
        let saved_path = self.panel_text_editor.as_ref().and_then(|e| e.path.clone());
        if let Some(editor) = self.panel_text_editor.as_mut() {
            editor.save()?;
            if saved_path.as_ref().is_some_and(|path| is_config_path(path)) {
                self.reload_config_from_disk()?;
                return Ok(());
            }
            if self.config.auto_reload {
                self.reload_panels();
            }
            self.set_status("Text editor: file saved");
        }
        Ok(())
    }

    pub fn toggle_panel_text_editor_wrap(&mut self) {
        if let Some(editor) = self.panel_text_editor.as_mut() {
            editor.wrap = !editor.wrap;
            let wrap = editor.wrap;
            self.set_status(if wrap {
                "Text editor: wrap on"
            } else {
                "Text editor: wrap off"
            });
        }
    }
}

fn is_config_path(path: &std::path::Path) -> bool {
    let Ok(config_path) = crate::config::config_path() else {
        return false;
    };
    same_path(path, &config_path)
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::PanelTextEditorState;
    use std::fs;

    #[test]
    fn file_text_roundtrips_with_final_newline() {
        let path =
            std::env::temp_dir().join(format!("kkc-panel-text-editor-{}.txt", std::process::id()));
        fs::write(&path, b"one\ntwo\n").unwrap();

        let editor = PanelTextEditorState::from_file(path.clone()).unwrap();

        assert_eq!(editor.current_text(), "one\ntwo\n");
        assert!(!editor.is_modified());

        let _ = fs::remove_file(path);
    }
}
