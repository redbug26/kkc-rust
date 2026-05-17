use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct HelpSpan {
    pub text: String,
    pub link_target: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HelpLine {
    pub spans: Vec<HelpSpan>,
}

#[derive(Debug, Clone)]
pub struct HelpTopic {
    pub title: String,
    pub lines: Vec<HelpLine>,
}

#[derive(Debug, Clone)]
pub struct HelpSection {
    pub title: String,
    pub topics: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct HelpSystem {
    pub sections: Vec<HelpSection>,
    pub topics: Vec<HelpTopic>,
    topic_index: HashMap<String, usize>,
}

#[derive(Debug, Clone, Copy)]
pub enum HelpView {
    Index {
        cursor: usize,
    },
    Topics {
        section: usize,
        cursor: usize,
    },
    Page {
        topic: usize,
        scroll: u16,
        selected_link: usize,
    },
}

#[derive(Debug, Clone)]
pub struct HelpState {
    pub system: HelpSystem,
    pub view: HelpView,
    pub history: Vec<HelpView>,
    /// Path of the loaded .hlp file, or "(built-in)" if using the embedded one.
    pub hlp_path: String,
}

impl HelpState {
    pub fn back(&mut self) -> bool {
        if let Some(view) = self.history.pop() {
            self.view = view;
            true
        } else {
            false
        }
    }

    pub fn open_index(&mut self) {
        self.history.push(self.view);
        self.view = HelpView::Index { cursor: 0 };
    }

    pub fn open_topic_by_name(&mut self, name: &str) -> bool {
        if help_key(name) == "index" {
            self.open_index();
            return true;
        }
        if let Some(topic) = self.system.find_topic(name) {
            self.history.push(self.view);
            self.view = HelpView::Page {
                topic,
                scroll: 0,
                selected_link: 0,
            };
            true
        } else {
            false
        }
    }
}

impl HelpSystem {
    pub fn find_topic(&self, name: &str) -> Option<usize> {
        self.topic_index.get(&help_key(name)).copied()
    }
}

impl HelpTopic {
    pub fn link_count(&self) -> usize {
        self.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .filter(|span| span.link_target.is_some())
                    .count()
            })
            .sum()
    }

    pub fn selected_link_target(&self, selected_link: usize) -> Option<&str> {
        let mut idx = 0usize;
        for line in &self.lines {
            for span in &line.spans {
                if let Some(target) = span.link_target.as_deref() {
                    if idx == selected_link {
                        return Some(target);
                    }
                    idx += 1;
                }
            }
        }
        None
    }

    pub fn to_render_lines(&self, selected_link: usize) -> Vec<Line<'static>> {
        let mut current_link = 0usize;
        self.lines
            .iter()
            .map(|line| {
                let spans = line
                    .spans
                    .iter()
                    .map(|span| {
                        let style = match &span.link_target {
                            Some(_) if current_link == selected_link => {
                                current_link += 1;
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD)
                            }
                            Some(_) => {
                                current_link += 1;
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::UNDERLINED)
                            }
                            None => Style::default().fg(Color::White),
                        };
                        Span::styled(span.text.clone(), style)
                    })
                    .collect::<Vec<_>>();

                Line::from(spans)
            })
            .collect()
    }
}

fn help_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}
