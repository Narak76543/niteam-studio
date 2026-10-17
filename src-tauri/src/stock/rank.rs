//! Local ranking of stock candidates (no AI; `docs/11-real-world-visuals.md` §4): keyword
//! match, resolution, shape, license, AI-generated penalty; sources are mixed so one can't
//! fill the grid. Also the resolution rules used by the renderer (low-res, max zoom).

use std::collections::HashMap;

use super::license::LicenseCode;
use super::{Candidate, Source};

/// Largest zoom of the photo motion (`render/clip.rs`).
#[cfg_attr(not(test), expect(dead_code, reason = "used by the photo motion (render step)"))]
pub const MAX_ZOOM: f32 = 1.08;
const AI_PENALTY: f32 = 50.0;

/// Width left after cropping to 16:9.
pub fn cropped_width(width: u32, height: u32) -> u32 {
    width.min((u64::from(height) * 16 / 9) as u32)
}

/// Smaller than the frame after the 16:9 crop (would be upscaled → soft).
pub fn is_low_res(width: u32, height: u32, frame_width: u32) -> bool {
    cropped_width(width, height) < frame_width
}

/// Zoom that never enlarges the picture past its real pixels: 1.0 (no zoom) for low-res
/// images, at most [`MAX_ZOOM`].
#[cfg_attr(not(test), expect(dead_code, reason = "used by the photo motion (render step)"))]
pub fn max_zoom(width: u32, height: u32, frame_width: u32) -> f32 {
    let ratio = cropped_width(width, height) as f32 / frame_width.max(1) as f32;
    ratio.clamp(1.0, MAX_ZOOM)
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(str::to_owned)
        .collect()
}

/// 0–40: share of the keyword words found in the title or tags.
fn keyword_score(c: &Candidate, keywords: &[String]) -> f32 {
    let wanted: Vec<String> = keywords.iter().flat_map(|k| words(k)).collect();
    if wanted.is_empty() {
        return 0.0;
    }
    let have: Vec<String> = words(&c.title).into_iter().chain(c.tags.iter().flat_map(|t| words(t))).collect();
    // "racks" matches "rack" and the other way round.
    let found = wanted
        .iter()
        .filter(|w| have.iter().any(|h| h == *w || h.trim_end_matches('s') == w.trim_end_matches('s')))
        .count();
    40.0 * found as f32 / wanted.len() as f32
}

/// 0 when low-res, else 10–30 (30 from twice the frame width up).
fn resolution_score(c: &Candidate, frame_width: u32) -> f32 {
    if is_low_res(c.width, c.height, frame_width) {
        return 0.0;
    }
    let ratio = cropped_width(c.width, c.height) as f32 / frame_width.max(1) as f32;
    10.0 + 20.0 * ratio.log2().clamp(0.0, 1.0)
}

fn shape_score(c: &Candidate) -> f32 {
    let aspect = c.width as f32 / c.height.max(1) as f32;
    if aspect >= 1.3 {
        10.0
    } else if aspect >= 0.9 {
        4.0
    } else {
        0.0
    }
}

fn license_score(c: &Candidate) -> f32 {
    match c.license.code {
        LicenseCode::Cc0 | LicenseCode::Pdm | LicenseCode::Pexels | LicenseCode::Pixabay | LicenseCode::Own => 10.0,
        LicenseCode::By => 8.0,
        LicenseCode::BySa => 5.0,
    }
}

pub fn score(c: &Candidate, keywords: &[String], frame_width: u32) -> f32 {
    let mut s = keyword_score(c, keywords) + resolution_score(c, frame_width) + shape_score(c) + license_score(c);
    if c.ai_generated {
        s -= AI_PENALTY;
    }
    s
}

/// Scores, marks low-res, drops AI-generated files and returns the best `limit`, at most
/// half of them from one source while others still have candidates.
pub fn rank(mut all: Vec<Candidate>, keywords: &[String], frame_width: u32, limit: usize) -> Vec<Candidate> {
    for c in &mut all {
        c.score = score(c, keywords, frame_width);
        c.low_res = is_low_res(c.width, c.height, frame_width);
    }
    all.retain(|c| !c.ai_generated);
    all.sort_by(|a, b| b.score.total_cmp(&a.score));
    let sources = all.iter().map(|c| c.source).collect::<std::collections::HashSet<Source>>().len();
    let cap = if sources > 1 { limit.div_ceil(2) } else { limit };
    let mut per_source: HashMap<Source, usize> = HashMap::new();
    let mut out = Vec::with_capacity(limit);
    let mut skipped = Vec::new();
    for c in all {
        if out.len() == limit {
            break;
        }
        let n = per_source.entry(c.source).or_default();
        if *n < cap {
            *n += 1;
            out.push(c);
        } else {
            skipped.push(c);
        }
    }
    // Not enough from the other sources: fill up with the best skipped ones.
    out.extend(skipped.into_iter().take(limit - out.len()));
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stock::license::License;
    use crate::stock::tests::candidate;

    #[test]
    fn resolution_rules() {
        assert_eq!(cropped_width(6000, 4000), 6000);
        assert_eq!(cropped_width(3456, 5184), 3456);
        assert_eq!(cropped_width(1280, 600), 1066);
        assert!(is_low_res(1280, 853, 1920));
        assert!(!is_low_res(1280, 853, 1280));
        assert_eq!(max_zoom(1280, 853, 1920), 1.0, "no zoom on small images");
        assert!((max_zoom(2000, 1125, 1920) - 2000.0 / 1920.0).abs() < 1e-4);
        assert_eq!(max_zoom(6000, 4000, 1920), MAX_ZOOM);
    }

    #[test]
    fn bigger_and_better_matching_ranks_higher() {
        let kw = vec!["server rack".to_owned(), "data center".to_owned()];
        let big = candidate(Source::Wikimedia, 6000, 4000, License::cc0());
        let small = candidate(Source::Wikimedia, 1024, 683, License::cc0());
        let mut off_topic = candidate(Source::Wikimedia, 6000, 4000, License::cc0());
        off_topic.title = "Cat on a sofa".into();
        assert!(score(&big, &kw, 1920) > score(&small, &kw, 1920));
        assert!(score(&big, &kw, 1920) > score(&off_topic, &kw, 1920));
        let mut portrait = big.clone();
        (portrait.width, portrait.height) = (4000, 6000);
        assert!(score(&big, &kw, 1920) > score(&portrait, &kw, 1920));
    }

    #[test]
    fn hides_ai_images_marks_low_res_and_mixes_sources() {
        let kw = vec!["server".to_owned()];
        let mut all = Vec::new();
        for i in 0..6 {
            let mut c = candidate(Source::Wikimedia, 6000 - i * 100, 4000, License::cc0());
            c.source_id = format!("w{i}");
            all.push(c);
        }
        let mut ov = candidate(Source::Openverse, 1024, 683, License::cc0());
        ov.source_id = "o1".into();
        all.push(ov);
        let mut ai = candidate(Source::Wikimedia, 8000, 4500, License::cc0());
        ai.ai_generated = true;
        all.push(ai);
        let top = rank(all, &kw, 1920, 6);
        assert_eq!(top.len(), 6);
        assert!(top.iter().all(|c| !c.ai_generated));
        let ov = top.iter().find(|c| c.source == Source::Openverse).expect("other source kept in the grid");
        assert!(ov.low_res);
        assert!(top.windows(2).all(|w| w[0].score >= w[1].score));
    }
}
