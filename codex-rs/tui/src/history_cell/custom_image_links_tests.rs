use super::*;
use crate::history_cell::HistoryCell;
use crate::history_cell::new_user_prompt;
use crate::history_cell::new_view_image_tool_call;
use crate::terminal_hyperlinks::visible_lines;
use crate::transcript_view::TranscriptView;
use crate::transcript_view::ViewAction;
use codex_protocol::user_input::TextElement;
use codex_utils_path_uri::LegacyAppPathString;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Text;
use std::path::PathBuf;
use unicode_segmentation::UnicodeSegmentation;

fn fixture_image(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    image::RgbaImage::new(/*width*/ 1, /*height*/ 1)
        .save(&path)
        .unwrap();
    path
}

fn linked_text(lines: &[HyperlinkLine], url: &Url) -> String {
    let mut text = String::new();
    for line in lines {
        let mut column = 0;
        for grapheme in line.line.to_string().graphemes(/*is_extended*/ true) {
            if line.hyperlinks.iter().any(|link| {
                link.columns.contains(&column)
                    && link.terminal_destination().as_deref() == Some(url.as_str())
            }) {
                text.push_str(grapheme);
            }
            column += display_width(grapheme);
        }
    }
    text
}

#[test]
fn viewed_image_links_preserve_rendering_and_encoded_paths() {
    let temp = tempfile::tempdir().unwrap();
    let path = fixture_image(temp.path(), "土地 图#50%.png");
    let url = image_url(&path).unwrap();
    assert_eq!(
        url.to_file_path().unwrap().canonicalize().unwrap(),
        path.canonicalize().unwrap()
    );
    let cell = new_view_image_tool_call(LegacyAppPathString::from_string(path.to_string_lossy()));
    let mut snapshots = Vec::new();
    for width in [0, 8, 22, 80] {
        let compact = cell.display_hyperlink_lines(width);
        let detailed = cell.transcript_hyperlink_lines(width);
        assert_eq!(visible_lines(compact.clone()), cell.display_lines(width));
        assert_eq!(
            visible_lines(detailed.clone()),
            cell.transcript_lines(width)
        );
        if width == 80 {
            assert_eq!(linked_text(&compact, &url), "土地 图#50%.png");
        }
        if width > 0 {
            assert!(linked_text(&detailed, &url).ends_with("图#50%.png"));
        }
        snapshots.push(format!(
            "width {width}\n{}\nlinked: {}",
            Text::from(visible_lines(compact.clone())),
            linked_text(&compact, &url)
        ));
    }
    insta::assert_snapshot!(snapshots.join("\n\n"));
}

#[test]
fn user_image_links_follow_elements_across_unicode_and_wrapping() {
    let temp = tempfile::tempdir().unwrap();
    let first = fixture_image(temp.path(), "first.png");
    let second = fixture_image(temp.path(), "second.png");
    let message =
        "同一行 [Image #2] and [Image #3]\n同一行 [Image #2] literal\nhttps://example.com/";
    let elements = ["[Image #2]", "[Image #3]"].map(|label| {
        let start = message.find(label).unwrap();
        TextElement::new((start..start + label.len()).into(), Some(label.into()))
    });
    let cell = new_user_prompt(
        message.into(),
        elements.into(),
        vec![first.clone(), second.clone()],
        vec!["https://example.com/remote.png".into()],
    );
    let mut snapshots = Vec::new();
    for width in [12, 24, 80] {
        let lines = cell.display_hyperlink_lines(width);
        assert_eq!(cell.transcript_hyperlink_lines(width), lines);
        assert_eq!(
            linked_text(&lines, &image_url(&first).unwrap()).replace(' ', ""),
            "[Image#2]"
        );
        assert_eq!(
            linked_text(&lines, &image_url(&second).unwrap()).replace(' ', ""),
            "[Image#3]"
        );
        assert_eq!(
            linked_text(&lines, &Url::parse("https://example.com/").unwrap()),
            "https://example.com/"
        );
        snapshots.push(format!(
            "width {width}\n{}",
            Text::from(visible_lines(lines))
        ));
    }
    insta::assert_snapshot!(snapshots.join("\n\n"));
}

#[test]
fn cli_image_only_links_include_split_placeholders() {
    let temp = tempfile::tempdir().unwrap();
    let first = fixture_image(temp.path(), "first.png");
    let second = fixture_image(temp.path(), "second.png");
    let cell = new_user_prompt(
        String::new(),
        Vec::new(),
        vec![first.clone(), second.clone()],
        Vec::new(),
    );
    for width in [6, 12, 40] {
        let lines = cell.display_hyperlink_lines(width);
        for (path, label) in [(&first, "[Image#1]"), (&second, "[Image#2]")] {
            assert_eq!(
                linked_text(&lines, &image_url(path).unwrap()).replace(' ', ""),
                label
            );
        }
    }
}

#[test]
fn absent_images_and_plain_placeholder_text_stay_unlinked() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing.png");
    let cell =
        new_view_image_tool_call(LegacyAppPathString::from_string(missing.to_string_lossy()));
    assert!(
        cell.display_hyperlink_lines(/*width*/ 80)
            .iter()
            .all(|line| line.hyperlinks.is_empty())
    );
    let path = fixture_image(temp.path(), "present.png");
    let message = "[Image #1]";
    let cell = new_user_prompt(message.into(), Vec::new(), vec![path.clone()], Vec::new());
    // Only the automatically added attachment row is linked, not literal prompt text.
    assert_eq!(
        linked_text(
            &cell.display_hyperlink_lines(/*width*/ 80),
            &image_url(&path).unwrap()
        ),
        message
    );
    let cell = new_user_prompt(
        message.into(),
        vec![TextElement::new(
            (0..message.len()).into(),
            Some(message.into()),
        )],
        vec![missing],
        Vec::new(),
    );
    assert!(
        cell.display_hyperlink_lines(/*width*/ 80)
            .iter()
            .all(|line| line.hyperlinks.is_empty())
    );
}

#[test]
fn image_clicks_reuse_transcript_open_link_and_survive_resize() {
    let temp = tempfile::tempdir().unwrap();
    let path = fixture_image(temp.path(), "click.png");
    let url = image_url(&path).unwrap();
    let message = "Look [Image #1]";
    let cells: Vec<Arc<dyn HistoryCell>> = vec![
        Arc::new(new_user_prompt(
            message.into(),
            vec![TextElement::new(
                (5..message.len()).into(),
                Some("[Image #1]".into()),
            )],
            vec![path.clone()],
            Vec::new(),
        )),
        Arc::new(new_view_image_tool_call(LegacyAppPathString::from_string(
            path.to_string_lossy(),
        ))),
    ];
    let mut view = TranscriptView::default();
    for width in [80, 20, 80] {
        let area = Rect::new(/*x*/ 0, /*y*/ 0, width, /*height*/ 30);
        view.render(area, &mut Buffer::empty(area), &cells);
        let targets = (0..area.height)
            .flat_map(|row| (0..width).map(move |column| (column, row)))
            .filter(|(column, row)| view.link_at(*column, *row).as_deref() == Some(url.as_str()))
            .collect::<Vec<_>>();
        assert!(targets.len() >= "[Image #1]".len());
        if width == 20 {
            let (column, row) = targets[0];
            let event = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            };
            assert!(!matches!(
                view.handle_mouse(event, &cells),
                Some(ViewAction::OpenLink(_))
            ));
            let action = view.handle_mouse(
                MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    ..event
                },
                &cells,
            );
            assert!(
                matches!(action, Some(ViewAction::OpenLink(destination)) if destination == url.as_str())
            );
        }
        for (column, row) in targets {
            let action = view.handle_mouse(
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    modifiers: KeyModifiers::CONTROL,
                },
                &cells,
            );
            assert!(
                matches!(action, Some(ViewAction::OpenLink(destination)) if destination == url.as_str())
            );
        }
    }
}
