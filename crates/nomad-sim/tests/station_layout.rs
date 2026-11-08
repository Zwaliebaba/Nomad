//! The mother station in the rules file (D-104) copies its module bounds and launch point from
//! the phase-1 scene (`station_scene.json`, `ships_placement.json`; doc 10 §8). The scene is
//! frozen as a regression fixture (D-111, D-290). This preserves the simulation's agreed
//! scale and bounds independently of the new art assembly; it is not an active scene generator.

use std::collections::BTreeMap;
use std::path::PathBuf;

use nomad_sim::rules::Rules;
use serde_json::Value;

fn phase1_asset(relative: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../Design/phase1-visual-slice/assets")
        .join(relative);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn corner(value: &Value) -> [i32; 3] {
    let numbers: Vec<i32> = value
        .as_array()
        .expect("a corner is an array")
        .iter()
        .map(|n| i32::try_from(n.as_i64().expect("a corner holds integers")).expect("fits i32"))
        .collect();
    numbers.try_into().expect("a corner has three numbers")
}

#[test]
fn rules_modules_match_the_phase1_station_scene() {
    let scene = phase1_asset("voxels/station_scene.json");
    let bounds: BTreeMap<&str, ([i32; 3], [i32; 3])> = scene["modules"]
        .as_array()
        .expect("the scene lists modules")
        .iter()
        .map(|module| {
            (
                module["id"].as_str().expect("a module has an id"),
                (
                    corner(&module["bounds"]["min"]),
                    corner(&module["bounds"]["max"]),
                ),
            )
        })
        .collect();
    let rules = Rules::embedded();
    assert!(!rules.mother_station.modules.is_empty());
    for module in &rules.mother_station.modules {
        let Some(&(min, max)) = bounds.get(module.id.as_str()) else {
            panic!("rules module {} is not in station_scene.json", module.id);
        };
        assert_eq!(
            (module.min_voxels, module.max_voxels),
            (min, max),
            "bounds of {} differ from station_scene.json",
            module.id
        );
    }
}

#[test]
fn launch_point_is_the_cap_wing_centre() {
    let placement = phase1_asset("layout/ships_placement.json");
    let wing: Vec<(f64, f64)> = placement["placements"]
        .as_array()
        .expect("the placement lists ships")
        .iter()
        .filter(|ship| {
            ship["group"]
                .as_str()
                .is_some_and(|g| g.starts_with("CAP WING"))
        })
        .map(|ship| {
            let at = &ship["pos"];
            (
                at[0].as_f64().expect("x is a number"),
                at[1].as_f64().expect("y is a number"),
            )
        })
        .collect();
    assert!(
        !wing.is_empty(),
        "no CAP WING ships in ships_placement.json"
    );
    let n = wing.len() as f64;
    let centre = (
        wing.iter().map(|p| p.0).sum::<f64>() / n,
        wing.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let [x, y] = Rules::embedded().mother_station.launch_point_voxels;
    assert!(
        (x - centre.0).abs() <= 0.05 && (y - centre.1).abs() <= 0.05,
        "launch point ({x}, {y}) is not the CAP WING centre ({:.2}, {:.2})",
        centre.0,
        centre.1
    );
}
