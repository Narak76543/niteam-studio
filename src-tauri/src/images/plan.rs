//! What each scene's image will be made with: provider, model, final prompt, seed, steps and
//! cache hash (07 §3, §5, §8). Pure: depends only on `project.json`.

use sha2::{Digest, Sha256};

use super::cloudflare;
use super::prompt::{PromptParts, build_prompt, cache_hash, is_cambodia_related, scene_seed};
use super::provider::ProviderId;
use crate::pipeline::project::{ImageSnapshot, Language, Project, Scene};

#[derive(Debug, Clone, PartialEq)]
pub struct ScenePlan {
    pub scene_id: String,
    /// Scene whose image is reused ("2 scenes per image"); `None` = own image.
    pub shared_from: Option<String>,
    pub provider: ProviderId,
    pub model: String,
    pub prompt: String,
    pub seed: u32,
    pub steps: u8,
    pub hash: String,
    pub key_scene: bool,
}

pub fn provider_for(scene: &Scene, s: &ImageSnapshot) -> ProviderId {
    match (scene.key_scene, s.key_scene_provider) {
        (true, Some(p)) => p,
        _ => s.provider,
    }
}

pub fn model_for(provider: ProviderId, s: &ImageSnapshot) -> String {
    match provider {
        ProviderId::CloudflareFlux => cloudflare::MODEL.to_owned(),
        ProviderId::GeminiNanoBanana => s.gemini_model.clone(),
    }
}

/// Seed base for projects without a Visual Bible: stable, derived from the project id.
fn fallback_seed(project_id: &str) -> u32 {
    let d = Sha256::digest(project_id.as_bytes());
    u32::from_le_bytes([d[0], d[1], d[2], d[3]]) % 100_000
}

/// Index of the scene that owns the image for `index` (pairs share with 2 scenes per image).
pub fn source_index(index: usize, scenes_per_image: u8) -> usize {
    if scenes_per_image >= 2 && index % 2 == 1 {
        index - 1
    } else {
        index
    }
}

fn own_plan(project: &Project, scene: &Scene) -> ScenePlan {
    let snap = &project.image_settings;
    let provider = provider_for(scene, snap);
    let model = model_for(provider, snap);
    let bible = project.visual_bible.as_ref();
    let style = bible.map_or(project.style.style_prompt.as_str(), |b| b.style.as_str());
    let characters: Vec<(&str, &str)> = bible
        .map(|b| {
            scene
                .characters
                .iter()
                .filter_map(|k| b.characters.get(k).map(|d| (k.as_str(), d.as_str())))
                .collect()
        })
        .unwrap_or_default();
    let place = bible.and_then(|b| scene.place.as_ref().and_then(|p| b.places.get(p)));
    let cambodia = is_cambodia_related(
        project.language == Language::Km,
        &[&project.topic, &project.title],
    );
    let culture = bible
        .map(|b| b.culture_notes.as_str())
        .filter(|c| cambodia && !c.trim().is_empty());
    let prompt = build_prompt(&PromptParts {
        style,
        shot: scene.shot,
        subject: &scene.image_prompt,
        characters,
        place: place.map(String::as_str),
        culture_notes: culture,
        square: provider == ProviderId::CloudflareFlux,
    });
    let base = bible.map_or_else(|| fallback_seed(&project.id), |b| b.seed);
    let seed = scene_seed(base, &scene.id, scene.image.seed);
    let steps = match provider {
        ProviderId::CloudflareFlux => snap.steps,
        ProviderId::GeminiNanoBanana => 0,
    };
    let hash = cache_hash(provider.as_str(), &model, &prompt, seed, steps);
    ScenePlan {
        scene_id: scene.id.clone(),
        shared_from: None,
        provider,
        model,
        prompt,
        seed,
        steps,
        hash,
        key_scene: scene.key_scene,
    }
}

/// Plan for the scene at `index` (a shared scene gets its source's prompt/hash).
pub fn plan_scene(project: &Project, index: usize) -> Option<ScenePlan> {
    let scene = project.scenes.get(index)?;
    let src = source_index(index, project.image_settings.scenes_per_image);
    let source = project.scenes.get(src)?;
    let mut plan = own_plan(project, source);
    if src != index {
        plan.shared_from = Some(source.id.clone());
        plan.scene_id = scene.id.clone();
    }
    Some(plan)
}

pub fn plan_all(project: &Project) -> Vec<ScenePlan> {
    (0..project.scenes.len())
        .filter_map(|i| plan_scene(project, i))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::pipeline::project::tests::sample;
    use crate::pipeline::project::{Shot, VisualBible};

    fn project() -> Project {
        let mut p = sample();
        p.topic = "How was Angkor Wat built?".into();
        p.visual_bible = Some(VisualBible {
            style: "warm watercolor".into(),
            characters: BTreeMap::from([("farmer".to_owned(), "old Khmer farmer in krama".to_owned())]),
            places: BTreeMap::from([("village".to_owned(), "stilt-house village".to_owned())]),
            culture_notes: "Khmer roof finials".into(),
            seed: 100,
        });
        for i in 1..=4 {
            let mut s = Scene::new(format!("s{i:03}"), 0);
            s.image_prompt = format!("the farmer walks, scene {i}");
            s.characters = vec!["farmer".into()];
            s.place = Some("village".into());
            s.shot = Shot::Wide;
            p.scenes.push(s);
        }
        p.scenes[0].key_scene = true;
        p
    }

    #[test]
    fn plan_builds_prompt_seed_and_hash() {
        let p = project();
        let plan = plan_scene(&p, 1).unwrap();
        assert_eq!(plan.provider, ProviderId::CloudflareFlux);
        assert_eq!(plan.model, cloudflare::MODEL);
        assert_eq!(plan.seed, 102);
        assert!(plan.prompt.starts_with("warm watercolor, wide establishing shot, the farmer (old Khmer farmer in krama) walks"));
        assert!(plan.prompt.contains("stilt-house village, Khmer roof finials"));
        assert!(plan.prompt.contains("center band"));
    }

    #[test]
    fn key_scene_provider_and_pairs() {
        let mut p = project();
        p.image_settings.key_scene_provider = Some(ProviderId::GeminiNanoBanana);
        p.image_settings.gemini_model = "gemini-3-pro-image".into();
        let key = plan_scene(&p, 0).unwrap();
        assert_eq!(key.provider, ProviderId::GeminiNanoBanana);
        assert!(!key.prompt.contains("center band"), "16:9 provider: no square composition");
        assert_eq!(plan_scene(&p, 1).unwrap().provider, ProviderId::CloudflareFlux);

        p.image_settings.scenes_per_image = 2;
        let second = plan_scene(&p, 1).unwrap();
        assert_eq!(second.shared_from.as_deref(), Some("s001"));
        assert_eq!(second.hash, plan_scene(&p, 0).unwrap().hash);
        assert_eq!(plan_scene(&p, 2).unwrap().shared_from, None);
    }

    #[test]
    fn changes_that_must_invalidate_the_cache() {
        let p = project();
        let base = plan_scene(&p, 1).unwrap().hash;
        let mut q = p.clone();
        q.visual_bible.as_mut().unwrap().style = "flat 2D".into();
        assert_ne!(plan_scene(&q, 1).unwrap().hash, base, "style");
        let mut q = p.clone();
        q.scenes[1].image.seed = Some(9);
        assert_ne!(plan_scene(&q, 1).unwrap().hash, base, "seed");
        let mut q = p.clone();
        q.image_settings.steps = 6;
        assert_ne!(plan_scene(&q, 1).unwrap().hash, base, "steps");
        let mut q = p.clone();
        q.scenes[1].image_prompt = "a river".into();
        assert_ne!(plan_scene(&q, 1).unwrap().hash, base, "prompt");
        let mut q = p.clone();
        q.scenes.swap(0, 2);
        assert_eq!(plan_scene(&q, 1).unwrap().hash, base, "reordering keeps the seed");
    }
}
