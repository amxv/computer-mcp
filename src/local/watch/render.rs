use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use time::OffsetDateTime;

use super::super::presentation::sanitize_display_text;
use super::super::{
    PresentationFileOperation, PresentationKind, PresentationRecord, PresentationWriteMode,
};
use super::model::{ConnectionState, WatchApp, WatchCard, WatchScope};

const MAX_HEADER_WORKDIRS: usize = 3;
const MAX_COMPACT_OUTPUT_LINES: usize = 3;
const MAX_EXPANDED_LINES: usize = 300;

pub(super) fn render(frame: &mut Frame<'_>, app: &WatchApp) {
    let areas = Layout::vertical([
        Constraint::Length(header_height(app)),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(frame.area());
    render_header(frame, areas[0], app);
    render_body(frame, areas[1], app);
    render_footer(frame, areas[2], app);
}

fn header_height(app: &WatchApp) -> u16 {
    let mut lines = 2;
    if !header_workdirs(app).is_empty() {
        lines += 1;
    }
    if app.new_workdir_notice().is_some() {
        lines += 1;
    }
    if app.recovery_notice().is_some() {
        lines += 1;
    }
    if matches!(app.connection, ConnectionState::Degraded(_)) {
        lines += 1;
    }
    lines + 2
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &WatchApp) {
    let connection = match &app.connection {
        ConnectionState::Connecting => {
            Span::styled("connecting", Style::default().fg(Color::Yellow))
        }
        ConnectionState::Connected => Span::styled("connected", Style::default().fg(Color::Green)),
        ConnectionState::Degraded(_) => Span::styled("degraded", Style::default().fg(Color::Red)),
    };
    let scope = match &app.scope {
        WatchScope::Waiting => "Waiting for Agent".to_owned(),
        WatchScope::Picker => "Choose Agent".to_owned(),
        WatchScope::Agent(id) => format!("Agent {}", sanitize_display_text(id)),
        WatchScope::All => "All Agents".to_owned(),
        WatchScope::Unattributed => "Unattributed".to_owned(),
    };
    let process_count = app
        .current_agent()
        .map(|agent| agent.active_process_count)
        .unwrap_or(app.status_active_process_count);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Zodex Local", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  ·  "),
            connection,
            Span::raw("  ·  "),
            Span::raw(scope),
        ]),
        Line::from(format!(
            "{}  ·  processes: {}  ·  runtime {}",
            app.ttl_label(OffsetDateTime::now_utc()),
            process_count,
            short_id(&app.runtime_id)
        )),
    ];
    let workdirs = header_workdirs(app);
    if !workdirs.is_empty() {
        lines.push(Line::from(format!("workdirs: {}", workdirs.join("  ·  "))));
    }
    if let Some(path) = app.new_workdir_notice() {
        lines.push(Line::from(Span::styled(
            format!("new workdir: {path}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
    }
    if let Some(message) = app.recovery_notice() {
        lines.push(Line::from(Span::styled(
            message.to_owned(),
            Style::default().fg(Color::Yellow),
        )));
    }
    if let ConnectionState::Degraded(message) = &app.connection {
        lines.push(Line::from(Span::styled(
            message.clone(),
            Style::default().fg(Color::Red),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" watch "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_body(frame: &mut Frame<'_>, area: Rect, app: &WatchApp) {
    match &app.scope {
        WatchScope::Waiting => render_message(
            frame,
            area,
            "Waiting for the first current-runtime Agent. Live activity will appear from now; completed history is not preloaded.",
        ),
        WatchScope::Unattributed if app.visible_cards().is_empty() => render_message(
            frame,
            area,
            "Unattributed Local activity was observed. No Agent identity is being inferred. Press a to inspect current Agents when they appear.",
        ),
        WatchScope::Agent(id)
            if app.current_agent().is_none() && app.visible_cards().is_empty() =>
        {
            render_message(
                frame,
                area,
                &format!(
                    "Waiting for Agent {id} in the current Local runtime. Live activity will appear from now; completed history is not preloaded."
                ),
            )
        }
        WatchScope::Picker => render_picker(frame, area, app),
        _ => render_timeline(frame, area, app),
    }
}

fn render_message(frame: &mut Frame<'_>, area: Rect, message: &str) {
    frame.render_widget(
        Paragraph::new(message)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_picker(frame: &mut Frame<'_>, area: Rect, app: &WatchApp) {
    let mut items = Vec::with_capacity(app.agents.len() + 1);
    items.push(ListItem::new("All Agents"));
    items.extend(app.agents.iter().map(|agent| {
        let workdir = agent
            .workdirs
            .last()
            .map(|workdir| sanitize_display_text(&workdir.normalized_workdir))
            .unwrap_or_else(|| "no declared workdir".to_owned());
        ListItem::new(format!(
            "{}  ·  {} process(es)  ·  {}",
            sanitize_display_text(&agent.id),
            agent.active_process_count,
            workdir
        ))
    }));
    let mut state = ListState::default().with_selected(Some(app.picker_index));
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Agent picker "),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD))
            .highlight_symbol("› "),
        area,
        &mut state,
    );
}

fn render_timeline(frame: &mut Frame<'_>, area: Rect, app: &WatchApp) {
    let visible = app.visible_cards();
    if visible.is_empty() {
        let message = if app.search_query.is_empty() {
            "No live activity since this viewer attached."
        } else {
            "No live activity matches the current filter."
        };
        render_message(frame, area, message);
        return;
    }
    let selected = visible.get(app.selected).copied();
    if selected.is_some_and(|card| app.is_expanded(card) || app.raw_is_open(card)) {
        render_detail(frame, area, app, selected.unwrap());
        return;
    }

    let items = visible
        .iter()
        .map(|card| ListItem::new(compact_card_lines(app, card)))
        .collect::<Vec<_>>();
    let mut state = ListState::default()
        .with_selected(Some(app.selected))
        .with_offset(app.scroll as usize);
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" live "))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("› "),
        area,
        &mut state,
    );
}

fn render_detail(frame: &mut Frame<'_>, area: Rect, app: &WatchApp, card: &WatchCard) {
    let mut lines = expanded_card_lines(app, card);
    if app.raw_is_open(card) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "RAW LOGICAL EVIDENCE",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        if let Some(raw) = app.raw_display_for(card) {
            lines.extend(
                raw.lines()
                    .take(MAX_EXPANDED_LINES)
                    .map(|line| Line::from(line.to_owned())),
            );
        }
    }
    let max_scroll = lines
        .len()
        .saturating_sub(area.height.saturating_sub(2) as usize) as u16;
    let scroll = app.scroll.min(max_scroll);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title(" detail "))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}

fn compact_card_lines(app: &WatchApp, card: &WatchCard) -> Vec<Line<'static>> {
    let record = &card.record;
    let prefix = if matches!(app.scope, WatchScope::All) {
        format!(
            "[{}] ",
            record.agent_id.as_deref().unwrap_or("Unattributed")
        )
    } else {
        String::new()
    };
    match &record.kind {
        PresentationKind::Command {
            command,
            status,
            effective_cwd,
            exit_code,
            output,
            polls,
            ..
        } => {
            let status_mark = status_mark(status, *exit_code);
            let mut lines = vec![Line::from(format!("{prefix}{status_mark} $ {command}"))];
            append_cwd_line(&mut lines, record, effective_cwd.as_deref());
            let display_output = app
                .live_output_for(card)
                .map(|(text, _)| text)
                .or(output.as_deref());
            if let Some(output) = display_output {
                lines.extend(
                    output
                        .lines()
                        .filter(|line| !line.is_empty())
                        .take(MAX_COMPACT_OUTPUT_LINES)
                        .map(|line| Line::from(format!("  {line}"))),
                );
            } else if status == "running" {
                lines.push(Line::from("  Running…"));
            }
            if let Some(polls) = polls {
                lines.push(Line::from(format!(
                    "  polls ×{}{}",
                    polls.count,
                    if polls.cross_agent {
                        " · cross-Agent"
                    } else {
                        ""
                    }
                )));
            }
            lines
        }
        PresentationKind::FileChanges { changes, .. } => changes
            .iter()
            .map(|change| {
                Line::from(format!(
                    "{prefix}{} {}  +{} -{}{}",
                    operation_label(change.operation),
                    change.path,
                    change.added,
                    change.removed,
                    change
                        .write_mode
                        .map(write_mode_label)
                        .map(|mode| format!(" · {mode}"))
                        .unwrap_or_default()
                ))
            })
            .collect(),
        PresentationKind::Stdin {
            target_session_handle,
            chars,
            cross_agent,
            creator_agent_id,
            ..
        } => vec![Line::from(format!(
            "{prefix}→ stdin {target_session_handle}  {}{}{}",
            compact_text(chars, 80),
            if *cross_agent { " · cross-Agent" } else { "" },
            creator_agent_id
                .as_deref()
                .map(|id| format!(" · creator {id}"))
                .unwrap_or_default()
        ))],
        PresentationKind::Kill {
            target_session_handle,
            cross_agent,
            creator_agent_id,
            ..
        } => vec![Line::from(format!(
            "{prefix}■ kill {target_session_handle}{}{}",
            if *cross_agent { " · cross-Agent" } else { "" },
            creator_agent_id
                .as_deref()
                .map(|id| format!(" · creator {id}"))
                .unwrap_or_default()
        ))],
        PresentationKind::PollAggregate {
            target_session_handle,
            count,
            final_status,
            caller_agent_ids,
            cross_agent,
            ..
        } => vec![Line::from(format!(
            "{prefix}↻ poll {target_session_handle} ×{count} · {}{}{}",
            final_status.as_deref().unwrap_or("running"),
            if caller_agent_ids.is_empty() {
                String::new()
            } else {
                format!(" · callers {}", caller_agent_ids.join(","))
            },
            if *cross_agent { " · cross-Agent" } else { "" }
        ))],
        PresentationKind::Generic {
            tool_name,
            status,
            summary,
        } => vec![Line::from(format!(
            "{prefix}{} {tool_name} · {status}{}",
            if status == "running" { "…" } else { "•" },
            summary
                .as_deref()
                .map(|value| format!(" · {}", compact_text(value, 120)))
                .unwrap_or_default()
        ))],
    }
}

fn expanded_card_lines(app: &WatchApp, card: &WatchCard) -> Vec<Line<'static>> {
    let record = &card.record;
    let mut lines = compact_card_lines(app, card);
    match &record.kind {
        PresentationKind::Command { output, .. } => {
            if let Some((live, truncated)) = app.live_output_for(card) {
                if truncated {
                    lines.push(Line::from("… live output prefix omitted …"));
                }
                lines.extend(
                    live.lines()
                        .take(MAX_EXPANDED_LINES)
                        .map(|line| Line::from(line.to_owned())),
                );
            } else if let Some(output) = output {
                lines.extend(
                    output
                        .lines()
                        .skip(MAX_COMPACT_OUTPUT_LINES)
                        .take(MAX_EXPANDED_LINES)
                        .map(|line| Line::from(line.to_owned())),
                );
            }
        }
        PresentationKind::FileChanges { changes, .. } => {
            for change in changes {
                lines.push(Line::from(format!(
                    "--- {}{}",
                    change.path,
                    if change.diff_truncated {
                        " (diff truncated)"
                    } else {
                        ""
                    }
                )));
                lines.extend(change.lines.iter().take(MAX_EXPANDED_LINES).map(|line| {
                    let marker = match line.kind.as_str() {
                        "add" => "+",
                        "remove" => "-",
                        _ => " ",
                    };
                    Line::from(format!(
                        "{:>5} {:>5} {marker}{}",
                        line.old_line
                            .map(|value| value.to_string())
                            .unwrap_or_default(),
                        line.new_line
                            .map(|value| value.to_string())
                            .unwrap_or_default(),
                        line.text
                    ))
                }));
            }
        }
        _ => {}
    }
    if record.evidence.degraded {
        lines.push(Line::from(Span::styled(
            format!(
                "evidence degraded{}",
                record
                    .evidence
                    .reason
                    .as_deref()
                    .map(|reason| format!(": {reason}"))
                    .unwrap_or_default()
            ),
            Style::default().fg(Color::Yellow),
        )));
    }
    lines
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &WatchApp) {
    let line = if let Some(search) = app.search_input.as_deref() {
        format!(
            "/{}  ·  Enter apply  Esc cancel",
            sanitize_display_text(search)
        )
    } else if !app.search_query.is_empty() {
        format!(
            "filter: /{}  ·  j/k move  Enter expand  Tab agents  a picker  r raw  y copy  g/G ends  / search  q quit",
            sanitize_display_text(&app.search_query)
        )
    } else {
        "j/k move  Enter expand  Tab/Shift-Tab agents  a picker  r raw  y copy  g/G ends  / search  q quit".to_owned()
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn append_cwd_line(
    lines: &mut Vec<Line<'static>>,
    record: &PresentationRecord,
    effective: Option<&str>,
) {
    if let Some(workdir) = record.normalized_workdir.as_deref() {
        let suffix = effective
            .filter(|cwd| *cwd != workdir)
            .map(|cwd| format!(" → cwd {cwd}"))
            .unwrap_or_default();
        lines.push(Line::from(format!("  @ {workdir}{suffix}")));
    } else if let Some(effective) = effective {
        lines.push(Line::from(format!("  cwd {effective}")));
    }
}

fn header_workdirs(app: &WatchApp) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut all = Vec::new();
    let agents: Vec<_> = match &app.scope {
        WatchScope::Agent(id) => app.agents.iter().filter(|agent| &agent.id == id).collect(),
        WatchScope::All | WatchScope::Picker => app.agents.iter().collect(),
        WatchScope::Waiting | WatchScope::Unattributed => Vec::new(),
    };
    for agent in agents {
        for workdir in &agent.workdirs {
            if seen.insert(workdir.normalized_workdir.clone()) {
                all.push(sanitize_display_text(&workdir.normalized_workdir));
            }
        }
    }
    let hidden = all.len().saturating_sub(MAX_HEADER_WORKDIRS);
    all.truncate(MAX_HEADER_WORKDIRS);
    if hidden > 0 {
        all.push(format!("+{hidden} more"));
    }
    all
}

fn status_mark(status: &str, exit_code: Option<i64>) -> &'static str {
    if status == "running" {
        "…"
    } else if exit_code.is_some_and(|code| code != 0) || matches!(status, "error" | "failed") {
        "✗"
    } else {
        "✓"
    }
}

fn operation_label(operation: PresentationFileOperation) -> &'static str {
    match operation {
        PresentationFileOperation::Created => "Created",
        PresentationFileOperation::Edited => "Edited",
        PresentationFileOperation::Deleted => "Deleted",
        PresentationFileOperation::Renamed => "Renamed",
    }
}

fn write_mode_label(mode: PresentationWriteMode) -> &'static str {
    match mode {
        PresentationWriteMode::Overwrite => "overwrite",
        PresentationWriteMode::Append => "append",
    }
}

fn compact_text(value: &str, max_chars: usize) -> String {
    let flattened = value.replace('\n', " ↵ ");
    let mut chars = flattened.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn short_id(value: &str) -> &str {
    value.get(..value.len().min(8)).unwrap_or(value)
}
