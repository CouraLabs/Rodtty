use super::*;

/// The header's search field: one query over files, their contents and the
/// terminals, whose results show in the Search tab.
pub(crate) fn field(s: &State, a: &Appearance, width: f32) -> El {
    text_input(&s.explorer_search.query, ui_label())
        .key(QUERY_KEY)
        .role("searchbox")
        .label("Search")
        .help("Search files, file contents and terminals")
        .placeholder("Search…", a.muted())
        .single_line()
        .no_window_drag()
        .w(width)
        .min_w(0.0)
        .shrink()
        .px(8.0)
        .py(4.0)
        .color(a.ink())
        .fill(a.bg())
        .border(1.0, a.kit().c.border_base)
        .radius(6.0)
        .caret(a.accent(), a.selection())
        .on_edit(|s: &mut State| &mut s.explorer_search.query)
        .on_input_change(|s: &mut State, _| s.explorer_search_edited())
        .on_input_key(|s: &mut State, event, tx| s.explorer_search_key(event, tx))
}

/// A group's header: a disclosure triangle, then what the group is. Pressing
/// it shows or hides the group's rows.
pub(super) fn group_header(
    a: &Appearance,
    key: String,
    group: Group,
    collapsed: bool,
    rows: usize,
    help: String,
    content: El,
) -> El {
    let row_height = 24.0_f32.max(ui_label().size * 1.5 + 4.0);
    let name = format!(
        "{help}, {rows} {}",
        if rows == 1 { "result" } else { "results" }
    );
    row()
        .key(key)
        .role("button")
        .label(name.clone())
        .help(name)
        .expanded(!collapsed)
        .w_full()
        .min_h(row_height)
        .px(4.0)
        .gap(6.0)
        .center_y()
        .radius(5.0)
        .hover(a.hover_bg())
        .on_press(move |s: &mut State| s.explorer_search.toggle(group))
        .child(
            rune::icon(
                if collapsed {
                    &rune_kit::glyphs::TRIANGLE_RIGHT
                } else {
                    &rune_kit::glyphs::TRIANGLE_DOWN
                },
                12.0,
            )
            .color(a.muted()),
        )
        .child(content.grow(1.0).min_w(0.0))
        .child(
            text(rows.to_string())
                .font(ui_style(10.0, 500.0))
                .color(a.muted()),
        )
}

/// The Search tab's rows: files grouped under path headers, then terminal
/// matches under each terminal's title. Each group collapses. Rows show as
/// the search finds them.
pub(crate) fn results(s: &State, a: &Appearance) -> El {
    let search = &s.explorer_search;
    let note = |message: String| {
        text(message)
            .key("explorer-search-note")
            .font(ui_label())
            .color(a.muted())
            .wrap()
    };
    let mut list = col()
        .key(RESULTS_KEY)
        .role("listbox")
        .label("Search results")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(1.0)
        .pb(6.0)
        .preserve_focus_on_press(true)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .scroll_margin(8.0);
    let row_height = 24.0_f32.max(ui_label().size * 1.5 + 4.0);
    let mark = a.accent().alpha(0.32);
    let mut index = 0;
    for (f, file) in search.results.iter().enumerate() {
        let (folder, name) = match file.relative.rsplit_once('/') {
            Some((folder, name)) => (Some(folder), name),
            None => (None, file.relative.as_str()),
        };
        let collapsed = search.collapsed.contains(&Group::File(f));
        let mut title = row()
            .gap(6.0)
            .center_y()
            .child(rune::icon(
                explorer::icons::resolve(&file.path, false, false, a.dark),
                16.0,
            ))
            .child(
                text(name)
                    .font(ui_label())
                    .color(a.ink())
                    .min_w(0.0)
                    .truncate(),
            );
        if let Some(folder) = folder {
            title = title.child(
                text(folder)
                    .font(ui_label())
                    .color(a.muted())
                    .min_w(0.0)
                    .shrink()
                    .truncate(),
            );
        }
        list = list.child(group_header(
            a,
            format!("explorer-search-file-{f}"),
            Group::File(f),
            collapsed,
            usize::from(file.name.is_some()) + file.lines.len(),
            file.relative.clone(),
            title,
        ));
        if collapsed {
            continue;
        }
        let mut hit_row = |label: String, lead: El, body: El| {
            let i = index;
            index += 1;
            let selected = i == search.selected;
            row()
                .key(hit_key(i))
                .role("option")
                .label(label)
                .w_full()
                .min_h(row_height)
                .pl(26.0)
                .pr(4.0)
                .gap(8.0)
                .center_y()
                .radius(5.0)
                .fill(if selected {
                    a.hover_bg()
                } else {
                    Color::TRANSPARENT
                })
                .hover(a.hover_bg())
                .on_press(move |s: &mut State| s.open_search_hit(i))
                .child(lead)
                .child(body)
        };
        if let Some(range) = file.name.clone() {
            let file_name = file.path.file_name().unwrap_or_default().to_string_lossy();
            list = list.child(hit_row(
                format!("File name: {file_name}"),
                text("Name").font(ui_label()).color(a.muted()),
                text(file_name.into_owned())
                    .key("name")
                    .font(ui_label())
                    .color(a.ink())
                    .min_w(0.0)
                    .truncate()
                    .text_highlight(rune::TextHighlight::new(range, mark)),
            ));
        }
        for line in &file.lines {
            list = list.child(hit_row(
                format!("Line {}: {}", line.line, line.snippet.text),
                text(line.line.to_string())
                    .key("line")
                    .font(ui_label())
                    .color(a.muted())
                    .min_w(28.0),
                text(line.snippet.text.clone())
                    .key("snippet")
                    .font(fonts::editor())
                    .color(a.ink())
                    .min_w(0.0)
                    .shrink()
                    .truncate()
                    .text_highlight(rune::TextHighlight::new(
                        line.snippet.highlight.clone(),
                        mark,
                    )),
            ));
        }
    }
    for (t, terminal) in search.terminal.iter().enumerate() {
        let group = Group::Terminal(terminal.session);
        let collapsed = search.collapsed.contains(&group);
        list = list.child(group_header(
            a,
            format!("explorer-search-terminal-{t}"),
            group,
            collapsed,
            terminal.found.len(),
            format!("Terminal: {}", terminal.title),
            row()
                .gap(6.0)
                .center_y()
                .child(rune::icon(&rune_kit::glyphs::TERMINAL, 16.0).color(a.muted()))
                .child(
                    text(terminal.title.clone())
                        .font(ui_label())
                        .color(a.ink())
                        .min_w(0.0)
                        .truncate(),
                )
                .child(text("Terminal").font(ui_label()).color(a.muted())),
        ));
        if collapsed {
            continue;
        }
        for found in &terminal.found {
            let i = index;
            index += 1;
            let selected = i == search.selected;
            list = list.child(
                row()
                    .key(hit_key(i))
                    .role("option")
                    .label(format!("Terminal {}: {}", terminal.title, found.text))
                    .w_full()
                    .min_h(row_height)
                    .pl(26.0)
                    .pr(4.0)
                    .gap(8.0)
                    .center_y()
                    .radius(5.0)
                    .fill(if selected {
                        a.hover_bg()
                    } else {
                        Color::TRANSPARENT
                    })
                    .hover(a.hover_bg())
                    .on_press(move |s: &mut State| s.open_search_hit(i))
                    .child(
                        text(found.text.clone())
                            .key("snippet")
                            .font(fonts::editor())
                            .color(a.ink())
                            .min_w(0.0)
                            .shrink()
                            .truncate()
                            .text_highlight(rune::TextHighlight::new(
                                found.highlight.clone(),
                                mark,
                            )),
                    ),
            );
        }
    }
    if search.query.text.is_empty() {
        list = list.child(note(
            "Type in the search field to search file names, file contents and terminals.".into(),
        ));
    }
    a.kit().scroll_bar(list)
}

/// "1 result" or "3 results".
pub(super) fn results_count(n: usize) -> String {
    format!("{n} {}", if n == 1 { "result" } else { "results" })
}

/// The Search tab's footer: whether the file search is still running, and
/// how many results it and the terminals found.
pub(crate) fn footer(s: &State, a: &Appearance) -> El {
    let search = &s.explorer_search;
    let (file_rows, files, terminal_rows) = search.counts();
    let total = file_rows + terminal_rows;
    let running = !search.query.text.is_empty() && search.summary.is_none();
    let mut parts = Vec::new();
    if search.query.text.is_empty() {
        parts.push("No search".to_owned());
    } else if !running && total == 0 {
        parts.push("No results".to_owned());
    } else {
        let mut found = results_count(total);
        if running {
            found.push_str(" so far");
        }
        parts.push(found);
        parts.push(format!(
            "{files} {}",
            if files == 1 { "file" } else { "files" }
        ));
        if terminal_rows > 0 {
            let terminals = search.terminal.len();
            parts.push(format!(
                "{terminal_rows} in {terminals} {}",
                if terminals == 1 {
                    "terminal"
                } else {
                    "terminals"
                }
            ));
        }
    }
    let mut status = parts.join(" · ");
    if search
        .summary
        .as_ref()
        .is_some_and(|summary| summary.truncated)
    {
        status = format!(
            "{status} · First {} file results shown, refine to see more",
            search::RESULT_LIMIT
        );
    }
    let mut bar = row()
        .key("explorer-search-footer")
        .role("status")
        .label(if running {
            format!("Searching files… {status}")
        } else {
            status.clone()
        })
        .w_full()
        .min_h(26.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .fill(a.panel())
        .border_sides([1.0, 0.0, 0.0, 0.0], a.hover_bg());
    if running {
        bar = bar
            .child(
                rune_kit::Spinner::new()
                    .size(12.0)
                    .color(a.muted())
                    .build(a.kit()),
            )
            .child(
                text("Searching files…")
                    .key("explorer-search-searching")
                    .font(ui_label())
                    .color(a.muted()),
            );
    }
    bar.child(
        text(status)
            .key("explorer-search-status")
            .font(ui_label())
            .color(a.muted())
            .min_w(0.0)
            .shrink()
            .truncate(),
    )
}
