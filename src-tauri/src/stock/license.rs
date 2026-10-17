//! Licenses we accept and how they are credited (`docs/11-real-world-visuals.md` §3).
//! Allowed: Pexels License, Pixabay Content License, CC0, public domain, CC BY, and CC BY-SA
//! only when the user turned it on. Never NC or ND; anything we can't recognise is skipped.

use serde::{Deserialize, Serialize};

use super::{Candidate, Media, Source};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LicenseCode {
    Pexels,
    Pixabay,
    Cc0,
    /// Public domain (PD templates, Public Domain Mark).
    Pdm,
    By,
    BySa,
    /// The user's own screenshot or photo (consent checkbox).
    Own,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct License {
    pub code: LicenseCode,
    /// "4.0", "2.0 fr" (version + port).
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

impl License {
    pub fn new(code: LicenseCode) -> Self {
        Self { code, version: None, url: None }
    }

    #[cfg(test)]
    pub fn cc0() -> Self {
        Self::new(LicenseCode::Cc0)
    }

    pub fn allowed(&self, allow_by_sa: bool) -> bool {
        self.code != LicenseCode::BySa || allow_by_sa
    }

    pub fn attribution_required(&self) -> bool {
        matches!(self.code, LicenseCode::By | LicenseCode::BySa)
    }

    /// "CC BY-SA 2.0 fr", "CC0", "Public domain", "Pexels License".
    pub fn label(&self) -> String {
        let with_version = |name: &str| match &self.version {
            Some(v) if !v.is_empty() => format!("{name} {v}"),
            _ => name.to_owned(),
        };
        match self.code {
            LicenseCode::Pexels => "Pexels License".into(),
            LicenseCode::Pixabay => "Pixabay Content License".into(),
            LicenseCode::Cc0 => "CC0".into(),
            LicenseCode::Pdm => "Public domain".into(),
            LicenseCode::By => with_version("CC BY"),
            LicenseCode::BySa => with_version("CC BY-SA"),
            LicenseCode::Own => "Own".into(),
        }
    }
}

/// Openverse `license` + `license_version` (`cc0`, `pdm`, `by`, `by-sa`; anything else → None).
pub fn from_openverse(code: &str, version: Option<&str>, url: Option<&str>) -> Option<License> {
    let code = match code.trim().to_lowercase().as_str() {
        "cc0" => LicenseCode::Cc0,
        "pdm" => LicenseCode::Pdm,
        "by" => LicenseCode::By,
        "by-sa" => LicenseCode::BySa,
        _ => return None,
    };
    Some(License {
        code,
        version: version.map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned),
        url: url.map(str::to_owned),
    })
}

/// Commons `LicenseShortName`: "CC0", "Public domain", "CC BY 4.0", "CC BY-SA 2.0 fr",
/// "CC-BY-SA-3.0"… NC, ND, GFDL-only and unknown names → None.
pub fn from_commons(short_name: &str, url: Option<&str>) -> Option<License> {
    let lower = short_name.trim().to_lowercase();
    let url = url.map(str::to_owned);
    if lower == "cc0" || lower.starts_with("cc0 ") || lower.starts_with("cc-zero") {
        return Some(License { code: LicenseCode::Cc0, version: None, url });
    }
    if lower.starts_with("public domain") || lower == "pd" || lower.starts_with("pdm") || lower.starts_with("cc pdm") {
        return Some(License { code: LicenseCode::Pdm, version: None, url });
    }
    let tokens: Vec<String> = lower.replace('-', " ").split_whitespace().map(str::to_owned).collect();
    if tokens.first().map(String::as_str) != Some("cc") {
        return None;
    }
    let mut elements = Vec::new();
    let mut rest = Vec::new();
    for t in &tokens[1..] {
        if rest.is_empty() && t.chars().all(|c| c.is_ascii_alphabetic()) {
            elements.push(t.as_str());
        } else {
            rest.push(t.as_str());
        }
    }
    let code = match elements.as_slice() {
        ["by"] => LicenseCode::By,
        ["by", "sa"] => LicenseCode::BySa,
        _ => return None, // nc, nd, sampling, unknown
    };
    if !rest.first().is_some_and(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(License { code, version: Some(rest.join(" ")), url })
}

/// One credit line, e.g. `"Server racks" by Jane Doe — CC BY 4.0 (https://…) — https://commons…`
/// or `Photo by Jane Doe on Pexels — https://www.pexels.com/photo/…`.
pub fn credit_line(c: &Candidate) -> String {
    let kind = if c.media == Media::Video { "Video" } else { "Photo" };
    let author = if c.author.trim().is_empty() { "unknown author" } else { c.author.trim() };
    match c.source {
        Source::Pexels => format!("{kind} by {author} on Pexels — {}", c.page_url),
        Source::Pixabay => format!("{kind} by {author} from Pixabay — {}", c.page_url),
        Source::User => format!("{} — own material", c.title),
        Source::Openverse | Source::Wikimedia => {
            let license = match &c.license.url {
                Some(u) if c.license.attribution_required() => format!("{} ({u})", c.license.label()),
                _ => c.license.label(),
            };
            let via = if c.source == Source::Wikimedia { " via Wikimedia Commons" } else { "" };
            format!("\"{}\" by {author}{via} — {license} — {}", c.title, c.page_url)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stock::tests::candidate;

    #[test]
    fn commons_names() {
        let l = from_commons("CC BY-SA 2.0 fr", None).unwrap();
        assert_eq!((l.code, l.version.as_deref()), (LicenseCode::BySa, Some("2.0 fr")));
        assert_eq!(l.label(), "CC BY-SA 2.0 fr");
        assert_eq!(from_commons("CC BY 4.0", None).unwrap().code, LicenseCode::By);
        assert_eq!(from_commons("CC-BY-SA-3.0", None).unwrap().code, LicenseCode::BySa);
        assert_eq!(from_commons("CC0", None).unwrap().code, LicenseCode::Cc0);
        assert_eq!(from_commons("Public domain", None).unwrap().code, LicenseCode::Pdm);
        for bad in ["CC BY-NC 2.0", "CC BY-ND 4.0", "CC BY-NC-SA 3.0", "GFDL", "Attribution", "CC BY", ""] {
            assert!(from_commons(bad, None).is_none(), "{bad} must not be allowed");
        }
    }

    #[test]
    fn openverse_codes() {
        assert_eq!(from_openverse("by-sa", Some("4.0"), None).unwrap().label(), "CC BY-SA 4.0");
        assert!(from_openverse("by-nc", Some("2.0"), None).is_none());
        assert!(from_openverse("by-nd", Some("2.0"), None).is_none());
    }

    #[test]
    fn by_sa_only_when_allowed() {
        let sa = from_openverse("by-sa", Some("4.0"), None).unwrap();
        assert!(!sa.allowed(false) && sa.allowed(true));
        assert!(License::cc0().allowed(false));
    }

    #[test]
    fn credit_lines() {
        let mut c = candidate(Source::Wikimedia, 4000, 3000, from_commons("CC BY 2.0", Some("https://creativecommons.org/licenses/by/2.0")).unwrap());
        c.title = "Datacenter Server Racks".into();
        assert_eq!(
            credit_line(&c),
            "\"Datacenter Server Racks\" by Jane via Wikimedia Commons — CC BY 2.0 (https://creativecommons.org/licenses/by/2.0) — https://example.org/p"
        );
        let mut p = candidate(Source::Pexels, 4000, 3000, License::new(LicenseCode::Pexels));
        p.media = Media::Video;
        assert_eq!(credit_line(&p), "Video by Jane on Pexels — https://example.org/p");
        c.author.clear();
        assert!(credit_line(&c).contains("by unknown author"));
    }
}
