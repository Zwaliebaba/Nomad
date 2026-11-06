//! Command-line options.

use std::ffi::OsString;
use std::path::PathBuf;

pub const USAGE: &str = "\
usage: nomad-client [--size WxH] [--view N] [--zoom Z]
       nomad-client --screenshot OUT.png [--size WxH] [--view N] [--zoom Z]
                    [--hover X,Y] [--turn-ms MS]

  --screenshot OUT.png  render one frame offscreen (no window) and write it as a PNG
  --size WxH            window or image size in pixels (default 1920x1080)
  --view N              initial view 0-3 (default 0)
  --zoom Z              initial zoom, one of the zoom levels in scene_config.json (default 1)
  --hover X,Y           screenshot only: as if the cursor were at pixel X,Y (hover highlight
                        and ELEV tag)
  --turn-ms MS          screenshot only: render MS milliseconds into a +90° turn (E) from
                        --view, 0-250

environment:
  NOMAD_ASSETS          asset root (default: Design/phase1-visual-slice/assets in this workspace)
  RUST_LOG              log filter, for example `info` or `nomad_client=debug`";

/// Default window and screenshot size (phase-1 doc 06, M1).
pub const DEFAULT_SIZE: (u32, u32) = (1920, 1080);
const MAX_SIZE: u32 = 16_384;
/// Length of a snap turn in milliseconds (doc 02), the limit of `--turn-ms`.
const TURN_MS: u32 = 250;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Render offscreen to this PNG instead of opening a window.
    pub screenshot: Option<PathBuf>,
    pub size: (u32, u32),
    pub view: u8,
    pub zoom: Option<f32>,
    /// Screenshot only: a simulated cursor position in pixels.
    pub hover: Option<(f32, f32)>,
    /// Screenshot only: milliseconds into a +90° turn from `view`.
    pub turn_ms: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            screenshot: None,
            size: DEFAULT_SIZE,
            view: 0,
            zoom: None,
            hover: None,
            turn_ms: None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Run(Options),
    Help,
}

/// Parses the arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let flag = flag
            .into_string()
            .map_err(|raw| format!("unknown argument {}", raw.display()))?;
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--screenshot" => options.screenshot = Some(PathBuf::from(value()?)),
            "--size" => options.size = parse_size(&utf8(value()?)?)?,
            "--view" => {
                let text = utf8(value()?)?;
                options.view = text
                    .parse()
                    .ok()
                    .filter(|v| *v < 4)
                    .ok_or_else(|| format!("--view must be 0, 1, 2 or 3, got {text:?}"))?;
            }
            "--zoom" => {
                let text = utf8(value()?)?;
                options.zoom = Some(
                    text.parse::<f32>()
                        .ok()
                        .filter(|z| z.is_finite() && *z > 0.0)
                        .ok_or_else(|| format!("--zoom needs a positive number, got {text:?}"))?,
                );
            }
            "--hover" => options.hover = Some(parse_point(&utf8(value()?)?)?),
            "--turn-ms" => {
                let text = utf8(value()?)?;
                options.turn_ms = Some(
                    text.parse()
                        .ok()
                        .filter(|ms| *ms <= TURN_MS)
                        .ok_or_else(|| format!("--turn-ms must be 0 to {TURN_MS}, got {text:?}"))?,
                );
            }
            "--help" | "-h" => return Ok(Command::Help),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if options.screenshot.is_none() && (options.hover.is_some() || options.turn_ms.is_some()) {
        return Err("--hover and --turn-ms only apply to --screenshot".into());
    }
    Ok(Command::Run(options))
}

fn parse_point(text: &str) -> Result<(f32, f32), String> {
    let invalid = || format!("--hover needs X,Y in pixels, for example 700,420, got {text:?}");
    let (x, y) = text.split_once(',').ok_or_else(invalid)?;
    let coordinate = |v: &str| {
        v.trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0 && *v <= MAX_SIZE as f32)
            .ok_or_else(invalid)
    };
    Ok((coordinate(x)?, coordinate(y)?))
}

fn utf8(value: OsString) -> Result<String, String> {
    value
        .into_string()
        .map_err(|raw| format!("not valid UTF-8: {}", raw.display()))
}

fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let invalid = || format!("--size needs WIDTHxHEIGHT, for example 1920x1080, got {text:?}");
    let (w, h) = text.split_once(['x', 'X']).ok_or_else(invalid)?;
    let w: u32 = w.parse().map_err(|_| invalid())?;
    let h: u32 = h.parse().map_err(|_| invalid())?;
    if w == 0 || h == 0 || w > MAX_SIZE || h > MAX_SIZE {
        return Err(format!(
            "--size must be between 1 and {MAX_SIZE} on each side"
        ));
    }
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn defaults_open_a_window() {
        assert_eq!(run(&[]), Ok(Command::Run(Options::default())));
    }

    #[test]
    fn parses_screenshot_options() {
        let parsed = run(&[
            "--screenshot",
            "out/v1.png",
            "--view",
            "1",
            "--zoom",
            "0.5",
            "--size",
            "1280x720",
            "--hover",
            "700,420.5",
            "--turn-ms",
            "125",
        ]);
        assert_eq!(
            parsed,
            Ok(Command::Run(Options {
                screenshot: Some(PathBuf::from("out/v1.png")),
                size: (1280, 720),
                view: 1,
                zoom: Some(0.5),
                hover: Some((700.0, 420.5)),
                turn_ms: Some(125),
            }))
        );
    }

    #[test]
    fn rejects_bad_input() {
        for args in [
            &["--view", "4"][..],
            &["--view"],
            &["--zoom", "0"],
            &["--zoom", "NaN"],
            &["--size", "1920"],
            &["--size", "0x10"],
            &["--size", "99999x10"],
            &["--fullscreen"],
            &["--screenshot", "a.png", "--hover", "700"],
            &["--screenshot", "a.png", "--hover", "-1,5"],
            &["--screenshot", "a.png", "--hover", "1,inf"],
            &["--screenshot", "a.png", "--turn-ms", "251"],
            &["--hover", "700,420"],
            &["--turn-ms", "100"],
        ] {
            assert!(run(args).is_err(), "{args:?} should be rejected");
        }
    }

    #[test]
    fn help() {
        assert_eq!(run(&["--help"]), Ok(Command::Help));
    }
}
