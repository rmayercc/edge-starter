//! Example: a launcher, like a virtual Stream Deck. A grid of buttons that
//! open an app, a folder or a URL, run a macOS Shortcut or run a shell command.
//!
//! The buttons live outside the repo, in
//! `~/Library/Application Support/edge-starter/launcher.json`, and the card
//! reloads when that file changes (no rebuild). It's seeded on first run:
//!
//! ```json
//! { "buttons": [
//!   { "label": "Safari",  "app": "Safari" },
//!   { "label": "Projects", "folder": "~/Projects" },
//!   { "label": "Mail",    "url": "https://outlook.office.com", "iconApp": "Microsoft Outlook" },
//!   { "label": "Focus",   "shortcut": "Start Focus", "icon": "⚡" },
//!   { "label": "Lock",    "shell": "pmset displaysleepnow", "icon": "/path/to/lock.png" }
//! ] }
//! ```
//!
//! Each button has a `label` and exactly one of `app` (a name or a path, as
//! for `open -a`), `folder` (a path, opened in Finder; `~/` works), `url`,
//! `shortcut` (as for `shortcuts run`) or `shell` (run with `zsh -lc`). The
//! icon is the app's or folder's real icon where there is one;
//! override it with `icon` (an emoji or short text, or a path to an image or
//! a .app) or `iconApp` (borrow another app's icon).
//!
//! Nothing here sends keystrokes or Apple Events to other apps, so it needs
//! no Accessibility or Automation permission.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

/// The event the page listens for to reload the card.
pub const CHANGED: &str = "launcher-changed";

const SEED: &str = r#"{
  "buttons": [
    { "label": "Outlook", "app": "Microsoft Outlook" },
    { "label": "Teams", "app": "Microsoft Teams" },
    { "label": "Safari", "app": "Safari" },
    { "label": "Finder", "app": "Finder" },
    { "label": "Calendar", "app": "Calendar" },
    { "label": "Shortcut", "shortcut": "Name Of Your Shortcut", "icon": "⚡" }
  ]
}
"#;

#[derive(Debug, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    #[serde(default)]
    pub label: String,
    pub app: Option<String>,
    pub folder: Option<String>,
    pub url: Option<String>,
    pub shortcut: Option<String>,
    pub shell: Option<String>,
    pub icon: Option<String>,
    pub icon_app: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Config {
    #[serde(default)]
    buttons: Vec<Button>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    App(String),
    Folder(String),
    Url(String),
    Shortcut(String),
    Shell(String),
}

impl Button {
    /// The one thing this button does.
    pub fn action(&self) -> Result<Action, String> {
        let set = |o: &Option<String>| o.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let mut all = [
            set(&self.app).map(Action::App),
            set(&self.folder).map(Action::Folder),
            set(&self.url).map(Action::Url),
            set(&self.shortcut).map(Action::Shortcut),
            set(&self.shell).map(Action::Shell),
        ]
        .into_iter()
        .flatten();
        match (all.next(), all.next()) {
            (Some(a), None) => Ok(a),
            (None, _) => Err("needs one of app, folder, url, shortcut or shell".into()),
            _ => Err("has more than one of app, folder, url, shortcut, shell".into()),
        }
    }
}

impl Action {
    pub fn kind(&self) -> &'static str {
        match self {
            Action::App(_) => "app",
            Action::Folder(_) => "folder",
            Action::Url(_) => "url",
            Action::Shortcut(_) => "shortcut",
            Action::Shell(_) => "shell",
        }
    }

    /// The program and arguments to run. Nothing goes through a shell except
    /// `shell` buttons, so names and URLs can't be read as extra commands.
    pub fn command(&self) -> Result<(&'static str, Vec<String>), String> {
        Ok(match self {
            Action::App(app) => ("/usr/bin/open", vec!["-a".into(), app.clone()]),
            Action::Folder(dir) => {
                let path = expand(dir);
                if !path.is_dir() {
                    return Err(format!("No folder at {dir}"));
                }
                ("/usr/bin/open", vec![path.to_string_lossy().into_owned()])
            }
            Action::Url(u) => {
                url::Url::parse(u).map_err(|_| format!("not a URL: {u}"))?;
                ("/usr/bin/open", vec![u.clone()])
            }
            Action::Shortcut(name) => ("/usr/bin/shortcuts", vec!["run".into(), name.clone()]),
            Action::Shell(cmd) => ("/bin/zsh", vec!["-lc".into(), cmd.clone()]),
        })
    }
}

pub fn parse(text: &str) -> Result<Vec<Button>, String> {
    serde_json::from_str::<Config>(text).map(|c| c.buttons).map_err(|e| format!("launcher.json: {e}"))
}

pub fn config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("edge-starter/launcher.json")
}

/// One button as the page draws it.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ButtonView {
    pub label: String,
    /// "app" | "url" | "shortcut" | "shell", or None if the button is broken.
    pub kind: Option<&'static str>,
    /// The icon as a data URL…
    pub image: Option<String>,
    /// …or an emoji / short text to show instead.
    pub glyph: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// Pass this back to `run` so a tap can't hit a button that has moved.
    pub rev: u64,
    pub buttons: Vec<ButtonView>,
    pub error: Option<String>,
    pub path: String,
}

struct State {
    rev: u64,
    buttons: Vec<Button>,
}
static STATE: Mutex<State> = Mutex::new(State { rev: 0, buttons: Vec::new() });
static ICONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn data_url(mime: &str, bytes: &[u8]) -> String {
    use base64::Engine;
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    }
}

/// Where an app lives: a path as given, else ask macOS, else the usual places.
fn app_path(app: &str) -> Option<String> {
    if app.starts_with('/') || app.starts_with("~/") {
        let p = expand(app);
        return p.exists().then(|| p.to_string_lossy().into_owned());
    }
    if let Some(p) = mac::find_app(app) {
        return Some(p);
    }
    let name = if app.ends_with(".app") { app.to_string() } else { format!("{app}.app") };
    let home = dirs::home_dir().unwrap_or_default();
    [Path::new("/Applications"), Path::new("/System/Applications"), Path::new("/System/Applications/Utilities"), Path::new("/System/Library/CoreServices"), &home.join("Applications")]
        .iter()
        .map(|dir| dir.join(&name))
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

/// An app's real icon as a PNG data URL.
fn app_icon(app: &str) -> Option<String> {
    file_icon(&app_path(app)?)
}

/// The icon Finder shows for a file, folder or app, as a PNG data URL (kept
/// in memory once made).
fn file_icon(path: &str) -> Option<String> {
    let path = path.to_string();
    let mut cache = ICONS.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(hit) = cache.get(&path) {
        return hit.clone();
    }
    let icon = mac::icon_png(&path).map(|png| data_url("image/png", &png));
    cache.insert(path, icon.clone());
    icon
}

/// (image, glyph) for a button.
fn icon_for(b: &Button, action: Option<&Action>) -> (Option<String>, Option<String>) {
    if let Some(icon) = b.icon.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !(icon.starts_with('/') || icon.starts_with("~/")) {
            return (None, Some(icon.chars().take(6).collect()));
        }
        let path = expand(icon);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let mime = match ext.as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "webp" => Some("image/webp"),
            "svg" => Some("image/svg+xml"),
            _ => None,
        };
        let image = match mime {
            Some(mime) => std::fs::read(&path).ok().map(|bytes| data_url(mime, &bytes)),
            None => app_icon(icon),
        };
        if image.is_some() {
            return (image, None);
        }
    }
    let from_app = match (&b.icon_app, action) {
        (Some(app), _) => app_icon(app),
        (None, Some(Action::App(app))) => app_icon(app),
        (None, Some(Action::Folder(dir))) => {
            let path = expand(dir);
            if path.is_dir() { file_icon(&path.to_string_lossy()) } else { None }
        }
        (None, Some(Action::Shortcut(_))) => app_icon("Shortcuts"),
        _ => None,
    };
    if from_app.is_some() {
        return (from_app, None);
    }
    let glyph = match action {
        Some(Action::Url(_)) => "↗",
        Some(Action::Folder(_)) => "📁",
        Some(Action::Shell(_)) => "›_",
        Some(Action::Shortcut(_)) => "⚡",
        _ => "?",
    };
    (None, Some(glyph.into()))
}

/// Read launcher.json (writing the seed on first run) and describe the
/// buttons for the page. A broken file keeps the last good buttons.
pub fn load() -> View {
    let path = config_path();
    if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&path, SEED) {
            Ok(()) => crate::log(&format!("launcher: wrote starter buttons to {}", path.display())),
            Err(e) => crate::log(&format!("launcher: couldn't write {}: {e}", path.display())),
        }
    }
    let parsed = std::fs::read_to_string(&path).map_err(|e| format!("launcher.json: {e}")).and_then(|t| parse(&t));
    let mut state = STATE.lock().unwrap();
    state.rev += 1;
    let error = match parsed {
        Ok(buttons) => {
            state.buttons = buttons;
            None
        }
        Err(e) => {
            crate::log(&format!("launcher: {e}"));
            Some(e)
        }
    };
    let buttons = state
        .buttons
        .iter()
        .map(|b| {
            let action = b.action();
            let (image, glyph) = icon_for(b, action.as_ref().ok());
            ButtonView { label: b.label.clone(), kind: action.as_ref().ok().map(Action::kind), image, glyph, error: action.err() }
        })
        .collect();
    View { rev: state.rev, buttons, error, path: path.to_string_lossy().into_owned() }
}

/// Run button `index` of the buttons last sent to the page as `rev`.
pub fn run(rev: u64, index: usize) -> Result<(), String> {
    let button = {
        let state = STATE.lock().unwrap();
        if state.rev != rev {
            return Err("The buttons just changed. Tap again.".into());
        }
        state.buttons.get(index).cloned().ok_or("No such button")?
    };
    let (program, args) = button.action()?.command()?;
    let mut child = Command::new(program)
        .args(&args)
        .current_dir(dirs::home_dir().unwrap_or_else(|| "/".into()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    // Give quick failures ("no such app") a moment to show up on the button;
    // anything still running after that carries on in the background.
    for _ in 0..12 {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let label = button.label.clone();
    let finish = move |child: std::process::Child| -> Result<(), String> {
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let why = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("it failed").to_string();
        crate::log(&format!("launcher: \"{label}\" failed ({}): {why}", out.status));
        Err(why)
    };
    if child.try_wait().map_err(|e| e.to_string())?.is_some() {
        return finish(child);
    }
    std::thread::spawn(move || finish(child));
    Ok(())
}

/// Watch launcher.json and tell the page when it changes.
pub fn start() {
    std::thread::spawn(|| {
        let stamp = || std::fs::metadata(config_path()).ok().map(|m| (m.modified().ok(), m.len()));
        let mut last = stamp();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = stamp();
            if now != last {
                last = now;
                crate::emit(CHANGED);
            }
        }
    });
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, Encode, Encoding, RefEncode};
    use objc2_foundation::{NSData, NSDictionary, NSString};

    /// Icons are drawn at up to ~110px on the Edge; 256 keeps them sharp.
    const ICON_PX: f64 = 256.0;

    #[repr(C)]
    struct Size(f64, f64);
    unsafe impl Encode for Size {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    #[repr(C)]
    struct Rect([f64; 4]);
    unsafe impl RefEncode for Rect {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct(
            "CGRect",
            &[Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]), Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING])],
        ));
    }
    #[repr(C)]
    struct CGImage([u8; 0]);
    unsafe impl RefEncode for CGImage {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
    }

    /// "Safari" -> "/Applications/Safari.app", as Launch Services sees it.
    pub fn find_app(name: &str) -> Option<String> {
        unsafe {
            let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
            let path: Option<Retained<NSString>> = msg_send![&*ws, fullPathForApplication: &*NSString::from_str(name)];
            path.map(|p| p.to_string())
        }
    }

    /// The icon Finder shows for `path`, as PNG bytes.
    pub fn icon_png(path: &str) -> Option<Vec<u8>> {
        unsafe {
            let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
            let image: Option<Retained<AnyObject>> = msg_send![&*ws, iconForFile: &*NSString::from_str(path)];
            let image = image?;
            let _: () = msg_send![&*image, setSize: Size(ICON_PX, ICON_PX)];
            let cg: *mut CGImage = msg_send![&*image, CGImageForProposedRect: std::ptr::null_mut::<Rect>(), context: std::ptr::null::<AnyObject>(), hints: std::ptr::null::<AnyObject>()];
            if cg.is_null() {
                return None;
            }
            let alloc: Allocated<AnyObject> = msg_send![class!(NSBitmapImageRep), alloc];
            let rep: Option<Retained<AnyObject>> = msg_send![alloc, initWithCGImage: cg];
            let props = NSDictionary::<NSString, AnyObject>::new();
            // 4 = NSBitmapImageFileTypePNG
            let png: Option<Retained<NSData>> = msg_send![&*rep?, representationUsingType: 4usize, properties: &*props];
            png.map(|d| d.to_vec())
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    pub fn find_app(_: &str) -> Option<String> {
        None
    }
    pub fn icon_png(_: &str) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_valid() {
        let buttons = parse(SEED).unwrap();
        assert_eq!(buttons.len(), 6);
        assert!(buttons.iter().all(|b| b.action().is_ok()));
        assert_eq!(buttons[5].action().unwrap().kind(), "shortcut");
    }

    #[test]
    fn one_action_per_button() {
        let b = &parse(r#"{"buttons":[{"label":"x"},{"label":"y","app":"Safari","url":"https://example.com"},{"label":"z","app":" "}]}"#).unwrap();
        assert!(b.iter().all(|b| b.action().is_err()));
        assert!(parse("{ nope").is_err());
    }

    #[test]
    fn commands_never_go_through_a_shell_by_accident() {
        let app = Action::App("Safari; rm -rf ~".into()).command().unwrap();
        assert_eq!(app, ("/usr/bin/open", vec!["-a".to_string(), "Safari; rm -rf ~".to_string()]));
        assert_eq!(Action::Shortcut("My Flow".into()).command().unwrap().1, ["run", "My Flow"]);
        assert!(Action::Url("--help".into()).command().is_err());
        assert_eq!(Action::Url("https://example.com/a b".into()).command().unwrap().0, "/usr/bin/open");
        assert_eq!(Action::Shell("echo hi".into()).command().unwrap(), ("/bin/zsh", vec!["-lc".to_string(), "echo hi".to_string()]));
    }

    #[test]
    fn folders_open_as_one_argument() {
        let dir = std::env::temp_dir().join("edge starter test; echo hi");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.to_string_lossy().into_owned();
        assert_eq!(Action::Folder(path.clone()).command().unwrap(), ("/usr/bin/open", vec![path.clone()]));
        assert!(Action::Folder(format!("{path}/missing")).command().is_err());
        let b = &parse(&format!(r#"{{"buttons":[{{"label":"x","folder":{}}}]}}"#, serde_json::to_string(&path).unwrap())).unwrap()[0];
        assert_eq!(b.action().unwrap().kind(), "folder");
        let _ = std::fs::remove_dir(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finds_apps_and_their_icons() {
        let finder = app_path("Finder").expect("Finder");
        assert!(finder.ends_with("Finder.app"), "{finder}");
        let png = mac::icon_png(&finder).expect("icon");
        assert!(png.starts_with(b"\x89PNG"));
        assert!(app_icon("Finder").unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(app_path("No Such App 12345"), None);
    }
}
