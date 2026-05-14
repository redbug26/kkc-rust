use super::*;

pub(crate) fn help_shortcuts(view: &crate::help::HelpView) -> Vec<FooterShortcut> {
    match view {
        HelpView::Index { .. } => vec![
            FooterShortcut {
                label: " \u{238B} :Close",
                key: KeyCode::Esc,
            },
            FooterShortcut {
                label: "F10:Close",
                key: KeyCode::F(10),
            },
            FooterShortcut {
                label: " \u{23ce} :Open",
                key: KeyCode::Enter,
            },
        ],
        HelpView::Topics { .. } => vec![
            FooterShortcut {
                label: " \u{238B} :Close",
                key: KeyCode::Esc,
            },
            FooterShortcut {
                label: "Backspace:Back",
                key: KeyCode::Backspace,
            },
            FooterShortcut {
                label: " \u{23ce} :Open",
                key: KeyCode::Enter,
            },
        ],
        HelpView::Page { .. } => vec![
            FooterShortcut {
                label: " \u{238B} :Close",
                key: KeyCode::Esc,
            },
            FooterShortcut {
                label: "Backspace:Back",
                key: KeyCode::Backspace,
            },
            FooterShortcut {
                label: "PgUpPgDn:Scroll",
                key: KeyCode::Null,
            },
            FooterShortcut {
                label: " \u{21E5} :NextLink",
                key: KeyCode::Tab,
            },
            FooterShortcut {
                label: " \u{23ce} :Open",
                key: KeyCode::Enter,
            },
        ],
    }
}

pub(super) fn render_help(f: &mut Frame, state: &crate::help::HelpState, area: Rect) {
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        },
    );

    safe_render_widget(f, Clear, popup);
    let title = match state.view {
        HelpView::Index { .. } => " KKC-DOS Help Index ",
        HelpView::Topics { section, .. } => {
            let title = &state.system.sections[section].title;
            return render_help_with_title(f, popup, title, state);
        }
        HelpView::Page { topic, .. } => {
            let title = &state.system.topics[topic].title;
            return render_help_with_title(f, popup, title, state);
        }
    };
    render_help_with_title(f, popup, title, state);
}

pub(super) fn render_help_with_title(
    f: &mut Frame,
    popup: Rect,
    title: &str,
    state: &crate::help::HelpState,
) {
    let block = Block::default()
        .title(format!(" {} ", title))
        .title_bottom(
            Line::from(Span::styled(
                format!(" {} ", state.hlp_path),
                Style::default().fg(Color::DarkGray).bg(clr_app_bg()),
            ))
            .right_aligned(),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(clr_panel_border()).bg(clr_app_bg()))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    if inner.height < 3 {
        return;
    }

    let body = clamp_rect(
        popup,
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(1),
        },
    );
    let footer = clamp_rect(
        popup,
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        },
    );

    match state.view {
        HelpView::Index { cursor } => {
            let items: Vec<ListItem> = state
                .system
                .sections
                .iter()
                .enumerate()
                .map(|(idx, section)| {
                    let style = if idx == cursor {
                        Style::default()
                            .fg(Color::Black)
                            .bg(clr_selected())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(Span::styled(
                        format!(" {}", section.title),
                        style,
                    )))
                })
                .collect();
            safe_render_widget(f, List::new(items), body);
            let items = footer_shortcut_items(&help_shortcuts(&state.view));
            render_shortcut_bar(f, footer, &items, secondary_shortcut_bar_style());
        }
        HelpView::Topics { section, cursor } => {
            let items: Vec<ListItem> = state.system.sections[section]
                .topics
                .iter()
                .enumerate()
                .map(|(idx, topic_idx)| {
                    let style = if idx == cursor {
                        Style::default()
                            .fg(Color::Black)
                            .bg(clr_selected())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(Span::styled(
                        format!(" {}", state.system.topics[*topic_idx].title),
                        style,
                    )))
                })
                .collect();
            safe_render_widget(f, List::new(items), body);
            let items = footer_shortcut_items(&help_shortcuts(&state.view));
            render_shortcut_bar(f, footer, &items, secondary_shortcut_bar_style());
        }
        HelpView::Page {
            topic,
            scroll,
            selected_link,
        } => {
            let topic = &state.system.topics[topic];
            safe_render_widget(
                f,
                Paragraph::new(topic.to_render_lines(selected_link))
                    .style(Style::default().fg(Color::White))
                    .scroll((scroll, 0))
                    .wrap(Wrap { trim: false }),
                body,
            );
            let items = footer_shortcut_items(&help_shortcuts(&state.view));
            render_shortcut_bar(f, footer, &items, secondary_shortcut_bar_style());
        }
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------
