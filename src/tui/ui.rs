use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Table};
use ratatui::Frame;

use crate::session::{Session, SessionStatus};
use crate::tui::app::App;

pub fn draw(f: &mut Frame, app: &mut App) {
    draw_main(f, app);
    if let Some(picker) = &app.picker {
        draw_picker(f, picker);
    }
    if app.log_viewer.is_some() {
        draw_log_viewer(f, app);
    }
    if let Some(ce) = &app.config_editor {
        draw_config_editor(f, ce);
    }
    if let Some(fp) = &app.focus_picker {
        draw_focus_picker(f, fp);
    }
}

fn draw_main(f: &mut Frame, app: &mut App) {
    let is_detailed = app.config.view_mode == "detailed";

    let chunks = if is_detailed {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // header
                Constraint::Min(8),    // cards
                Constraint::Length(1), // footer
            ])
            .split(f.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // header
                Constraint::Min(8),    // table
                Constraint::Length(8), // detail
                Constraint::Length(1), // footer
            ])
            .split(f.area())
    };

    draw_header(f, chunks[0], app);
    if is_detailed {
        draw_detailed_view(f, chunks[1], app);
        draw_footer(f, chunks[2], app);
    } else {
        draw_table(f, chunks[1], app);
        draw_detail(f, chunks[2], app);
        draw_footer(f, chunks[3], app);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let alive = app
        .sessions
        .iter()
        .filter(|s| s.status != SessionStatus::Dead)
        .count();
    let dead = app.sessions.len() - alive;

    let mut spans = vec![
        Span::styled(
            " Claude Code Command Center ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(format!("{} active", alive), Style::default().fg(Color::Green)),
        Span::raw(" | "),
        Span::styled(format!("{} terminated", dead), Style::default().fg(Color::DarkGray)),
    ];

    if let Some(hk) = &app.hotkey_display {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("[{}]", hk),
            Style::default().fg(Color::Yellow),
        ));
    }

    let text = Line::from(spans);

    let block = Block::default().borders(Borders::ALL);
    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

fn draw_table(f: &mut Frame, area: Rect, app: &mut App) {
    use crate::tui::app::{SortColumn, SortDir};

    let arrow = match app.sort_dir {
        SortDir::Asc => " ^",
        SortDir::Desc => " v",
    };

    let sort_label = |col: SortColumn, label: &str| -> String {
        if app.sort_column == col {
            format!("{}{}", label, arrow)
        } else {
            label.to_string()
        }
    };

    let header = Row::new(vec![
        Cell::from(" # "),
        Cell::from(sort_label(SortColumn::Project, "Project")),
        Cell::from("Task"),
        Cell::from("Session"),
        Cell::from(sort_label(SortColumn::Status, "Status")),
        Cell::from(sort_label(SortColumn::Context, "Ctx")),
        Cell::from(sort_label(SortColumn::Cost, "Cost")),
        Cell::from(sort_label(SortColumn::Messages, "Msgs")),
        Cell::from("Jobs"),
        Cell::from(""),
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    // Reserve 1 row for search bar if active
    let search_height: u16 = if app.searching || !app.search_query.is_empty() { 3 } else { 0 };
    // Calculate page size from available area (borders=2, header=1)
    let available_rows = area.height.saturating_sub(3 + search_height) as usize;
    if available_rows > 0 {
        app.page_size = available_rows;
    }
    app.clamp_page();
    let range = app.page_range();

    // Build visible items: (visible_position, session_index, &Session)
    let visible_items: Vec<(usize, usize, &Session)> = (range.start..range.end)
        .filter_map(|vis_pos| {
            let real_idx = app.visible_session_index(vis_pos)?;
            Some((vis_pos, real_idx, &app.sessions[real_idx]))
        })
        .collect();

    let rows: Vec<Row> = visible_items
        .iter()
        .map(|&(vis_pos, real_idx, s)| {
            let status_style = match s.status {
                SessionStatus::WaitingForApproval => Style::default().fg(Color::Yellow),
                SessionStatus::Idle => Style::default().fg(Color::Magenta),
                SessionStatus::Thinking => Style::default().fg(Color::Green),
                SessionStatus::Dead => Style::default().fg(Color::Red),
            };

            let ctx_pct = s.context_usage.percentage();
            let ctx_style = if ctx_pct > 80.0 {
                Style::default().fg(Color::Red)
            } else if ctx_pct > 50.0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Green)
            };

            let cost = s.cost.estimated_cost_usd(s.model.as_deref());

            let is_external = s.status != SessionStatus::Dead && !s.in_iterm;

            let row_style = if vis_pos == app.selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if s.status == SessionStatus::Dead || is_external {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(format!(" {} ", real_idx + 1)),
                if s.is_ephemeral {
                    Cell::from(Span::styled("~ ephemeral", Style::default().fg(Color::Cyan)))
                } else {
                    Cell::from(s.project_name.clone())
                },
                Cell::from(
                    s.summary
                        .as_deref()
                        .unwrap_or("-")
                        .to_string(),
                ),
                Cell::from(s.session_id[..8].to_string()),
                Cell::from(Span::styled(s.status.label(), status_style)),
                Cell::from(if s.status == SessionStatus::Dead || s.context_usage.current_tokens == 0 {
                    " -- ".to_string()
                } else {
                    format!("{:.0}%", ctx_pct)
                })
                .style(ctx_style),
                Cell::from(format!("${:.2}", cost)),
                Cell::from(format!("{}", s.message_count)),
                {
                    let total = s.active_agents + s.active_bg_jobs;
                    if total > 0 {
                        Cell::from(format!("{}", total))
                            .style(Style::default().fg(Color::Magenta))
                    } else {
                        Cell::from("-".to_string())
                            .style(Style::default().fg(Color::DarkGray))
                    }
                },
                if is_external {
                    Cell::from("ext").style(if vis_pos == app.selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    })
                } else {
                    Cell::from("")
                },
            ])
            .style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Length(4),  // #
        Constraint::Length(12), // Project
        Constraint::Min(20),   // Task (takes remaining space)
        Constraint::Length(9),  // Session
        Constraint::Length(10), // Status
        Constraint::Length(5),  // Ctx
        Constraint::Length(8),  // Cost
        Constraint::Length(5),  // Msgs
        Constraint::Length(5),  // Jobs
        Constraint::Length(4),  // ext
    ];

    let page_info = if app.total_pages() > 1 {
        format!(" Sessions [{}/{}] ", app.page + 1, app.total_pages())
    } else {
        " Sessions ".to_string()
    };

    // Split area for search bar if active
    let (table_area, search_area) = if search_height > 0 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(search_height)])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(page_info));

    f.render_widget(table, table_area);

    // Render search bar
    if let Some(sa) = search_area {
        let search_text = if app.search_query.is_empty() && app.searching {
            " Type to filter by project/task...".to_string()
        } else {
            format!(" {}", &app.search_query)
        };
        let style = if app.searching {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let match_count = if !app.search_query.is_empty() {
            format!(" {}/{} ", app.filtered_indices.len(), app.sessions.len())
        } else {
            String::new()
        };
        let search = Paragraph::new(search_text).style(style).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if app.searching { Color::Yellow } else { Color::DarkGray }))
                .title(format!(" / Search{}", match_count)),
        );
        f.render_widget(search, sa);
        if app.searching {
            f.set_cursor_position((
                sa.x + 1 + app.search_query.len() as u16,
                sa.y + 1,
            ));
        }
    }
}

fn draw_detail(f: &mut Frame, area: Rect, app: &App) {
    let content = if let Some(s) = app.selected_session() {
        let elapsed = chrono::Utc::now()
            .signed_duration_since(s.started_at);
        let duration = format_duration(elapsed);

        let model_str = s
            .model
            .as_deref()
            .unwrap_or("unknown");

        let last_msg = s
            .last_message_preview
            .as_deref()
            .unwrap_or("(no messages)");

        let ago = s
            .last_message_at
            .map(|t| {
                let d = chrono::Utc::now().signed_duration_since(t);
                format_duration(d) + " ago"
            })
            .unwrap_or_else(|| "-".to_string());

        // Context bar
        let ctx_pct = s.context_usage.percentage();

        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(Block::default().borders(Borders::ALL).inner(area));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " {} ({}) ",
                s.project_name,
                s.summary.as_deref().unwrap_or(s.git_branch.as_deref().unwrap_or("-"))
            ));
        f.render_widget(block, area);

        let info_line = Line::from(vec![
            Span::raw("Started: "),
            Span::styled(&duration, Style::default().fg(Color::Cyan)),
            Span::raw(" ago | Messages: "),
            Span::styled(
                format!("{}", s.message_count),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" | Model: "),
            Span::styled(model_str, Style::default().fg(Color::Cyan)),
            Span::raw(" | PID: "),
            Span::styled(
                format!("{}", s.pid),
                Style::default().fg(Color::Cyan),
            ),
            if s.active_agents > 0 || s.active_bg_jobs > 0 {
                Span::raw(" | ")
            } else {
                Span::raw("")
            },
            if s.active_agents > 0 {
                Span::styled(
                    format!("{} agent{}", s.active_agents, if s.active_agents == 1 { "" } else { "s" }),
                    Style::default().fg(Color::Magenta),
                )
            } else {
                Span::raw("")
            },
            if s.active_agents > 0 && s.active_bg_jobs > 0 {
                Span::raw(", ")
            } else {
                Span::raw("")
            },
            if s.active_bg_jobs > 0 {
                Span::styled(
                    format!("{} bg job{}", s.active_bg_jobs, if s.active_bg_jobs == 1 { "" } else { "s" }),
                    Style::default().fg(Color::Magenta),
                )
            } else {
                Span::raw("")
            },
        ]);
        f.render_widget(Paragraph::new(info_line), inner[0]);

        let last_line = Line::from(vec![
            Span::raw("Last: "),
            Span::styled(
                format!("\"{}\"", last_msg),
                Style::default().fg(Color::White),
            ),
            Span::raw(format!(" ({})", ago)),
        ]);
        f.render_widget(Paragraph::new(last_line), inner[1]);

        let cost = s.cost.estimated_cost_usd(s.model.as_deref());
        let cost_line = Line::from(vec![
            Span::raw("Cost: "),
            Span::styled(
                format!("${:.4}", cost),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(format!(
                " (in: {}  out: {}  new: {}  cache_r: {}  cache_w: {})",
                format_tokens(s.cost.input_tokens + s.cost.cache_read_tokens + s.cost.cache_creation_tokens),
                format_tokens(s.cost.output_tokens),
                format_tokens(s.cost.input_tokens),
                format_tokens(s.cost.cache_read_tokens),
                format_tokens(s.cost.cache_creation_tokens),
            )),
        ]);
        f.render_widget(Paragraph::new(cost_line), inner[2]);

        // Context gauge
        let gauge = Gauge::default()
            .label(format!(
                "Context: {} / {} ({:.1}%)",
                format_tokens(s.context_usage.current_tokens),
                format_tokens(s.context_usage.max_tokens),
                ctx_pct
            ))
            .ratio((ctx_pct as f64 / 100.0).min(1.0))
            .gauge_style(
                Style::default().fg(if ctx_pct > 80.0 {
                    Color::Red
                } else if ctx_pct > 50.0 {
                    Color::Rgb(200, 130, 0)
                } else {
                    Color::Green
                }),
            );
        f.render_widget(gauge, inner[3]);

        return;
    } else {
        vec![Line::from("No sessions found")]
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Detail ");
    let paragraph = Paragraph::new(content).block(block);
    f.render_widget(paragraph, area);
}

fn draw_detailed_view(f: &mut Frame, area: Rect, app: &App) {
    let card_height: u16 = 6;
    let visible_cards = (area.height / card_height) as usize;
    if app.visible_count() == 0 {
        let block = Block::default().borders(Borders::ALL).title(" Sessions ");
        let p = Paragraph::new("  No sessions").block(block);
        f.render_widget(p, area);
        return;
    }

    // Scroll so selected is visible
    let scroll_offset = if app.selected >= visible_cards {
        app.selected - visible_cards + 1
    } else {
        0
    };

    let block = Block::default().borders(Borders::ALL).title(" Sessions (detailed) ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let constraints: Vec<Constraint> = (0..visible_cards)
        .map(|_| Constraint::Length(card_height))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();

    let card_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (vi, vis_pos) in (scroll_offset..app.visible_count())
        .take(visible_cards)
        .enumerate()
    {
        let real_idx = match app.visible_session_index(vis_pos) {
            Some(i) => i,
            None => break,
        };
        let s = &app.sessions[real_idx];
        let selected = vis_pos == app.selected;
        let card_area = card_areas[vi];

        let status_style = match s.status {
            SessionStatus::WaitingForApproval => Style::default().fg(Color::Yellow),
            SessionStatus::Idle => Style::default().fg(Color::DarkGray),
            SessionStatus::Thinking => Style::default().fg(Color::Green),
            SessionStatus::Dead => Style::default().fg(Color::DarkGray),
        };

        let border_style = if selected {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let card_block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    &s.project_name,
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    s.summary.as_deref().unwrap_or("-"),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(" "),
                Span::styled(s.status.label(), status_style),
                Span::raw(" "),
            ]));

        let card_inner = card_block.inner(card_area);
        f.render_widget(card_block, card_area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(card_inner);

        // Row 1: model, pid, duration, messages, agents
        let elapsed = chrono::Utc::now().signed_duration_since(s.started_at);
        let duration = format_duration(elapsed);
        let model_str = s.model.as_deref().unwrap_or("?");
        let mut info_spans = vec![
            Span::raw(" "),
            Span::styled(model_str, Style::default().fg(Color::Cyan)),
            Span::raw("  PID:"),
            Span::styled(format!("{}", s.pid), Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled(&duration, Style::default().fg(Color::DarkGray)),
            Span::raw(" ago  "),
            Span::styled(format!("{} msgs", s.message_count), Style::default().fg(Color::White)),
        ];
        let total_jobs = s.active_agents + s.active_bg_jobs;
        if total_jobs > 0 {
            info_spans.push(Span::raw("  "));
            info_spans.push(Span::styled(
                format!("{} jobs", total_jobs),
                Style::default().fg(Color::Magenta),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(info_spans)), rows[0]);

        // Row 2: cost breakdown
        let cost = s.cost.estimated_cost_usd(s.model.as_deref());
        let cost_line = Line::from(vec![
            Span::raw(" Cost: "),
            Span::styled(format!("${:.2}", cost), Style::default().fg(Color::Yellow)),
            Span::raw(format!(
                "  (in:{} out:{})",
                format_tokens(s.cost.input_tokens + s.cost.cache_read_tokens + s.cost.cache_creation_tokens),
                format_tokens(s.cost.output_tokens),
            )),
        ]);
        f.render_widget(Paragraph::new(cost_line), rows[1]);

        // Row 3: context gauge + last message
        let ctx_pct = s.context_usage.percentage();
        let ctx_color = if ctx_pct > 80.0 {
            Color::Red
        } else if ctx_pct > 50.0 {
            Color::Yellow
        } else {
            Color::Green
        };
        let last_msg = s.last_message_preview.as_deref().unwrap_or("");
        let ctx_line = Line::from(vec![
            Span::raw(" Ctx: "),
            Span::styled(
                format!(
                    "{} / {} ({:.0}%)",
                    format_tokens(s.context_usage.current_tokens),
                    format_tokens(s.context_usage.max_tokens),
                    ctx_pct
                ),
                Style::default().fg(ctx_color),
            ),
            Span::raw("  "),
            Span::styled(last_msg, Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(ctx_line), rows[2]);
    }
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = if app.leader_active {
        vec![
            Span::styled(" SPACE > ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("n", Style::default().fg(Color::Yellow)),
            Span::raw(":new  "),
            Span::styled("e", Style::default().fg(Color::Yellow)),
            Span::raw(":ephemeral  "),
            Span::styled("x", Style::default().fg(Color::Yellow)),
            Span::raw(":close  "),
            Span::styled("r", Style::default().fg(Color::Yellow)),
            Span::raw(":refresh  "),
            Span::styled("l", Style::default().fg(Color::Yellow)),
            Span::raw(":logs  "),
            Span::styled("c", Style::default().fg(Color::Yellow)),
            Span::raw(":config"),
        ]
    } else {
        let enter_label = if app.selected_session().is_some_and(|s| s.status == SessionStatus::Dead) {
            ":revive  "
        } else {
            ":focus  "
        };
        let mut v = vec![
            Span::styled(" enter", Style::default().fg(Color::Yellow)),
            Span::raw(enter_label),
            Span::styled("s/S", Style::default().fg(Color::Yellow)),
            Span::raw(":sort  "),
            Span::styled("o", Style::default().fg(Color::Yellow)),
            Span::raw(":order  "),
        ];
        if app.total_pages() > 1 {
            v.push(Span::styled("</>", Style::default().fg(Color::Yellow)));
            v.push(Span::raw(":page  "));
        }
        v.extend([
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(":search  "),
            Span::styled("t", Style::default().fg(Color::Yellow)),
            Span::raw(":terminated  "),
            Span::styled("l", Style::default().fg(Color::Yellow)),
            Span::raw(":logs  "),
            Span::styled("space", Style::default().fg(Color::Yellow)),
            Span::raw(":menu  "),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(":quit"),
        ]);
        v
    };

    if let Some(msg) = &app.status_message {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(msg, Style::default().fg(Color::Red)));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn format_duration(d: chrono::Duration) -> String {
    let total_secs = d.num_seconds();
    if total_secs < 60 {
        return format!("{}s", total_secs);
    }
    if total_secs < 3600 {
        return format!("{}m", total_secs / 60);
    }

    let days = total_secs / 86400;
    if days == 0 {
        let h = total_secs / 3600;
        let m = (total_secs % 3600) / 60;
        return if m == 0 { format!("{}h", h) } else { format!("{}h{}m", h, m) };
    }

    let years = days / 365;
    let months = (days % 365) / 30;
    let remaining_days = (days % 365) % 30;

    let mut parts = Vec::new();
    if years > 0 { parts.push(format!("{}y", years)); }
    if months > 0 { parts.push(format!("{}mo", months)); }
    if remaining_days > 0 { parts.push(format!("{}d", remaining_days)); }
    if parts.is_empty() { parts.push("0d".to_string()); }
    parts.join("")
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
}

fn draw_picker(f: &mut Frame, picker: &crate::tui::app::DirPicker) {
    let area = centered_rect(60, 70, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" New Session - Pick Directory ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // search input
            Constraint::Min(3),   // results list
            Constraint::Length(1), // help
        ])
        .split(inner);

    // Search input
    let input_text = if picker.query.is_empty() {
        " Type to filter...".to_string()
    } else {
        format!(" {}", &picker.query)
    };
    let input_style = if picker.query.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    let input = Paragraph::new(input_text)
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(format!(" Search ({} dirs) ", picker.filtered.len())),
        );
    f.render_widget(input, chunks[0]);
    f.set_cursor_position((
        chunks[0].x + 1 + picker.query.len() as u16,
        chunks[0].y + 1,
    ));

    // Results list
    let visible_height = chunks[1].height.saturating_sub(2) as usize;
    let scroll_offset = if picker.selected >= visible_height {
        picker.selected - visible_height + 1
    } else {
        0
    };

    let rows: Vec<Row> = picker
        .filtered
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(i, path)| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let parent = path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();

            let style = if i == picker.selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let parent_color = if i == picker.selected { Color::White } else { Color::DarkGray };
            Row::new(vec![
                Cell::from(Span::styled(name, style.fg(Color::Cyan))),
                Cell::from(Span::styled(parent, style.fg(parent_color))),
            ])
            .style(style)
        })
        .collect();

    let widths = [Constraint::Min(20), Constraint::Min(30)];
    let table = Table::new(rows, widths).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Directories "),
    );
    f.render_widget(table, chunks[1]);

    // Help
    let help = Line::from(vec![
        Span::styled(" Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" launch  "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" cancel  "),
        Span::styled("Up/Down", Style::default().fg(Color::Yellow)),
        Span::raw(" select"),
    ]);
    f.render_widget(Paragraph::new(help), chunks[2]);
}

fn draw_log_viewer(f: &mut Frame, app: &App) {
    use crate::tui::app::LogLevel;

    let area = centered_rect(80, 75, f.area());
    f.render_widget(Clear, area);

    let entries = app.logs.entries();
    let viewer = app.log_viewer.as_ref().unwrap();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(" Logs ({}) ", entries.len()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let visible_height = chunks[0].height as usize;

    let total = entries.len();
    let scroll_offset = if viewer.scroll >= visible_height {
        (viewer.scroll - visible_height + 1).min(total.saturating_sub(visible_height))
    } else {
        0
    };

    let lines: Vec<Line> = entries
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(i, e)| {
            let selected = i == viewer.scroll;
            let level_span = match e.level {
                LogLevel::Info => Span::styled(" INFO ", Style::default().fg(Color::Green)),
                LogLevel::Warn => Span::styled(" WARN ", Style::default().fg(Color::Yellow)),
                LogLevel::Error => Span::styled(" ERR  ", Style::default().fg(Color::Red)),
            };
            let mut line = Line::from(vec![
                if selected {
                    Span::styled(" > ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                } else {
                    Span::raw("   ")
                },
                Span::styled(&e.timestamp, Style::default().fg(Color::DarkGray)),
                level_span,
                Span::raw(&e.message),
            ]);
            if selected {
                line = line.style(Style::default().bg(Color::DarkGray));
            }
            line
        })
        .collect();

    f.render_widget(Paragraph::new(lines), chunks[0]);

    let copied_hint = if viewer.copied {
        Span::styled("  Copied!", Style::default().fg(Color::Green))
    } else {
        Span::raw("")
    };

    let help = Line::from(vec![
        Span::styled(" l/Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" close  "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(" navigate  "),
        Span::styled("y", Style::default().fg(Color::Yellow)),
        Span::raw(" copy  "),
        Span::styled("g/G", Style::default().fg(Color::Yellow)),
        Span::raw(" top/bottom"),
        copied_hint,
    ]);
    f.render_widget(Paragraph::new(help), chunks[1]);
}

fn draw_config_editor(f: &mut Frame, ce: &crate::tui::app::ConfigEditor) {
    let area = centered_rect(60, 50, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Settings ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1), // error or help
        ])
        .split(inner);

    // Fields
    let rows: Vec<Row> = ce
        .fields
        .iter()
        .enumerate()
        .map(|(i, (key, label, value))| {
            let selected = i == ce.selected;
            let editing = selected && ce.editing;
            let is_action = key.starts_with("_update");
            let is_readonly = key.starts_with("_");

            let is_toggle = *key == "view_mode";

            let display_value = if is_action {
                if ce.updating && selected {
                    "Updating...".to_string()
                } else {
                    String::new()
                }
            } else if is_toggle {
                // Show as toggle: [compact] / detailed  or  compact / [detailed]
                let options = ["compact", "detailed"];
                options
                    .iter()
                    .map(|opt| {
                        if *opt == value.as_str() {
                            format!("[{}]", opt)
                        } else {
                            opt.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" / ")
            } else if editing {
                format!("{}_", &ce.edit_buf)
            } else {
                value.clone()
            };

            let label_style = if is_action && selected {
                Style::default().fg(Color::Green).bg(Color::DarkGray).add_modifier(Modifier::BOLD)
            } else if selected {
                Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
            } else if is_action {
                Style::default().fg(Color::Green)
            } else if is_readonly {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };

            let value_style = if editing {
                Style::default().fg(Color::Yellow).bg(Color::DarkGray)
            } else if is_readonly && !is_action {
                Style::default().fg(Color::DarkGray)
            } else if selected {
                Style::default().fg(Color::Cyan).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Cyan)
            };

            Row::new(vec![
                Cell::from(Span::styled(format!("  {}", label), label_style)),
                Cell::from(Span::styled(display_value, value_style)),
            ])
        })
        .collect();

    let widths = [Constraint::Min(25), Constraint::Min(30)];
    let table = Table::new(rows, widths);
    f.render_widget(table, chunks[0]);

    let help_line = if let Some(err) = &ce.error {
        Line::from(Span::styled(format!(" {}", err), Style::default().fg(Color::Red)))
    } else if let Some(msg) = &ce.success {
        Line::from(Span::styled(format!(" {}", msg), Style::default().fg(Color::Green)))
    } else if ce.editing {
        Line::from(vec![
            Span::styled(" Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" cancel"),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" edit  "),
            Span::styled("j/k", Style::default().fg(Color::Yellow)),
            Span::raw(" navigate  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" close"),
        ])
    };
    f.render_widget(Paragraph::new(help_line), chunks[1]);

    // Cursor for edit mode
    if ce.editing {
        let row_y = chunks[0].y + ce.selected as u16;
        let label_width = 25u16;
        f.set_cursor_position((
            chunks[0].x + label_width + ce.edit_buf.len() as u16,
            row_y,
        ));
    }
}

fn draw_focus_picker(f: &mut Frame, fp: &crate::tui::app::FocusPicker) {
    let height = (fp.candidates.len() as u16 + 4).min(12);
    let area = centered_rect(50, height * 100 / f.area().height.max(1), f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(" Focus {} -- pick terminal ", fp.project_name));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let lines: Vec<Line> = fp
        .candidates
        .iter()
        .enumerate()
        .map(|(i, (_id, name))| {
            Line::from(vec![
                Span::styled(
                    format!("  {} ", i + 1),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::raw(name),
            ])
        })
        .collect();

    f.render_widget(Paragraph::new(lines), chunks[0]);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" 1-9", Style::default().fg(Color::Yellow)),
            Span::raw(":pick  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(":cancel"),
        ])),
        chunks[1],
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}
