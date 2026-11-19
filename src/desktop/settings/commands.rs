use super::*;

pub(super) fn bundled_commands(s: &State, a: &Appearance) -> Vec<El> {
    profile::utility_names()
        .into_iter()
        .filter(|name| {
            if query(s).is_empty() {
                s.settings_tab == Tab::Commands
            } else {
                matches(s, Tab::Commands, name)
            }
        })
        .map(|name| {
            let enabled = !s.config.disabled.contains(&name);
            let key = format!("utility-{name}");
            let flip = name.clone();
            plain_row(
                a,
                format!("{key}-row"),
                &name,
                toggle(a, key, &name, enabled, move |s, _| {
                    let name = flip.clone();
                    s.save_config(move |config| {
                        if config.disabled.contains(&name) {
                            config.disabled.retain(|n| n != &name);
                        } else {
                            config.disabled.push(name);
                        }
                    });
                }),
            )
        })
        .collect()
}
