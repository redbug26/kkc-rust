use crate::config::{PanelConfig, PanelTabConfig};
use crate::panel::Panel;
use crate::remote::{RemoteKind, RemoteProfile};

#[derive(Debug, Default)]
pub(super) struct PanelTabs {
    before: Vec<Panel>,
    after: Vec<Panel>,
}

impl PanelTabs {
    pub(super) fn count(&self) -> usize {
        self.before.len() + 1 + self.after.len()
    }

    pub(super) fn current_index(&self) -> usize {
        self.before.len()
    }

    fn new_panel_from(current: &Panel) -> Panel {
        Panel::new(current.persisted_path(), current.sort, current.show_hidden)
    }

    pub(super) fn new_tab(current: &mut Panel, tabs: &mut Self) {
        let new_panel = Self::new_panel_from(current);
        let old_current = std::mem::replace(current, new_panel);
        tabs.before.push(old_current);
    }

    pub(super) fn close_tab(current: &mut Panel, tabs: &mut Self) -> bool {
        if tabs.count() <= 1 {
            return false;
        }

        if !tabs.after.is_empty() {
            let next = tabs.after.remove(0);
            let _closed = std::mem::replace(current, next);
        } else if let Some(previous) = tabs.before.pop() {
            let _closed = std::mem::replace(current, previous);
        }
        true
    }

    pub(super) fn next_tab(current: &mut Panel, tabs: &mut Self) -> bool {
        if tabs.count() <= 1 {
            return false;
        }

        if !tabs.after.is_empty() {
            let next = tabs.after.remove(0);
            let old_current = std::mem::replace(current, next);
            tabs.before.push(old_current);
        } else {
            let next = tabs.before.remove(0);
            let old_current = std::mem::replace(current, next);
            let mut new_after = std::mem::take(&mut tabs.before);
            new_after.push(old_current);
            tabs.after = new_after;
        }
        true
    }

    fn export_configs(&self, current: &Panel) -> (Vec<PanelTabConfig>, usize) {
        let mut tabs = Vec::with_capacity(self.count());
        tabs.extend(self.before.iter().map(panel_to_tab_config));
        let active_tab = tabs.len();
        tabs.push(panel_to_tab_config(current));
        tabs.extend(self.after.iter().map(panel_to_tab_config));
        (tabs, active_tab)
    }
}

pub(super) fn panel_config_for_save(panel: &Panel, tabs: &PanelTabs) -> PanelConfig {
    let active = panel_to_tab_config(panel);
    let (all_tabs, active_tab) = tabs.export_configs(panel);
    PanelConfig {
        path: active.path,
        remote_name: active.remote_name,
        remote_path: active.remote_path,
        sort: active.sort,
        show_hidden: active.show_hidden,
        tabs: all_tabs,
        active_tab,
    }
}

pub(super) fn restore_panel_side(
    cfg: &PanelConfig,
    profiles: &[RemoteProfile],
) -> (Panel, PanelTabs) {
    let tab_configs = if cfg.tabs.is_empty() {
        vec![cfg.active_tab_config()]
    } else {
        cfg.tabs.clone()
    };
    let active_idx = cfg.active_tab.min(tab_configs.len().saturating_sub(1));
    let mut current = None;
    let mut tabs = PanelTabs::default();

    for (idx, tab_cfg) in tab_configs.iter().enumerate() {
        let panel = panel_from_tab_config(tab_cfg, profiles);
        if idx < active_idx {
            tabs.before.push(panel);
        } else if idx == active_idx {
            current = Some(panel);
        } else {
            tabs.after.push(panel);
        }
    }

    let current =
        current.unwrap_or_else(|| panel_from_tab_config(&cfg.active_tab_config(), profiles));
    (current, tabs)
}

fn panel_to_tab_config(panel: &Panel) -> PanelTabConfig {
    let cursor_name = panel.current_entry().map(|e| e.name.clone());
    let selected_names = panel
        .entries
        .iter()
        .filter(|e| e.selected && e.name != "..")
        .map(|e| e.name.clone())
        .collect();
    PanelTabConfig {
        path: panel.persisted_path(),
        remote_name: panel.remote_profile().map(|p| p.name),
        remote_path: panel.remote_cwd().map(|s| s.to_string()),
        sort: panel.sort,
        show_hidden: panel.show_hidden,
        cursor_name,
        selected_names,
    }
}

fn panel_from_tab_config(cfg: &PanelTabConfig, profiles: &[RemoteProfile]) -> Panel {
    let mut panel = Panel::new(cfg.path.clone(), cfg.sort, cfg.show_hidden);
    restore_remote_panel(
        &mut panel,
        cfg.remote_name.as_ref(),
        cfg.remote_path.as_ref(),
        profiles,
    );
    if let Some(name) = &cfg.cursor_name {
        panel.restore_cursor_by_name(name);
    }
    panel.restore_selection_by_names(&cfg.selected_names);
    panel
}

fn restore_remote_panel(
    panel: &mut Panel,
    remote_name: Option<&String>,
    remote_path: Option<&String>,
    profiles: &[RemoteProfile],
) {
    let Some(remote_name) = remote_name else {
        return;
    };
    let Some(mut profile) = profiles.iter().find(|p| p.name == *remote_name).cloned() else {
        return;
    };
    if let Some(remote_path) = remote_path.cloned() {
        match &mut profile.kind {
            RemoteKind::Sftp(sftp) => sftp.path = Some(remote_path),
            RemoteKind::Smb(smb) => smb.path = Some(remote_path),
            RemoteKind::RemotePlugin(plugin) => plugin.path = Some(remote_path),
        }
    }
    let _ = panel.enter_remote(profile);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SortMode;
    use std::fs;

    #[test]
    fn panel_tabs_create_cycle_and_close() {
        let root = std::env::temp_dir().join(format!("kkc-tabs-{}", std::process::id()));
        let one = root.join("one");
        let two = root.join("two");
        fs::create_dir_all(&one).expect("create first tab dir");
        fs::create_dir_all(&two).expect("create second tab dir");

        let mut current = Panel::new(one.clone(), SortMode::Name, false);
        let mut tabs = PanelTabs::default();

        PanelTabs::new_tab(&mut current, &mut tabs);
        assert_eq!(tabs.count(), 2);
        assert_eq!(tabs.current_index(), 1);
        assert_eq!(current.path, one);

        current.enter_dir(two.clone()).expect("enter second dir");
        assert!(PanelTabs::next_tab(&mut current, &mut tabs));
        assert_eq!(tabs.current_index(), 0);
        assert_eq!(current.path, one);

        assert!(PanelTabs::close_tab(&mut current, &mut tabs));
        assert_eq!(tabs.count(), 1);
        assert_eq!(current.path, two);
        assert!(!PanelTabs::close_tab(&mut current, &mut tabs));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn panel_tabs_roundtrip_through_config() {
        let root = std::env::temp_dir().join(format!("kkc-tabs-config-{}", std::process::id()));
        let one = root.join("one");
        let two = root.join("two");
        fs::create_dir_all(&one).expect("create first tab dir");
        fs::create_dir_all(&two).expect("create second tab dir");

        let mut current = Panel::new(one.clone(), SortMode::Name, false);
        let mut tabs = PanelTabs::default();
        PanelTabs::new_tab(&mut current, &mut tabs);
        current.enter_dir(two.clone()).expect("enter second dir");

        let cfg = panel_config_for_save(&current, &tabs);
        assert_eq!(cfg.active_tab, 1);
        assert_eq!(cfg.tabs.len(), 2);

        let text = toml::to_string(&cfg).expect("serialize panel config");
        let parsed: PanelConfig = toml::from_str(&text).expect("parse panel config");
        let (restored, restored_tabs) = restore_panel_side(&parsed, &[]);

        assert_eq!(restored.path, two);
        assert_eq!(restored_tabs.count(), 2);
        assert_eq!(restored_tabs.current_index(), 1);
        assert_eq!(restored_tabs.before[0].path, one);

        let _ = fs::remove_dir_all(root);
    }
}
