//! Game assets other than configuration (phase-1 doc 05, `assets/`). Paths and the error type are
//! shared with [`crate::config`].

pub mod fonts;
pub mod voxel;

use std::path::Path;

use glyphon::fontdb;

use crate::config::{self, ConfigError, Fixture, FontRole, HudLayout, SceneConfig};
use voxel::StationScene;

/// Everything the renderer is built from, loaded once at startup.
pub struct SceneAssets {
    pub config: SceneConfig,
    pub station: StationScene,
    pub hud: HudLayout,
    /// The phase-1 data feed (D-028): so far only the shield pylons' strengths (M4).
    pub fixture: Fixture,
    /// The fonts found under `<asset root>/fonts/`; possibly none (they are not bundled).
    pub fonts: fontdb::Database,
}

impl SceneAssets {
    /// Loads the configuration, station, HUD layout and fixture under `asset_root`, and
    /// whatever fonts are installed. Missing fonts are a warning, not an error. Every shield
    /// sector's pylon must have a strength in the fixture.
    pub fn load(asset_root: &Path) -> Result<Self, ConfigError> {
        let config_path = config::scene_config_path(asset_root);
        tracing::info!(path = %config_path.display(), "loading scene config");
        let config = config::load_scene_config(&config_path)?;
        let station_path = config::station_scene_path(asset_root);
        tracing::info!(path = %station_path.display(), "loading station scene");
        let station = voxel::load_station_scene(&station_path)?;
        tracing::info!(
            emissive_voxels = station.emissive_voxel_count(),
            "station loaded"
        );
        let hud_path = config::hud_layout_path(asset_root);
        tracing::info!(path = %hud_path.display(), "loading HUD layout");
        let hud = config::load_hud_layout(&hud_path)?;
        let fixture_path = config::fixture_path(asset_root);
        tracing::info!(path = %fixture_path.display(), "loading fixture");
        let fixture = config::load_fixture(&fixture_path)?;
        if let Some(sector) = config
            .shield
            .sectors
            .iter()
            .find(|s| fixture.pylon_strength(&s.pylon).is_none())
        {
            return Err(ConfigError::Invalid {
                path: fixture_path,
                field: "selection.pylons",
                reason: format!(
                    "no strength for {}, which feeds a shield sector in scene_config.json",
                    sector.pylon
                ),
            });
        }
        let fonts_dir = config::fonts_dir(asset_root);
        if let Some(link) = config.shield.generator_links.iter().find(|link| {
            !station
                .modules
                .iter()
                .any(|module| module.id == link.generator)
        }) {
            return Err(ConfigError::Invalid {
                path: config_path,
                field: "shield.generator_links",
                reason: format!("unknown generator module {}", link.generator),
            });
        }
        let fonts = fonts::load_fonts(&fonts_dir);
        for role in [FontRole::Mono, FontRole::Display] {
            let family = hud.font_families.family(role);
            if !fonts::has_family(&fonts, family) {
                tracing::warn!(
                    family,
                    dir = %fonts_dir.display(),
                    "font not found; text that uses it is not drawn. The phase-1 fonts are OFL \
                     and not bundled: download JetBrains Mono and Rajdhani (.ttf) into this folder"
                );
            }
        }
        Ok(Self {
            config,
            station,
            hud,
            fixture,
            fonts,
        })
    }
}
