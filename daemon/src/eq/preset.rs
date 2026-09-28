//! EQ preset model: loading, validation, and persistence.
//!
//! Presets are TOML files identified by their file stem (the preset *id*).
//! Lookup order: user dir (`~/.config/airpods-helper/eq/`) → system dirs →
//! presets compiled into the binary. A user file with the same id as a
//! built-in overrides it; deleting that file reverts to the built-in.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::warn;

use crate::config;

pub const MAX_BANDS: usize = 16;
pub const FREQ_RANGE: (f64, f64) = (20.0, 20_000.0);
pub const Q_RANGE: (f64, f64) = (0.1, 10.0);
pub const GAIN_RANGE: (f64, f64) = (-24.0, 24.0);
pub const PREAMP_RANGE: (f64, f64) = (-24.0, 12.0);

const SYSTEM_DIRS: &[&str] = &[
    "/usr/share/airpods-helper/eq-presets",
    "/usr/local/share/airpods-helper/eq-presets",
];

const BUILTIN: &[(&str, &str)] = &[
    ("flat", include_str!("../../../eq-presets/flat.toml")),
    (
        "bass-boost",
        include_str!("../../../eq-presets/bass-boost.toml"),
    ),
    (
        "vocal-clarity",
        include_str!("../../../eq-presets/vocal-clarity.toml"),
    ),
    (
        "airpods-pro-crinacle",
        include_str!("../../../eq-presets/airpods-pro-crinacle.toml"),
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterType {
    Peaking,
    Lowshelf,
    Highshelf,
    Lowpass,
    Highpass,
    Notch,
}

impl FilterType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Peaking => "peaking",
            Self::Lowshelf => "lowshelf",
            Self::Highshelf => "highshelf",
            Self::Lowpass => "lowpass",
            Self::Highpass => "highpass",
            Self::Notch => "notch",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "peaking" | "peak" | "pk" => Self::Peaking,
            "lowshelf" | "ls" | "lsc" => Self::Lowshelf,
            "highshelf" | "hs" | "hsc" => Self::Highshelf,
            "lowpass" | "lp" => Self::Lowpass,
            "highpass" | "hp" => Self::Highpass,
            "notch" => Self::Notch,
            _ => return None,
        })
    }

    /// Whether the filter's gain parameter has any effect.
    pub fn uses_gain(self) -> bool {
        matches!(self, Self::Peaking | Self::Lowshelf | Self::Highshelf)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    #[serde(rename = "type")]
    pub filter_type: FilterType,
    pub freq: f64,
    pub q: f64,
    #[serde(default)]
    pub gain: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPreset {
    /// File stem; not stored in the file itself.
    #[serde(skip)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub preamp: f64,
    #[serde(default)]
    pub bands: Vec<EqBand>,
}

/// Where a preset was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    User,
    System,
    Builtin,
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("invalid preset id '{0}' (use lowercase letters, digits and '-', max 48 chars)")]
    InvalidId(String),
    #[error("preset '{0}' not found")]
    NotFound(String),
    #[error("preset '{0}' is built in and can't be deleted")]
    NotDeletable(String),
    #[error("invalid preset: {0}")]
    Invalid(String),
    #[error("failed to parse preset '{id}': {err}")]
    Parse { id: String, err: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Ids accepted when *reading*: anything that is a plain file stem. Stricter
/// rules apply to ids we create (see [`validate_new_id`]).
fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn validate_new_id(id: &str) -> Result<(), PresetError> {
    let ok = !id.is_empty()
        && id.len() <= 48
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(PresetError::InvalidId(id.to_string()))
    }
}

fn in_range(v: f64, (lo, hi): (f64, f64)) -> bool {
    v.is_finite() && v >= lo && v <= hi
}

impl EqPreset {
    pub fn validate(&self) -> Result<(), PresetError> {
        let bad = |m: String| Err(PresetError::Invalid(m));
        if self.name.trim().is_empty() || self.name.len() > 64 {
            return bad("name must be 1-64 characters".into());
        }
        if self.description.len() > 256 {
            return bad("description must be at most 256 characters".into());
        }
        if !in_range(self.preamp, PREAMP_RANGE) {
            return bad(format!(
                "preamp {} dB out of range {:?}",
                self.preamp, PREAMP_RANGE
            ));
        }
        if self.bands.len() > MAX_BANDS {
            return bad(format!("at most {MAX_BANDS} bands"));
        }
        for (i, b) in self.bands.iter().enumerate() {
            let n = i + 1;
            if !in_range(b.freq, FREQ_RANGE) {
                return bad(format!(
                    "band {n}: frequency {} Hz out of range {:?}",
                    b.freq, FREQ_RANGE
                ));
            }
            if !in_range(b.q, Q_RANGE) {
                return bad(format!("band {n}: Q {} out of range {:?}", b.q, Q_RANGE));
            }
            if !in_range(b.gain, GAIN_RANGE) {
                return bad(format!(
                    "band {n}: gain {} dB out of range {:?}",
                    b.gain, GAIN_RANGE
                ));
            }
        }
        Ok(())
    }

    /// A preset that doesn't change the signal needs no filter at all.
    pub fn is_flat(&self) -> bool {
        self.preamp.abs() < 0.001
            && self
                .bands
                .iter()
                .all(|b| b.filter_type.uses_gain() && b.gain.abs() < 0.001)
    }

    fn parse(id: &str, text: &str) -> Result<Self, PresetError> {
        let mut preset: EqPreset = toml::from_str(text).map_err(|e| PresetError::Parse {
            id: id.to_string(),
            err: e.message().to_string(),
        })?;
        preset.id = id.to_string();
        preset.validate()?;
        Ok(preset)
    }

    /// Load a preset by id, honouring the user → system → built-in order.
    pub fn load(id: &str) -> Result<(Self, Source), PresetError> {
        if !is_safe_id(id) {
            return Err(PresetError::InvalidId(id.to_string()));
        }
        let file = format!("{id}.toml");
        let dirs = std::iter::once((config::eq_presets_dir(), Source::User)).chain(
            SYSTEM_DIRS
                .iter()
                .map(|d| (PathBuf::from(d), Source::System)),
        );
        for (dir, source) in dirs {
            match std::fs::read_to_string(dir.join(&file)) {
                Ok(text) => return Self::parse(id, &text).map(|p| (p, source)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            }
        }
        match BUILTIN.iter().find(|(b, _)| *b == id) {
            Some((_, text)) => Self::parse(id, text).map(|p| (p, Source::Builtin)),
            None => Err(PresetError::NotFound(id.to_string())),
        }
    }

    /// All loadable presets, sorted by id. Unparseable files are skipped with
    /// a warning so one bad file can't hide the rest.
    pub fn list() -> Vec<(Self, Source)> {
        let mut ids: Vec<String> = BUILTIN.iter().map(|(id, _)| id.to_string()).collect();
        let dirs =
            std::iter::once(config::eq_presets_dir()).chain(SYSTEM_DIRS.iter().map(PathBuf::from));
        for dir in dirs {
            ids.extend(toml_stems(&dir));
        }
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| {
                Self::load(&id)
                    .inspect_err(|e| warn!("skipping EQ preset: {e}"))
                    .ok()
            })
            .collect()
    }

    /// Write this preset to the user preset dir, atomically.
    pub fn save(&self) -> Result<PathBuf, PresetError> {
        validate_new_id(&self.id)?;
        self.validate()?;
        let dir = config::eq_presets_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.toml", self.id));
        let text = toml::to_string_pretty(self).map_err(|e| PresetError::Invalid(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, format!("# airpods-helper EQ preset\n{text}"))?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// Delete a user preset. Built-in / system presets can't be deleted.
    pub fn delete(id: &str) -> Result<(), PresetError> {
        if !is_safe_id(id) {
            return Err(PresetError::InvalidId(id.to_string()));
        }
        let path = config::eq_presets_dir().join(format!("{id}.toml"));
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if Self::load(id).is_ok() {
                    Err(PresetError::NotDeletable(id.to_string()))
                } else {
                    Err(PresetError::NotFound(id.to_string()))
                }
            }
            Err(e) => Err(e.into()),
        }
    }
}

fn toml_stems(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            (path.extension()? == "toml")
                .then(|| path.file_stem()?.to_str().map(str::to_string))
                .flatten()
        })
        .filter(|id| is_safe_id(id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse_and_validate() {
        for (id, text) in BUILTIN {
            let p = EqPreset::parse(id, text).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(p.id, *id);
        }
    }

    #[test]
    fn flat_is_flat() {
        let (p, src) = EqPreset::load("flat").unwrap();
        assert!(p.is_flat());
        assert!(src == Source::Builtin || src == Source::User || src == Source::System);
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(matches!(
            EqPreset::load("../../etc/passwd"),
            Err(PresetError::InvalidId(_))
        ));
        assert!(matches!(
            EqPreset::load(".hidden"),
            Err(PresetError::InvalidId(_))
        ));
        assert!(validate_new_id("My Preset").is_err());
        assert!(validate_new_id("my-preset-2").is_ok());
    }

    #[test]
    fn validation_limits() {
        let mut p = EqPreset {
            id: "t".into(),
            name: "T".into(),
            description: String::new(),
            preamp: 0.0,
            bands: vec![EqBand {
                filter_type: FilterType::Peaking,
                freq: 1000.0,
                q: 1.0,
                gain: 3.0,
            }],
        };
        assert!(p.validate().is_ok());
        p.bands[0].freq = 5.0;
        assert!(p.validate().is_err());
        p.bands[0].freq = 1000.0;
        p.bands[0].gain = f64::NAN;
        assert!(p.validate().is_err());
        p.bands[0].gain = 3.0;
        p.preamp = 20.0;
        assert!(p.validate().is_err());
    }

    #[test]
    fn lowpass_is_never_flat() {
        let p = EqPreset {
            id: "t".into(),
            name: "T".into(),
            description: String::new(),
            preamp: 0.0,
            bands: vec![EqBand {
                filter_type: FilterType::Lowpass,
                freq: 8000.0,
                q: 0.7,
                gain: 0.0,
            }],
        };
        assert!(!p.is_flat());
    }

    #[test]
    fn filter_type_aliases() {
        assert_eq!(
            FilterType::parse("PK".to_lowercase().as_str()),
            Some(FilterType::Peaking)
        );
        assert_eq!(FilterType::parse("lsc"), Some(FilterType::Lowshelf));
        assert_eq!(FilterType::parse("bogus"), None);
    }
}
