use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct HelpSpan {
    pub text: String,
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpLineKind {
    Normal,
    Heading,
    CenteredHeading,
}

#[derive(Debug, Clone)]
pub struct HelpLine {
    pub kind: HelpLineKind,
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
}

impl HelpState {
    pub fn load() -> Self {
        Self {
            system: HelpSystem::from_bytes(include_bytes!("../assets/kkc.hlp")),
            view: HelpView::Index { cursor: 0 },
            history: Vec::new(),
        }
    }

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
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let decoded = decode_help(bytes);
        let mut sections: Vec<HelpSection> = Vec::new();
        let mut topics: Vec<HelpTopic> = Vec::new();
        let mut topic_index = HashMap::new();
        let mut current_section: Option<usize> = None;
        let mut current_topic: Option<usize> = None;

        for raw_line in decoded.lines() {
            let line = raw_line.trim_end_matches('\r');
            if line.is_empty() {
                if let Some(topic) = current_topic.and_then(|idx| topics.get_mut(idx)) {
                    topic.lines.push(HelpLine {
                        kind: HelpLineKind::Normal,
                        spans: vec![HelpSpan {
                            text: String::new(),
                            link_target: None,
                        }],
                    });
                }
                continue;
            }

            match line.chars().next().unwrap_or(' ') {
                '*' => {}
                '@' => {
                    let title = line[1..].trim().to_string();
                    sections.push(HelpSection {
                        title,
                        topics: Vec::new(),
                    });
                    current_section = Some(sections.len() - 1);
                    current_topic = None;
                }
                ':' => {
                    let title = line[1..].trim().to_string();
                    let topic_idx = topics.len();
                    topics.push(HelpTopic {
                        title: title.clone(),
                        lines: Vec::new(),
                    });
                    if let Some(section_idx) = current_section {
                        sections[section_idx].topics.push(topic_idx);
                    }
                    topic_index.entry(help_key(&title)).or_insert(topic_idx);
                    current_topic = Some(topic_idx);
                }
                '#' => {
                    if let Some(topic_idx) = current_topic {
                        let alias = line[1..].trim();
                        if !alias.is_empty() {
                            topic_index.entry(help_key(alias)).or_insert(topic_idx);
                        }
                    }
                }
                '^' | '%' => {
                    if let Some(topic_idx) = current_topic {
                        let expanded = expand_tabs(line);
                        let title = expanded[1..].trim();
                        topic_index.entry(help_key(title)).or_insert(topic_idx);
                        topics[topic_idx].lines.push(HelpLine {
                            kind: if line.starts_with('^') {
                                HelpLineKind::CenteredHeading
                            } else {
                                HelpLineKind::Heading
                            },
                            spans: parse_inline_links(title),
                        });
                    }
                }
                _ => {
                    if let Some(topic_idx) = current_topic {
                        let expanded = expand_tabs(line);
                        topics[topic_idx].lines.push(HelpLine {
                            kind: HelpLineKind::Normal,
                            spans: parse_inline_links(&expanded),
                        });
                    }
                }
            }
        }

        Self {
            sections,
            topics,
            topic_index,
        }
    }

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
                        let style = match (&span.link_target, line.kind) {
                            (Some(_), _) if current_link == selected_link => {
                                current_link += 1;
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD)
                            }
                            (Some(_), _) => {
                                current_link += 1;
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::UNDERLINED)
                            }
                            (None, HelpLineKind::CenteredHeading) => Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                            (None, HelpLineKind::Heading) => Style::default()
                                .fg(Color::LightYellow)
                                .add_modifier(Modifier::BOLD),
                            (None, HelpLineKind::Normal) => Style::default().fg(Color::White),
                        };
                        Span::styled(span.text.clone(), style)
                    })
                    .collect::<Vec<_>>();

                match line.kind {
                    HelpLineKind::CenteredHeading => Line::from(spans).centered(),
                    _ => Line::from(spans),
                }
            })
            .collect()
    }
}

fn parse_inline_links(line: &str) -> Vec<HelpSpan> {
    let mut spans = Vec::new();
    let mut rest = line;

    while let Some(start) = rest.find('<') {
        let (before, after_start) = rest.split_at(start);
        if !before.is_empty() {
            spans.push(HelpSpan {
                text: before.to_string(),
                link_target: None,
            });
        }

        let Some(end) = after_start.find('>') else {
            spans.push(HelpSpan {
                text: after_start.to_string(),
                link_target: None,
            });
            return spans;
        };

        let token = &after_start[1..end];
        if let Some((target, label)) = token.split_once(';') {
            spans.push(HelpSpan {
                text: label.to_string(),
                link_target: Some(target.to_string()),
            });
        } else {
            spans.push(HelpSpan {
                text: after_start[..=end].to_string(),
                link_target: None,
            });
        }

        rest = &after_start[end + 1..];
    }

    if !rest.is_empty() || spans.is_empty() {
        spans.push(HelpSpan {
            text: rest.to_string(),
            link_target: None,
        });
    }

    spans
}

fn expand_tabs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col = 0usize;

    for ch in line.chars() {
        if ch == '\t' {
            let spaces = 8 - (col % 8);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(ch);
            col += 1;
        }
    }

    out
}

fn help_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn decode_help(bytes: &[u8]) -> String {
    bytes.iter().map(|b| decode_cp437(*b)).collect()
}

fn decode_cp437(byte: u8) -> char {
    match byte {
        b'\n' => '\n',
        b'\r' => '\r',
        b'\t' => '\t',
        0x20..=0x7e => byte as char,
        0xB3 | 0xBA => '|',
        0xC4 | 0xCD | 0xC6 | 0xC7 | 0xCC | 0xCE => '-',
        0xDA | 0xBF | 0xC0 | 0xD9 | 0xC9 | 0xBB | 0xC8 | 0xBC | 0xC3 | 0xB4 | 0xC2 | 0xC1
        | 0xC5 | 0xD1 | 0xD2 | 0xD3 | 0xD4 | 0xD5 | 0xD6 | 0xD7 | 0xD8 => '+',
        0xB0 | 0xB1 | 0xB2 | 0xDB | 0xDC | 0xDD | 0xDE | 0xDF => '#',
        0xF9 => '.',
        0xFA => '*',
        _ => '?',
    }
}
