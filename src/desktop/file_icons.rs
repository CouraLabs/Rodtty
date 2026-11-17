//! Offline vscode-icons associations shared by the explorer and editor tabs.
//! Artwork is kept intact; Rune's SVG feature paints its original colors/gradients.
use super::file_icon_catalog::ARTWORK;
use rune::Icon;
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, sync::OnceLock};

#[derive(Deserialize)]
struct Associations {
    filenames: BTreeMap<String, [String; 2]>,
    extensions: BTreeMap<String, [String; 2]>,
    folders: BTreeMap<String, [String; 4]>,
}

pub(super) fn resolve(path: &Path, folder: bool, expanded: bool, dark: bool) -> &'static Icon {
    static ASSOCIATIONS: OnceLock<Associations> = OnceLock::new();
    let maps = ASSOCIATIONS.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/vscode-icons/mappings.json"))
            .expect("bundled vscode-icons associations")
    });
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let variant = usize::from(!dark);
    let artwork = if folder {
        maps.folders
            .get(&name)
            .map(|icons| icons[variant + usize::from(expanded) * 2].as_str())
    } else {
        maps.filenames
            .get(&name)
            .or_else(|| {
                // Match longest compound extensions first (e.g. d.ts before ts).
                name.match_indices('.')
                    .find_map(|(i, _)| maps.extensions.get(&name[i + 1..]))
            })
            .map(|icons| icons[variant].as_str())
    }
    .unwrap_or(if !folder {
        "default_file.svg"
    } else if expanded {
        "default_folder_opened.svg"
    } else {
        "default_folder.svg"
    });
    let index = ARTWORK
        .binary_search_by_key(&artwork, |(name, _)| *name)
        .expect("every bundled association has artwork");
    &ARTWORK[index].1
}
