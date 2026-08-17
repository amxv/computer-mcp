use std::fmt::Write as _;

use anyhow::{Context, Result};

use crate::local::HistoryFormat;

use super::model::{
    PresentationDocument, PresentationFileOperation, PresentationKind, PresentationRecord,
    PresentationWriteMode,
};
use super::sanitize::markdown_code_span;

pub fn render_presentation(
    document: &PresentationDocument,
    format: HistoryFormat,
) -> Result<String> {
    match format {
        HistoryFormat::Json => serde_json::to_string_pretty(document)
            .context("failed to render Local presentation JSON"),
        HistoryFormat::Markdown => Ok(render_markdown(document)),
    }
}

fn render_markdown(document: &PresentationDocument) -> String {
    if document.records.is_empty() {
        return "No Local history.\n".to_string();
    }

    let mut output = String::new();
    if !document.agents.is_empty() {
        for agent in &document.agents {
            let workdirs = agent
                .workdirs
                .iter()
                .map(|workdir| markdown_code_span(&workdir.normalized_workdir))
                .collect::<Vec<_>>()
                .join(" → ");
            let _ = if workdirs.is_empty() {
                writeln!(output, "- Agent `{}`", agent.id)
            } else {
                writeln!(output, "- Agent `{}` · {workdirs}", agent.id)
            };
        }
        output.push('\n');
    }

    for record in &document.records {
        render_record(&mut output, record);
    }
    output
}

fn render_record(output: &mut String, record: &PresentationRecord) {
    let id = if record.primary_invocation_id > 0 {
        format!("#{}", record.primary_invocation_id)
    } else {
        "#?".to_string()
    };
    let agent = record.agent_id.as_deref().unwrap_or("----");
    let workdir = record
        .normalized_workdir
        .as_deref()
        .or(record.declared_workdir.as_deref())
        .map(|workdir| format!(" · {}", markdown_code_span(workdir)))
        .unwrap_or_default();
    let new_workdir = record
        .new_workdir
        .as_deref()
        .map(|workdir| format!(" · new workdir {}", markdown_code_span(workdir)))
        .unwrap_or_default();
    let degraded = if record.evidence.degraded {
        " · ⚠ incomplete evidence"
    } else {
        ""
    };

    match &record.kind {
        PresentationKind::Command {
            command,
            status,
            effective_cwd,
            exit_code,
            termination_reason,
            output: command_output,
            output_truncated,
            polls,
        } => {
            let exit = exit_code
                .map(|code| format!(" · exit {code}"))
                .unwrap_or_default();
            let reason = termination_reason
                .as_deref()
                .map(|reason| format!(" · {reason}"))
                .unwrap_or_default();
            let cwd = effective_cwd
                .as_deref()
                .filter(|cwd| Some(*cwd) != record.normalized_workdir.as_deref())
                .map(|cwd| format!(" · cwd {}", markdown_code_span(cwd)))
                .unwrap_or_default();
            let _ = writeln!(
                output,
                "- `{id}` `{agent}` {} — {status}{exit}{reason}{workdir}{cwd}{new_workdir}{degraded}",
                markdown_code_span(&format!("$ {command}"))
            );
            if let Some(polls) = polls {
                let cross = if polls.cross_agent {
                    " · cross-agent"
                } else {
                    ""
                };
                let final_status = polls
                    .final_status
                    .as_deref()
                    .map(|status| format!(" · {status}"))
                    .unwrap_or_default();
                let _ = writeln!(output, "  polled {}x{final_status}{cross}", polls.count);
            }
            if let Some(command_output) = command_output
                && !command_output.is_empty()
            {
                let suffix = if *output_truncated {
                    " [truncated]"
                } else {
                    ""
                };
                let _ = writeln!(output, "  output{suffix}:");
                write_indented(output, command_output, "    ");
            }
        }
        PresentationKind::FileChanges {
            source_tool,
            changes,
        } => {
            let _ = writeln!(
                output,
                "- `{id}` `{agent}` **{}**{workdir}{new_workdir}{degraded}",
                markdown_inline(source_tool)
            );
            for change in changes {
                let operation = match change.operation {
                    PresentationFileOperation::Created => "Created",
                    PresentationFileOperation::Edited => "Edited",
                    PresentationFileOperation::Deleted => "Deleted",
                    PresentationFileOperation::Renamed => "Renamed",
                };
                let write_mode = match change.write_mode {
                    Some(PresentationWriteMode::Overwrite) => " · overwrite",
                    Some(PresentationWriteMode::Append) => " · append",
                    None => "",
                };
                let old_path = change
                    .old_path
                    .as_deref()
                    .map(|path| format!(" from {}", markdown_code_span(path)))
                    .unwrap_or_default();
                let _ = writeln!(
                    output,
                    "  - {operation} {}{old_path} · +{} -{}{write_mode}",
                    markdown_code_span(&change.path),
                    change.added,
                    change.removed
                );
                for line in &change.lines {
                    let marker = match line.kind.as_str() {
                        "add" => '+',
                        "remove" => '-',
                        _ => ' ',
                    };
                    let old = line
                        .old_line
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                    let new = line
                        .new_line
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                    let _ = writeln!(output, "      {old:>5} {new:>5} {marker} {}", line.text);
                }
                if change.diff_truncated {
                    output.push_str("      … diff display truncated\n");
                }
            }
        }
        PresentationKind::Stdin {
            target_session_handle,
            chars,
            chars_truncated,
            creator_agent_id,
            cross_agent,
            result_status,
        } => {
            let relation = relation_suffix(creator_agent_id.as_deref(), *cross_agent);
            let status = result_status
                .as_deref()
                .map(|status| format!(" · {status}"))
                .unwrap_or_default();
            let suffix = if *chars_truncated {
                " · truncated"
            } else {
                ""
            };
            let _ = writeln!(
                output,
                "- `{id}` `{agent}` sent stdin → {}{status}{relation}{suffix}{degraded}",
                markdown_code_span(target_session_handle)
            );
            write_indented(output, chars, "    ");
        }
        PresentationKind::Kill {
            target_session_handle,
            creator_agent_id,
            cross_agent,
            result_status,
        } => {
            let relation = relation_suffix(creator_agent_id.as_deref(), *cross_agent);
            let status = result_status
                .as_deref()
                .map(|status| format!(" · {status}"))
                .unwrap_or_default();
            let _ = writeln!(
                output,
                "- `{id}` `{agent}` killed {}{status}{relation}{degraded}",
                markdown_code_span(target_session_handle)
            );
        }
        PresentationKind::PollAggregate {
            target_session_handle,
            count,
            final_status,
            creator_agent_id,
            cross_agent,
            ..
        } => {
            let relation = relation_suffix(creator_agent_id.as_deref(), *cross_agent);
            let status = final_status
                .as_deref()
                .map(|status| format!(" · {status}"))
                .unwrap_or_default();
            let _ = writeln!(
                output,
                "- `{id}` `{agent}` polled {count}x → {}{status}{relation}{degraded}",
                markdown_code_span(target_session_handle)
            );
        }
        PresentationKind::Generic {
            tool_name,
            status,
            summary,
        } => {
            let _ = writeln!(
                output,
                "- `{id}` `{agent}` **{}** — {status}{workdir}{new_workdir}{degraded}",
                markdown_inline(tool_name)
            );
            if let Some(summary) = summary {
                write_indented(output, summary, "    ");
            }
        }
    }

    if record.evidence.degraded
        && let Some(reason) = record.evidence.reason.as_deref()
    {
        let _ = writeln!(output, "  evidence: {}", markdown_inline(reason));
    }
}

fn relation_suffix(creator_agent_id: Option<&str>, cross_agent: bool) -> String {
    if !cross_agent {
        return String::new();
    }
    creator_agent_id
        .map(|creator| format!(" · cross-agent · creator `{creator}`"))
        .unwrap_or_else(|| " · cross-agent".to_string())
}

fn write_indented(output: &mut String, value: &str, prefix: &str) {
    if value.is_empty() {
        let _ = writeln!(output, "{prefix}<empty>");
        return;
    }
    for line in value.split('\n') {
        let _ = writeln!(output, "{prefix}{line}");
    }
}

fn markdown_inline(value: &str) -> String {
    value.replace('\n', " ")
}
