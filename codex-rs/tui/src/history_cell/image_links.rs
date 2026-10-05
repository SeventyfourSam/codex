//! Local image links added after upstream rendering, without changing visible or model text.
//! Browser opening and mouse gestures use the existing transcript hyperlink path.

use super::UserHistoryCell;
use super::messages::sanitize_user_text;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::TerminalHyperlink;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use crate::ui_consts::LIVE_PREFIX_COLS;
use crate::width::display_width;
use crate::wrapping::RtOptions;
use crate::wrapping::adaptive_wrap_lines;
use codex_protocol::models::local_image_label_text;
use ratatui::style::Modifier;
use ratatui::text::Line;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use url::Url;

fn image_url(path: &Path) -> Option<Url> {
    image::ImageFormat::from_path(path).ok()?;
    let path = path.canonicalize().ok()?;
    path.is_file()
        .then(|| Url::from_file_path(path).ok())
        .flatten()
}

fn link(line: &mut HyperlinkLine, columns: Range<usize>, url: &Url) {
    if columns.is_empty() {
        return;
    }
    // Only actual image attachments and image-view events reach this capability.
    // Ordinary user text and Markdown still cannot promote arbitrary file URLs.
    let mut link = TerminalHyperlink::web(columns, url.to_string());
    link.retarget_to_trusted_file(url);
    line.hyperlinks.push(link);
}

pub(super) fn viewed_image(lines: Vec<Line<'static>>, path: &str) -> Vec<HyperlinkLine> {
    let mut lines = plain_hyperlink_lines(lines);
    let path = if path.starts_with("file://") {
        Url::parse(path)
            .ok()
            .and_then(|url| url.to_file_path().ok())
    } else {
        Some(Path::new(path).to_path_buf())
    };
    let Some(url) = path.as_deref().and_then(image_url) else {
        return lines;
    };
    for line in &mut lines {
        // Upstream dims the path and gutter, but makes the heading bold. Preserve
        // its truncation/wrapping, excluding the two-column gutter from the link.
        let mut column = 0;
        let ranges = line
            .line
            .spans
            .iter()
            .filter_map(|span| {
                let start = column;
                column += display_width(&span.content);
                (span.style.add_modifier.contains(Modifier::DIM)
                    && !span.style.add_modifier.contains(Modifier::BOLD))
                .then_some(start.max(/*other*/ 2)..column)
            })
            .collect::<Vec<_>>();
        for range in ranges {
            link(line, range, &url);
        }
    }
    lines
}

pub(super) fn user_images(cell: &UserHistoryCell, lines: &mut [HyperlinkLine], width: u16) {
    let targets = cell
        .local_image_paths
        .iter()
        .enumerate()
        .filter_map(|(index, path)| {
            let label = local_image_label_text(cell.remote_image_urls.len() + index + 1);
            image_url(path).map(|url| (label, url))
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return;
    }

    // CLI image arguments may have no inline placeholder. These leading rows
    // were wrapped before receiving source metadata; track their row counts so
    // even a placeholder split across narrow rows links to the correct image.
    let wrap_width = width.saturating_sub(LIVE_PREFIX_COLS + 1).max(/*other*/ 1);
    let mut row = 1;
    for index in 1..=cell.remote_image_urls.len() + cell.local_image_paths.len() {
        let label = local_image_label_text(index);
        if cell
            .text_elements
            .iter()
            .any(|element| element.placeholder(&cell.message) == Some(label.as_str()))
        {
            continue;
        }
        let fragments = adaptive_wrap_lines(
            [Line::from(label.clone())],
            RtOptions::new(usize::from(wrap_width))
                .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit),
        );
        for _ in fragments {
            if let Some(line) = lines.get_mut(row)
                && let Some((_, url)) = targets.iter().find(|(candidate, _)| candidate == &label)
            {
                link(line, 2..line.width(), url);
            }
            row += 1;
        }
    }

    // Match the upstream renderer's decision to discard element byte offsets
    // when sanitizing text or trimming a spoken message changes its contents.
    if sanitize_user_text(cell.message.as_str().into()) != cell.message
        || cell.spoken && cell.message.trim_start() != cell.message
    {
        return;
    }
    let elements = cell
        .text_elements
        .iter()
        .filter_map(|element| {
            let range = element.byte_range.start..element.byte_range.end;
            let text = cell.message.get(range.clone())?;
            targets
                .iter()
                .find(|(label, _)| label == text)
                .map(|(_, url)| (range, url))
        })
        .collect::<Vec<_>>();
    let mut previous: Option<Arc<str>> = None;
    let mut offset = 0;
    for line in lines.iter_mut().skip(row) {
        let Some(source) = line.source.clone() else {
            continue;
        };
        if let Some(previous) = &previous
            && !Arc::ptr_eq(previous, &source.text)
        {
            offset += previous.len() + 1;
        }
        previous = Some(Arc::clone(&source.text));
        for (range, url) in &elements {
            let start = range.start.max(offset + source.range.start);
            let end = range.end.min(offset + source.range.end);
            if start < end {
                let text = line.line.to_string();
                let prefix = display_width(&text[..source.prefix_bytes]);
                let start_column =
                    prefix + display_width(&source.text[source.range.start..start - offset]);
                let end_column =
                    prefix + display_width(&source.text[source.range.start..end - offset]);
                link(line, start_column..end_column, url);
            }
        }
    }
}

#[cfg(test)]
#[path = "image_links_tests.rs"]
mod tests;
