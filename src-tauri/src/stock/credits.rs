//! "Credits" block for the video description (`docs/11-real-world-visuals.md` §3): one line
//! per stock file the video uses, in order of first appearance, then the sources. The user's
//! own screenshots are left out.

use super::Source;
use super::assets::AssetRegistry;
use crate::pipeline::project::Project;

/// Credits text, or `None` when the video uses no stock files.
pub fn build(project: &Project, registry: &AssetRegistry) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut lines = Vec::new();
    let mut sources: Vec<Source> = Vec::new();
    for s in &project.scenes {
        if !s.visual.uses_asset() {
            continue;
        }
        let Some(record) = s.visual.asset.as_deref().and_then(|id| registry.get(id)) else { continue };
        if record.source == Source::User || seen.contains(&record.id.as_str()) {
            continue;
        }
        seen.push(&record.id);
        lines.push(format!("- {}", record.attribution));
        if !sources.contains(&record.source) {
            sources.push(record.source);
        }
    }
    if lines.is_empty() {
        return None;
    }
    let names: Vec<&str> = sources.iter().map(|s| s.display()).collect();
    Some(format!("Credits\n{}\nPhotos and videos from {}.", lines.join("\n"), names.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::tests::sample;
    use crate::pipeline::project::{Scene, VisualKind};
    use crate::stock::assets::AssetRecord;
    use crate::stock::license::{License, LicenseCode};
    use crate::stock::Media;

    fn record(id: &str, source: Source, attribution: &str) -> AssetRecord {
        AssetRecord {
            id: id.into(),
            source,
            media: Media::Photo,
            file: format!("assets/{id}.jpg"),
            title: String::new(),
            author: String::new(),
            author_url: None,
            license: License::new(LicenseCode::Cc0),
            original_url: String::new(),
            file_url: String::new(),
            downloaded_at: String::new(),
            width: 1,
            height: 1,
            duration_ms: None,
            attribution: attribution.into(),
            restrictions: vec![],
            sha256: String::new(),
            bytes: 0,
            poster: None,
        }
    }

    #[test]
    fn one_line_per_file_in_order_without_own_material() {
        let mut p = sample();
        for (i, (kind, asset)) in [
            (VisualKind::RealPhoto, Some("wikimedia_1")),
            (VisualKind::StockVideo, Some("pixabay_2")),
            (VisualKind::RealPhoto, Some("wikimedia_1")),
            (VisualKind::Screenshot, Some("user_abc")),
            (VisualKind::Diagram, None),
            (VisualKind::AiImage, Some("wikimedia_9")),
        ]
        .into_iter()
        .enumerate()
        {
            let mut s = Scene::new(format!("s{i}"), 0);
            s.visual.kind = kind;
            s.visual.asset = asset.map(str::to_owned);
            p.scenes.push(s);
        }
        let reg = AssetRegistry {
            assets: vec![
                record("wikimedia_1", Source::Wikimedia, "\"Cable\" by A via Wikimedia Commons — CC0 — https://c/1"),
                record("pixabay_2", Source::Pixabay, "Video by B from Pixabay — https://p/2"),
                record("user_abc", Source::User, "shot — own material"),
                record("wikimedia_9", Source::Wikimedia, "unused"),
            ],
        };
        assert_eq!(
            build(&p, &reg).unwrap(),
            "Credits\n- \"Cable\" by A via Wikimedia Commons — CC0 — https://c/1\n- Video by B from Pixabay — https://p/2\nPhotos and videos from Wikimedia Commons, Pixabay."
        );
        assert!(build(&sample(), &reg).is_none());
    }
}
