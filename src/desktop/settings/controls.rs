use super::*;

/// An on/off setting drawn as a switch, named for assistive technology.
pub(super) fn toggle(
    a: &Appearance,
    key: impl Into<String>,
    name: &str,
    on: bool,
    flip: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    let key = key.into();
    pressable(&key);
    rune_kit::Switch::new(on)
        .label(name)
        .hide_label()
        .on_toggle_tx(move |s: &mut State, tx: &mut Tx| {
            tx.blur();
            flip(s, tx);
        })
        .build(a.kit())
        .key(key)
        .no_window_drag()
        .help(name)
}

/// A dropdown choosing one of `options`, saved through `set` as it is chosen.
/// One dropdown is open at a time; its list is drawn as an overlay layer.
#[allow(clippy::too_many_arguments)]
pub(super) fn choice<T: Copy + PartialEq + 'static>(
    s: &State,
    a: &Appearance,
    layers: &mut Vec<El>,
    key: &'static str,
    name: &str,
    options: Vec<(String, T)>,
    current: T,
    set: fn(&mut Config, T),
) -> El {
    let labels: Vec<&str> = options.iter().map(|(label, _)| label.as_str()).collect();
    let shown = options
        .iter()
        .find(|(_, value)| *value == current)
        .map(|(label, _)| label.clone());
    // Return opens the dropdown and chooses its focused option, as Space does.
    pressable(&format!("{key}-trigger"));
    for i in 0..options.len() {
        pressable(&format!("option{i}"));
    }
    let select = rune_kit::Select::new(key, &labels)
        .label(name)
        .current(shown.as_deref())
        .appearance(rune_kit::SelectAppearance::Field)
        .width(240.0)
        .open(s.settings_select == Some(key))
        .on_toggle(move |s: &mut State| {
            s.theme_family_picker.close();
            s.font_ui.close();
            s.font_terminal.close();
            s.settings_select = if s.settings_select == Some(key) {
                None
            } else {
                Some(key)
            };
        })
        .on_close(move |s: &mut State| {
            if s.settings_select == Some(key) {
                s.settings_select = None;
            }
        })
        .on_choose(move |s: &mut State, label: &str, _: &mut Tx| {
            s.settings_select = None;
            if let Some(&(_, value)) = options.iter().find(|(l, _)| l == label) {
                s.save_config(|c| set(c, value));
            }
        });
    select.build(a.kit(), layers).shrink().min_w(0.0)
}

pub(super) fn labels<T: Copy>(options: &[(&str, T)]) -> Vec<(String, T)> {
    options.iter().map(|&(l, v)| (l.to_owned(), v)).collect()
}

/// A value between decrease and increase buttons keyed `less-{key}` and
/// `more-{key}`. `change` gets -1 or 1.
pub(super) fn stepper(
    a: &Appearance,
    key: &str,
    subject: &str,
    shown: String,
    change: fn(&mut Config, f32),
) -> El {
    let step_button = |action: &str, title: &'static str, name: &str, sign: f32| {
        let name = format!("{name} {subject}");
        button(a, format!("{action}-{key}"), title, move |s, _| {
            s.save_config(|c| change(c, sign));
        })
        .w(32.0)
        .px(0.0)
        .label(name.clone())
        .help(name)
    };
    row()
        .gap(6.0)
        .center_y()
        .child(step_button("less", "−", "Decrease", -1.0))
        .child(
            text(shown)
                .key(format!("{key}-value"))
                .font(ui_label())
                .color(a.ink())
                .min_w(72.0),
        )
        .child(step_button("more", "+", "Increase", 1.0))
}

/// `value` moved by `delta`, kept to one decimal place and inside `range`.
pub(super) fn step(value: f32, delta: f32, (min, max): (f32, f32)) -> f32 {
    (((value + delta) * 10.0).round() / 10.0).clamp(min, max)
}

/// An absolute folder path field, shown under a Specified folder choice. Each
/// edit stores the typed path through `save`.
pub(super) fn folder_field(
    a: &Appearance,
    edit: &TextEdit,
    key: &'static str,
    label: &'static str,
    field: fn(&mut State) -> &mut TextEdit,
    save: fn(&mut Config, PathBuf),
) -> El {
    text_field(a, edit, key, label)
        .py(5.0)
        .placeholder("Absolute folder path", a.muted())
        .border(1.0, a.kit().c.border_base)
        .radius(6.0)
        .on_edit(field)
        .on_input_change(move |s: &mut State, _| {
            let path = PathBuf::from(&field(s).text);
            s.save_config(|c| save(c, path));
        })
}
