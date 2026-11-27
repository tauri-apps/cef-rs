//! Load optional `settings.toml` for Vimium-style key bindings (see `resources/settings.default.toml`).
//!
//! Override the first-window URL with **`VMUX_STARTUP_URL`** (non-empty) when `open` is not passing env
//! — run `…/vmux.app/Contents/MacOS/vmux` from a shell with that variable set.

/// Default max parent nodes to walk when probing whether a focused DOM context allows typing.
pub const DEFAULT_DOM_FOCUS_ANCESTOR_WALK_MAX: usize = 64;

/// Default first tab URL when `[browser] startup_url` is missing or empty.
///
/// `http`/`https` values load directly (no leading `about:blank` in history). If this is empty,
/// vmux loads `about:blank` then the built-in default (see
/// [`OsrHostState::staged_initial_navigation_url`](crate::browser::cef::shell::OsrHostState::staged_initial_navigation_url)).
///
/// Default matches pass criteria (`open` does not pass env). Staged startup still loads
/// Direct load for `https`/`http` startup URLs (see `OsrHostState::staged_initial_navigation_url`).
pub const DEFAULT_STARTUP_URL: &str = "https://www.google.com";

use std::path::PathBuf;
use std::sync::Arc;

use bevy_app::{App, Plugin, Startup};
use bevy_ecs::prelude::Resource;
use bevy_ecs::schedule::{IntoSystemConfigs, SystemSet};
use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

/// Raw file format (all keys optional except via defaults).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SettingsFile {
    pub vimium: VimiumSettingsFile,
    pub browser: BrowserSettingsFile,
    pub window_manager: WindowManagerSettingsFile,
}

/// Tiling / stack window manager (`[window_manager]` in `settings.toml`).
///
/// Tmux-style prefix and global WM chords live under `[window_manager.tmux]` and
/// `[window_manager.tmux.keybindings]` (see [`WindowManagerTmuxFile`]).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WindowManagerSettingsFile {
    /// Default `tmux` (same as `manual`, BSP + split keys) or `dynamic` (auto dwindle).
    pub mode: String,
    /// Initial geometry: `tile` or `stack`.
    pub layout: String,
    /// Optional override: `bsp`, `dwindle`, or `grid`. Empty = derive from `mode`.
    pub tile_strategy: String,
    /// When true, dwindle recomputes equal splits on resize (handled in layout pass).
    pub balance_on_resize: bool,
    /// Default split axis for manual pending splits: `horizontal` or `vertical`.
    pub default_split_axis: String,
    #[serde(default)]
    pub tmux: WindowManagerTmuxFile,
}

impl Default for WindowManagerSettingsFile {
    fn default() -> Self {
        Self {
            mode: "tmux".to_string(),
            layout: "tile".to_string(),
            tile_strategy: String::new(),
            balance_on_resize: true,
            default_split_axis: "vertical".to_string(),
            tmux: WindowManagerTmuxFile::default(),
        }
    }
}

/// Tmux-aligned options and key chords (`[window_manager.tmux]` / `.keybindings`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WindowManagerTmuxFile {
    /// Matches tmux option `prefix` (vmux chord string, e.g. `ctrl+b`). Empty or `none` disables leader.
    pub prefix: String,
    /// Milliseconds to press the command key after prefix (vmux-specific; not tmux `repeat-time`).
    pub prefix_timeout_ms: u64,
    #[serde(default)]
    pub keybindings: WindowManagerTmuxKeybindingsFile,
}

impl Default for WindowManagerTmuxFile {
    fn default() -> Self {
        Self {
            prefix: "ctrl+b".to_string(),
            prefix_timeout_ms: 2000,
            keybindings: WindowManagerTmuxKeybindingsFile::default(),
        }
    }
}

/// Global WM chords using hyphenated names that mirror tmux commands (`[window_manager.tmux.keybindings]`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WindowManagerTmuxKeybindingsFile {
    #[serde(rename = "new-window")]
    pub new_window: String,
    #[serde(rename = "next-layout")]
    pub next_layout: String,
    #[serde(rename = "select-pane-next")]
    pub select_pane_next: String,
    #[serde(rename = "select-pane-previous")]
    pub select_pane_previous: String,
    #[serde(rename = "split-window-v")]
    pub split_window_v: String,
    #[serde(rename = "split-window-h")]
    pub split_window_h: String,
    #[serde(rename = "balance-panes")]
    pub balance_panes: String,
    #[serde(rename = "rotate-window")]
    pub rotate_window: String,
}

impl Default for WindowManagerTmuxKeybindingsFile {
    fn default() -> Self {
        Self {
            new_window: "cmd+n".to_string(),
            next_layout: "cmd+shift+t".to_string(),
            select_pane_next: "cmd+shift+bracketright".to_string(),
            select_pane_previous: "cmd+shift+bracketleft".to_string(),
            split_window_v: "cmd+ctrl+v".to_string(),
            split_window_h: "cmd+ctrl+s".to_string(),
            balance_panes: "cmd+ctrl+b".to_string(),
            rotate_window: "cmd+ctrl+r".to_string(),
        }
    }
}

/// General browser / DOM tuning (not key bindings).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BrowserSettingsFile {
    /// Max parent nodes to walk from the focused DOM node when deciding if typing is allowed.
    pub dom_focus_ancestor_walk_max: usize,
    /// Initial URL for the first window when the app starts. Empty uses [`DEFAULT_STARTUP_URL`].
    pub startup_url: String,
}

impl Default for BrowserSettingsFile {
    fn default() -> Self {
        Self {
            dom_focus_ancestor_walk_max: DEFAULT_DOM_FOCUS_ANCESTOR_WALK_MAX,
            startup_url: DEFAULT_STARTUP_URL.to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct VimiumSettingsFile {
    pub enabled: bool,
    /// Second `g` within this window (ms) counts as `gg` → scroll top.
    pub scroll_top_double_press_ms: u64,
    pub scroll_line_down: String,
    pub scroll_line_up: String,
    pub scroll_page_down: String,
    pub scroll_page_up: String,
    /// First key of `gg` (usually `g`). Empty disables double-g scroll top.
    pub scroll_top_prefix: String,
    pub scroll_bottom: String,
    pub history_back: String,
    pub history_forward: String,
    /// Letter-only reload (default **`shift+r`** / `R`). Use **Cmd+R** (macOS) or **Ctrl+R**
    /// elsewhere for Chrome-style reload (handled in `window::dispatch`, not here).
    pub reload: String,
    /// Link hints chord (default `f`). While hints are visible, the same key is fed to the page
    /// (hint letter), not a toggle — use Escape to cancel. Empty / `none` disables.
    pub hint_links: String,
    /// Pass keys to the page (Vimium insert). Empty / `none` disables.
    pub mode_insert: String,
    /// Open in-page find HUD (`/`). Empty / `none` disables.
    pub mode_find_open: String,
    /// Visual mode: `y` copies selection; Esc exits. Empty / `none` disables.
    pub mode_visual: String,
    pub find_next: String,
    pub find_prev: String,
    /// Copy page URL (clipboard). Empty / `none` disables.
    pub yank_url: String,
}

impl Default for VimiumSettingsFile {
    fn default() -> Self {
        Self {
            enabled: true,
            scroll_top_double_press_ms: 500,
            scroll_line_down: "j".to_string(),
            scroll_line_up: "k".to_string(),
            scroll_page_down: "d".to_string(),
            scroll_page_up: "u".to_string(),
            scroll_top_prefix: "g".to_string(),
            scroll_bottom: "shift+g".to_string(),
            history_back: "shift+h".to_string(),
            history_forward: "shift+l".to_string(),
            reload: "shift+r".to_string(),
            hint_links: "f".to_string(),
            mode_insert: "i".to_string(),
            mode_find_open: "/".to_string(),
            mode_visual: "v".to_string(),
            find_next: "n".to_string(),
            find_prev: "shift+n".to_string(),
            yank_url: "shift+y".to_string(),
        }
    }
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self {
            vimium: VimiumSettingsFile::default(),
            browser: BrowserSettingsFile::default(),
            window_manager: WindowManagerSettingsFile::default(),
        }
    }
}

/// High-level WM behavior from `[window_manager].mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WmMode {
    #[default]
    Dynamic,
    Manual,
}

/// Tile (non-overlapping) vs stack (same rect, focused on top).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WmGeometryLayout {
    #[default]
    Tile,
    Stack,
}

impl WmGeometryLayout {
    pub fn toggle(self) -> Self {
        match self {
            Self::Tile => Self::Stack,
            Self::Stack => Self::Tile,
        }
    }
}

/// Effective tiling algorithm when `layout = tile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WmTileStrategy {
    #[default]
    Dwindle,
    Bsp,
    Grid,
}

/// Parsed modifier + physical key (US layout–agnostic via `PhysicalKey` in winit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub cmd: bool,
    pub key: KeyCode,
}

impl KeyChord {
    #[inline]
    fn matches_physical_key(&self, physical: &winit::keyboard::PhysicalKey) -> bool {
        matches!(physical, winit::keyboard::PhysicalKey::Code(code) if *code == self.key)
    }

    #[inline]
    fn matches_modifiers(&self, mods: cef::sys::cef_event_flags_t) -> bool {
        use cef::sys::cef_event_flags_t as F;
        let mask = F::EVENTFLAG_SHIFT_DOWN.0
            | F::EVENTFLAG_CONTROL_DOWN.0
            | F::EVENTFLAG_ALT_DOWN.0
            | F::EVENTFLAG_COMMAND_DOWN.0;
        let actual = mods.0 & mask;
        let expected = (if self.shift {
            F::EVENTFLAG_SHIFT_DOWN.0
        } else {
            0
        }) | (if self.ctrl {
            F::EVENTFLAG_CONTROL_DOWN.0
        } else {
            0
        }) | (if self.alt { F::EVENTFLAG_ALT_DOWN.0 } else { 0 })
            | (if self.cmd {
                F::EVENTFLAG_COMMAND_DOWN.0
            } else {
                0
            });
        actual == expected
    }
}

/// Parsed vmux key bindings plus startup URL and DOM focus tuning from [`SettingsFile`].
#[derive(Debug, Clone)]
pub struct KeySettings {
    pub enabled: bool,
    /// See [`BrowserSettingsFile::dom_focus_ancestor_walk_max`].
    pub dom_focus_ancestor_walk_max: usize,
    /// See [`BrowserSettingsFile::startup_url`].
    pub startup_url: String,
    pub scroll_top_double_press_ms: u64,
    pub scroll_line_down: Option<KeyChord>,
    pub scroll_line_up: Option<KeyChord>,
    pub scroll_page_down: Option<KeyChord>,
    pub scroll_page_up: Option<KeyChord>,
    /// Key that arms `gg` (no modifiers).
    pub scroll_top_prefix: Option<KeyChord>,
    pub scroll_bottom: Option<KeyChord>,
    pub history_back: Option<KeyChord>,
    pub history_forward: Option<KeyChord>,
    pub reload: Option<KeyChord>,
    pub hint_links: Option<KeyChord>,
    pub mode_insert: Option<KeyChord>,
    pub mode_find_open: Option<KeyChord>,
    pub mode_visual: Option<KeyChord>,
    pub find_next: Option<KeyChord>,
    pub find_prev: Option<KeyChord>,
    pub yank_url: Option<KeyChord>,
    // --- window manager ([window_manager] / [window_manager.tmux].keybindings) ---
    pub wm_mode: WmMode,
    pub wm_initial_geometry_layout: WmGeometryLayout,
    pub wm_tile_strategy_override: Option<WmTileStrategy>,
    pub wm_balance_on_resize: bool,
    pub wm_default_split_axis_vertical: bool,
    pub wm_new_window: Option<KeyChord>,
    pub wm_toggle_layout: Option<KeyChord>,
    pub wm_focus_next: Option<KeyChord>,
    pub wm_focus_prev: Option<KeyChord>,
    pub wm_split_side_by_side: Option<KeyChord>,
    pub wm_split_stacked: Option<KeyChord>,
    pub wm_balance_windows: Option<KeyChord>,
    pub wm_rotate_split: Option<KeyChord>,
    /// Tmux-style leader (`ctrl+b`); `None` disables `"` / `%` / `c` / … sequences.
    pub wm_tmux_prefix: Option<KeyChord>,
    pub wm_tmux_prefix_timeout_ms: u64,
}

impl KeySettings {
    /// Resolved tiling strategy: override, or `bsp` for manual / `dwindle` for dynamic.
    #[inline]
    pub fn effective_tile_strategy(&self) -> WmTileStrategy {
        if let Some(s) = self.wm_tile_strategy_override {
            return s;
        }
        match self.wm_mode {
            WmMode::Manual => WmTileStrategy::Bsp,
            WmMode::Dynamic => WmTileStrategy::Dwindle,
        }
    }
}

#[derive(Resource, Clone)]
pub struct SettingsResource(pub Arc<KeySettings>);

/// Bevy [`Startup`] set: `load_settings_resource_system` runs here. Other plugins should schedule
/// work that needs [`SettingsResource`] **before** later `Startup` systems (e.g. [`crate::browser::cef::CefPlugin`]).
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct VmuxSettingsStartup;

/// Loads `settings.toml` on [`Startup`] and inserts [`SettingsResource`].
///
/// Same ordering idea as Bevy’s [`PreferencesPlugin`] in [`bevy_settings`]: load user settings
/// early and treat [`SettingsResource`] as the source of truth for anything that runs after
/// [`VmuxSettingsStartup`] (e.g. `.after(VmuxSettingsStartup)`).
///
/// [`PreferencesPlugin`]: https://github.com/bevyengine/bevy/blob/main/crates/bevy_settings/src/lib.rs
/// [`bevy_settings`]: https://github.com/bevyengine/bevy/tree/main/crates/bevy_settings
pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(Startup, VmuxSettingsStartup);
        app.add_systems(
            Startup,
            load_settings_resource_system.in_set(VmuxSettingsStartup),
        );
    }
}

fn load_settings_resource_system(mut commands: bevy_ecs::system::Commands) {
    let file = load_settings_file();
    commands.insert_resource(SettingsResource(Arc::new(key_settings_from_file(&file))));
}

fn parse_wm_mode(s: &str) -> WmMode {
    match s.trim().to_ascii_lowercase().as_str() {
        "manual" | "tmux" => WmMode::Manual,
        "dynamic" => WmMode::Dynamic,
        other => {
            eprintln!("vmux settings: unknown [window_manager] mode {other:?}, using dynamic");
            WmMode::Dynamic
        }
    }
}

fn parse_wm_geometry_layout(s: &str) -> WmGeometryLayout {
    match s.trim().to_ascii_lowercase().as_str() {
        "tile" => WmGeometryLayout::Tile,
        "stack" => WmGeometryLayout::Stack,
        other => {
            eprintln!("vmux settings: unknown [window_manager] layout {other:?}, using tile");
            WmGeometryLayout::Tile
        }
    }
}

fn parse_wm_tile_strategy(s: &str) -> Option<WmTileStrategy> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    Some(match t.to_ascii_lowercase().as_str() {
        "bsp" => WmTileStrategy::Bsp,
        "dwindle" => WmTileStrategy::Dwindle,
        "grid" => WmTileStrategy::Grid,
        other => {
            eprintln!("vmux settings: unknown [window_manager] tile_strategy {other:?}, ignoring");
            return None;
        }
    })
}

fn parse_wm_default_split_axis_vertical(s: &str) -> bool {
    match s.trim().to_ascii_lowercase().as_str() {
        "vertical" | "v" => true,
        "horizontal" | "h" => false,
        other => {
            eprintln!(
                "vmux settings: unknown [window_manager] default_split_axis {other:?}, using vertical"
            );
            true
        }
    }
}

fn key_settings_from_file(file: &SettingsFile) -> KeySettings {
    let v = &file.vimium;
    let wm = &file.window_manager;
    let wm_mode = parse_wm_mode(&wm.mode);
    let wm_initial_geometry_layout = parse_wm_geometry_layout(&wm.layout);
    let mut wm_tile_strategy_override = parse_wm_tile_strategy(&wm.tile_strategy);
    if wm_mode == WmMode::Manual && wm_tile_strategy_override == Some(WmTileStrategy::Dwindle) {
        eprintln!(
            "vmux settings: [window_manager] mode=manual with tile_strategy=dwindle is inconsistent; ignoring override"
        );
        wm_tile_strategy_override = None;
    }
    if wm_mode == WmMode::Dynamic && wm_tile_strategy_override == Some(WmTileStrategy::Bsp) {
        eprintln!(
            "vmux settings: [window_manager] mode=dynamic with tile_strategy=bsp is inconsistent; ignoring override"
        );
        wm_tile_strategy_override = None;
    }
    let startup_url = {
        let u = file.browser.startup_url.trim();
        let mut url = if u.is_empty() {
            DEFAULT_STARTUP_URL.to_string()
        } else {
            u.to_string()
        };
        if let Ok(env_url) = std::env::var("VMUX_STARTUP_URL") {
            let t = env_url.trim();
            if !t.is_empty() {
                url = t.to_string();
            }
        }
        url
    };
    KeySettings {
        enabled: v.enabled,
        dom_focus_ancestor_walk_max: file.browser.dom_focus_ancestor_walk_max.clamp(1, 512),
        startup_url,
        scroll_top_double_press_ms: v.scroll_top_double_press_ms.max(50),
        scroll_line_down: parse_chord_opt(&v.scroll_line_down),
        scroll_line_up: parse_chord_opt(&v.scroll_line_up),
        scroll_page_down: parse_chord_opt(&v.scroll_page_down),
        scroll_page_up: parse_chord_opt(&v.scroll_page_up),
        scroll_top_prefix: parse_chord_opt(&v.scroll_top_prefix),
        scroll_bottom: parse_chord_opt(&v.scroll_bottom),
        history_back: parse_chord_opt(&v.history_back),
        history_forward: parse_chord_opt(&v.history_forward),
        reload: parse_chord_opt(&v.reload),
        hint_links: parse_chord_opt(&v.hint_links),
        mode_insert: parse_chord_opt(&v.mode_insert),
        mode_find_open: parse_chord_opt(&v.mode_find_open),
        mode_visual: parse_chord_opt(&v.mode_visual),
        find_next: parse_chord_opt(&v.find_next),
        find_prev: parse_chord_opt(&v.find_prev),
        yank_url: parse_chord_opt(&v.yank_url),
        wm_mode,
        wm_initial_geometry_layout,
        wm_tile_strategy_override,
        wm_balance_on_resize: wm.balance_on_resize,
        wm_default_split_axis_vertical: parse_wm_default_split_axis_vertical(
            &wm.default_split_axis,
        ),
        wm_new_window: parse_chord_opt(&wm.tmux.keybindings.new_window),
        wm_toggle_layout: parse_chord_opt(&wm.tmux.keybindings.next_layout),
        wm_focus_next: parse_chord_opt(&wm.tmux.keybindings.select_pane_next),
        wm_focus_prev: parse_chord_opt(&wm.tmux.keybindings.select_pane_previous),
        wm_split_side_by_side: parse_chord_opt(&wm.tmux.keybindings.split_window_v),
        wm_split_stacked: parse_chord_opt(&wm.tmux.keybindings.split_window_h),
        wm_balance_windows: parse_chord_opt(&wm.tmux.keybindings.balance_panes),
        wm_rotate_split: parse_chord_opt(&wm.tmux.keybindings.rotate_window),
        wm_tmux_prefix: parse_chord_opt(&wm.tmux.prefix),
        wm_tmux_prefix_timeout_ms: wm.tmux.prefix_timeout_ms.clamp(100, 60_000),
    }
}

/// Test-only: build [`KeySettings`] from defaults with `[window_manager].mode` overridden.
#[cfg(test)]
pub(crate) fn key_settings_for_test_window_mode(mode: &str) -> KeySettings {
    let mut file = SettingsFile::default();
    file.window_manager.mode = mode.to_string();
    key_settings_from_file(&file)
}

fn parse_chord_opt(s: &str) -> Option<KeyChord> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return None;
    }
    match parse_key_chord(s) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("vmux settings: bad key chord {s:?}: {e}");
            None
        }
    }
}

fn parse_key_chord(s: &str) -> Result<KeyChord, String> {
    let mut shift = false;
    let mut ctrl = false;
    let mut alt = false;
    let mut cmd = false;
    let parts: Vec<&str> = s
        .split('+')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return Err("empty chord".into());
    }
    let key_name = *parts.last().ok_or("no key")?;
    for p in &parts[..parts.len() - 1] {
        match p.to_ascii_lowercase().as_str() {
            "shift" => shift = true,
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "cmd" | "command" | "meta" | "super" => cmd = true,
            other => return Err(format!("unknown modifier {other:?}")),
        }
    }
    let key = parse_key_code(key_name)?;
    Ok(KeyChord {
        shift,
        ctrl,
        alt,
        cmd,
        key,
    })
}

fn parse_key_code(name: &str) -> Result<KeyCode, String> {
    let n = name.trim().to_ascii_lowercase();
    let code = match n.as_str() {
        "a" => KeyCode::KeyA,
        "b" => KeyCode::KeyB,
        "c" => KeyCode::KeyC,
        "d" => KeyCode::KeyD,
        "e" => KeyCode::KeyE,
        "f" => KeyCode::KeyF,
        "g" => KeyCode::KeyG,
        "h" => KeyCode::KeyH,
        "i" => KeyCode::KeyI,
        "j" => KeyCode::KeyJ,
        "k" => KeyCode::KeyK,
        "l" => KeyCode::KeyL,
        "m" => KeyCode::KeyM,
        "n" => KeyCode::KeyN,
        "o" => KeyCode::KeyO,
        "p" => KeyCode::KeyP,
        "q" => KeyCode::KeyQ,
        "r" => KeyCode::KeyR,
        "s" => KeyCode::KeyS,
        "t" => KeyCode::KeyT,
        "u" => KeyCode::KeyU,
        "v" => KeyCode::KeyV,
        "w" => KeyCode::KeyW,
        "x" => KeyCode::KeyX,
        "y" => KeyCode::KeyY,
        "z" => KeyCode::KeyZ,
        "0" => KeyCode::Digit0,
        "1" => KeyCode::Digit1,
        "2" => KeyCode::Digit2,
        "3" => KeyCode::Digit3,
        "4" => KeyCode::Digit4,
        "5" => KeyCode::Digit5,
        "6" => KeyCode::Digit6,
        "7" => KeyCode::Digit7,
        "8" => KeyCode::Digit8,
        "9" => KeyCode::Digit9,
        "space" => KeyCode::Space,
        "escape" | "esc" => KeyCode::Escape,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "bracketleft" | "bracket_left" | "[" => KeyCode::BracketLeft,
        "bracketright" | "bracket_right" | "]" => KeyCode::BracketRight,
        "arrowleft" | "left" => KeyCode::ArrowLeft,
        "arrowright" | "right" => KeyCode::ArrowRight,
        "arrowup" | "up" => KeyCode::ArrowUp,
        "arrowdown" | "down" => KeyCode::ArrowDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "slash" | "/" => KeyCode::Slash,
        "quote" | "\"" => KeyCode::Quote,
        _ => return Err(format!("unknown key {name:?}")),
    };
    Ok(code)
}

pub fn chord_matches(
    chord: &KeyChord,
    mods: cef::sys::cef_event_flags_t,
    physical: &winit::keyboard::PhysicalKey,
) -> bool {
    chord.matches_physical_key(physical) && chord.matches_modifiers(mods)
}

pub fn chord_matches_winit(
    chord: &KeyChord,
    mods: winit::keyboard::ModifiersState,
    physical: &winit::keyboard::PhysicalKey,
) -> bool {
    chord.matches_physical_key(physical)
        && chord.shift == mods.shift_key()
        && chord.ctrl == mods.control_key()
        && chord.alt == mods.alt_key()
        && chord.cmd == mods.super_key()
}

#[inline]
fn winit_single_char_from_text_or_logical(
    text: Option<&str>,
    logical_key: &winit::keyboard::Key,
) -> Option<char> {
    if let Some(t) = text {
        let mut it = t.chars();
        if let Some(c) = it.next() {
            if it.next().is_none() {
                return Some(c);
            }
        }
    }
    match logical_key {
        winit::keyboard::Key::Character(s) => {
            let mut it = s.chars();
            let c = it.next()?;
            if it.next().is_none() { Some(c) } else { None }
        }
        _ => None,
    }
}

/// Like [`chord_matches_winit`], but when the physical key is wrong or [`PhysicalKey::Unidentified`]
/// (common on macOS while a web control has focus), match a single `KeyEvent::text` /
/// [`KeyEvent::logical_key`] character the same way as [`chord_matches_winit_ime_char`].
pub fn chord_matches_winit_or_text(
    chord: &KeyChord,
    mods: winit::keyboard::ModifiersState,
    physical: &winit::keyboard::PhysicalKey,
    text: Option<&str>,
    logical_key: &winit::keyboard::Key,
) -> bool {
    if chord_matches_winit(chord, mods, physical) {
        return true;
    }

    // Chords with no modifiers in settings (`f`, `j`, `/`, …): accept the committed character even
    // when `shift` disagrees (Caps Lock / platform quirks) or `physical` is Unidentified — the
    // strict block below would return false before reading `text`/`logical_key`. Do not treat
    // intentional `Shift`+letter as an unmodified chord unless the typed character is lowercase.
    if !chord.shift && !chord.ctrl && !chord.alt && !chord.cmd {
        if !mods.control_key() && !mods.alt_key() && !mods.super_key() {
            if let Some(c0) = winit_single_char_from_text_or_logical(text, logical_key) {
                let shift_ok_for_plain = !mods.shift_key() || c0.is_ascii_lowercase();
                if shift_ok_for_plain
                    && chord_matches_winit_ime_char(
                        chord,
                        winit::keyboard::ModifiersState::default(),
                        c0,
                    )
                {
                    return true;
                }
            }
        }
    }

    if chord.shift != mods.shift_key()
        || chord.ctrl != mods.control_key()
        || chord.alt != mods.alt_key()
        || chord.cmd != mods.super_key()
    {
        return false;
    }
    if chord.matches_physical_key(physical) {
        return false;
    }
    let Some(c0) = winit_single_char_from_text_or_logical(text, logical_key) else {
        return false;
    };
    chord_matches_winit_ime_char(chord, mods, c0)
}

#[derive(Clone, Copy)]
enum ImeKeyExpect {
    Letter(char),
    Char(char),
}

fn key_code_ime_expect(k: KeyCode) -> Option<ImeKeyExpect> {
    use KeyCode::*;
    match k {
        KeyA => Some(ImeKeyExpect::Letter('a')),
        KeyB => Some(ImeKeyExpect::Letter('b')),
        KeyC => Some(ImeKeyExpect::Letter('c')),
        KeyD => Some(ImeKeyExpect::Letter('d')),
        KeyE => Some(ImeKeyExpect::Letter('e')),
        KeyF => Some(ImeKeyExpect::Letter('f')),
        KeyG => Some(ImeKeyExpect::Letter('g')),
        KeyH => Some(ImeKeyExpect::Letter('h')),
        KeyI => Some(ImeKeyExpect::Letter('i')),
        KeyJ => Some(ImeKeyExpect::Letter('j')),
        KeyK => Some(ImeKeyExpect::Letter('k')),
        KeyL => Some(ImeKeyExpect::Letter('l')),
        KeyM => Some(ImeKeyExpect::Letter('m')),
        KeyN => Some(ImeKeyExpect::Letter('n')),
        KeyO => Some(ImeKeyExpect::Letter('o')),
        KeyP => Some(ImeKeyExpect::Letter('p')),
        KeyQ => Some(ImeKeyExpect::Letter('q')),
        KeyR => Some(ImeKeyExpect::Letter('r')),
        KeyS => Some(ImeKeyExpect::Letter('s')),
        KeyT => Some(ImeKeyExpect::Letter('t')),
        KeyU => Some(ImeKeyExpect::Letter('u')),
        KeyV => Some(ImeKeyExpect::Letter('v')),
        KeyW => Some(ImeKeyExpect::Letter('w')),
        KeyX => Some(ImeKeyExpect::Letter('x')),
        KeyY => Some(ImeKeyExpect::Letter('y')),
        KeyZ => Some(ImeKeyExpect::Letter('z')),
        Digit0 => Some(ImeKeyExpect::Char('0')),
        Digit1 => Some(ImeKeyExpect::Char('1')),
        Digit2 => Some(ImeKeyExpect::Char('2')),
        Digit3 => Some(ImeKeyExpect::Char('3')),
        Digit4 => Some(ImeKeyExpect::Char('4')),
        Digit5 => Some(ImeKeyExpect::Char('5')),
        Digit6 => Some(ImeKeyExpect::Char('6')),
        Digit7 => Some(ImeKeyExpect::Char('7')),
        Digit8 => Some(ImeKeyExpect::Char('8')),
        Digit9 => Some(ImeKeyExpect::Char('9')),
        Slash => Some(ImeKeyExpect::Char('/')),
        _ => None,
    }
}

/// macOS often delivers focused web controls' keys only as [`winit::event::Ime::Commit`], with no
/// [`WindowEvent::KeyboardInput`] for Vimium to intercept (e.g. Google consent language button).
pub fn chord_matches_winit_ime_char(
    chord: &KeyChord,
    mods: winit::keyboard::ModifiersState,
    committed: char,
) -> bool {
    if chord.shift != mods.shift_key()
        || chord.ctrl != mods.control_key()
        || chord.alt != mods.alt_key()
        || chord.cmd != mods.super_key()
    {
        return false;
    }
    let Some(expect) = key_code_ime_expect(chord.key) else {
        return false;
    };
    match expect {
        ImeKeyExpect::Letter(base) => {
            if chord.shift {
                committed == base.to_ascii_uppercase()
            } else {
                committed.to_ascii_lowercase() == base
            }
        }
        ImeKeyExpect::Char(ch) => committed == ch,
    }
}

fn settings_search_paths() -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("settings.toml"),
        PathBuf::from("Settings.toml"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(macos_dir) = exe.parent() {
            out.push(macos_dir.join("settings.toml"));
            // Bundled `.app`: `Contents/MacOS/vmux` → `Contents/Resources/settings.toml` (see `bundle-cef-app`).
            if let Some(contents) = macos_dir.parent() {
                out.push(contents.join("Resources").join("settings.toml"));
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        out.push(PathBuf::from(home).join(".config/vmux/settings.toml"));
    }
    out
}

/// Fallback when [`SettingsResource`] is missing (e.g. ordering); matches `SettingsFile::default()` parsing.
///
/// Only [`crate::browser::cef`] uses this (`pub(crate)` because Rust `pub(in path)` requires `path`
/// to be an ancestor module of `settings`, which it is not).
pub(crate) fn default_key_settings() -> Arc<KeySettings> {
    Arc::new(key_settings_from_file(&SettingsFile::default()))
}

fn load_settings_file() -> SettingsFile {
    for p in settings_search_paths() {
        if p.is_file() {
            match std::fs::read_to_string(&p) {
                Ok(text) => match toml::from_str::<SettingsFile>(&text) {
                    Ok(s) => {
                        bevy_log::info!(
                            target: "vmux",
                            pid = std::process::id(),
                            "settings: loaded {}",
                            p.display()
                        );
                        return s;
                    }
                    Err(e) => eprintln!("vmux: failed to parse {}: {e}", p.display()),
                },
                Err(e) => eprintln!("vmux: failed to read {}: {e}", p.display()),
            }
        }
    }
    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        "settings: no settings.toml found, using defaults"
    );
    SettingsFile::default()
}

#[cfg(test)]
use bevy_ecs::prelude::{Res, World};
#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;
#[cfg(test)]
use winit::keyboard::{Key, ModifiersState, NativeKeyCode, PhysicalKey};

#[cfg(test)]
#[derive(Resource)]
struct TomlCommentsOnlyIn(&'static str);

#[cfg(test)]
fn parse_comments_only_asserts_system(case: Res<TomlCommentsOnlyIn>) {
    let file: SettingsFile = toml::from_str(case.0).expect("comments-only settings");
    assert_eq!(file.browser.startup_url, DEFAULT_STARTUP_URL);
    assert_eq!(
        file.browser.dom_focus_ancestor_walk_max,
        DEFAULT_DOM_FOCUS_ANCESTOR_WALK_MAX
    );
}

#[cfg(test)]
#[test]
fn parse_comments_only_settings_toml_uses_defaults_via_system() {
    let mut world = World::default();
    world.insert_resource(TomlCommentsOnlyIn("# stub — no keys\n"));
    world
        .run_system_once(parse_comments_only_asserts_system)
        .unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseParseShiftG;

#[cfg(test)]
fn parse_shift_g_system(_: Res<CaseParseShiftG>) {
    let c = parse_key_chord("shift+g").unwrap();
    assert!(c.shift);
    assert_eq!(c.key, KeyCode::KeyG);
}

#[cfg(test)]
#[test]
fn parse_shift_g_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseParseShiftG);
    world.run_system_once(parse_shift_g_system).unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseParseShiftR;

#[cfg(test)]
fn parse_shift_r_system(_: Res<CaseParseShiftR>) {
    let c = parse_key_chord("shift+r").unwrap();
    assert!(c.shift);
    assert_eq!(c.key, KeyCode::KeyR);
}

#[cfg(test)]
#[test]
fn parse_shift_r_default_vimium_reload_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseParseShiftR);
    world.run_system_once(parse_shift_r_system).unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseParseJ;

#[cfg(test)]
fn parse_j_system(_: Res<CaseParseJ>) {
    let c = parse_key_chord("j").unwrap();
    assert!(!c.shift);
    assert_eq!(c.key, KeyCode::KeyJ);
}

#[cfg(test)]
#[test]
fn parse_j_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseParseJ);
    world.run_system_once(parse_j_system).unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseParseWmToggle;

#[cfg(test)]
fn parse_wm_toggle_layout_chord_system(_: Res<CaseParseWmToggle>) {
    let c = parse_key_chord("cmd+shift+t").unwrap();
    assert!(c.cmd);
    assert!(c.shift);
    assert_eq!(c.key, KeyCode::KeyT);
}

#[cfg(test)]
#[test]
fn parse_wm_toggle_layout_chord_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseParseWmToggle);
    world
        .run_system_once(parse_wm_toggle_layout_chord_system)
        .unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseParseTmuxLeader;

#[cfg(test)]
fn parse_tmux_leader_ctrl_b_system(_: Res<CaseParseTmuxLeader>) {
    let c = parse_key_chord("ctrl+b").unwrap();
    assert!(c.ctrl);
    assert!(!c.cmd);
    assert_eq!(c.key, KeyCode::KeyB);
}

#[cfg(test)]
#[test]
fn parse_tmux_leader_ctrl_b_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseParseTmuxLeader);
    world
        .run_system_once(parse_tmux_leader_ctrl_b_system)
        .unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseParseQuote;

#[cfg(test)]
fn parse_quote_key_name_system(_: Res<CaseParseQuote>) {
    let c = parse_key_chord("shift+quote").unwrap();
    assert!(c.shift);
    assert_eq!(c.key, KeyCode::Quote);
}

#[cfg(test)]
#[test]
fn parse_quote_key_name_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseParseQuote);
    world.run_system_once(parse_quote_key_name_system).unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseParseWmFocusBrackets;

#[cfg(test)]
fn parse_wm_focus_brackets_system(_: Res<CaseParseWmFocusBrackets>) {
    let n = parse_key_chord("cmd+shift+bracketright").unwrap();
    assert!(n.cmd && n.shift);
    assert_eq!(n.key, KeyCode::BracketRight);
    let p = parse_key_chord("cmd+shift+bracketleft").unwrap();
    assert_eq!(p.key, KeyCode::BracketLeft);
}

#[cfg(test)]
#[test]
fn parse_wm_focus_brackets_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseParseWmFocusBrackets);
    world
        .run_system_once(parse_wm_focus_brackets_system)
        .unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseDeserializeNestedTmux;

#[cfg(test)]
fn deserialize_nested_window_manager_tmux_keybindings_system(_: Res<CaseDeserializeNestedTmux>) {
    let toml_str = r#"
[window_manager]
mode = "tmux"
layout = "tile"
tile_strategy = ""
balance_on_resize = true
default_split_axis = "vertical"

[window_manager.tmux]
prefix = "ctrl+b"
prefix_timeout_ms = 1500

[window_manager.tmux.keybindings]
new-window = "cmd+n"
next-layout = "cmd+shift+t"
select-pane-next = "cmd+shift+bracketright"
select-pane-previous = "cmd+shift+bracketleft"
split-window-v = "cmd+ctrl+v"
split-window-h = "cmd+ctrl+s"
balance-panes = "cmd+ctrl+b"
rotate-window = "cmd+ctrl+r"
"#;
    let file: SettingsFile = toml::from_str(toml_str).expect("parse nested window_manager.tmux");
    let ks = key_settings_from_file(&file);
    assert_eq!(ks.wm_tmux_prefix_timeout_ms, 1500);
    let prefix = ks.wm_tmux_prefix.as_ref().expect("prefix");
    assert!(prefix.ctrl);
    assert_eq!(prefix.key, KeyCode::KeyB);
    let nw = ks.wm_new_window.as_ref().expect("new-window");
    assert!(nw.cmd);
    assert_eq!(nw.key, KeyCode::KeyN);
    let tl = ks.wm_toggle_layout.as_ref().expect("next-layout");
    assert!(tl.cmd && tl.shift);
    assert_eq!(tl.key, KeyCode::KeyT);
    assert!(ks.wm_focus_next.is_some() && ks.wm_focus_prev.is_some());
    assert!(ks.wm_split_side_by_side.is_some() && ks.wm_split_stacked.is_some());
    assert!(ks.wm_balance_windows.is_some() && ks.wm_rotate_split.is_some());
}

#[cfg(test)]
#[test]
fn deserialize_nested_window_manager_tmux_keybindings_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseDeserializeNestedTmux);
    world
        .run_system_once(deserialize_nested_window_manager_tmux_keybindings_system)
        .unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseImeCharHintF;

#[cfg(test)]
fn ime_char_matches_hint_f_system(_: Res<CaseImeCharHintF>) {
    let chord = parse_key_chord("f").unwrap();
    let mods = ModifiersState::default();
    assert!(chord_matches_winit_ime_char(&chord, mods, 'f'));
    assert!(chord_matches_winit_ime_char(&chord, mods, 'F'));
    assert!(!chord_matches_winit_ime_char(&chord, mods, 'g'));
}

#[cfg(test)]
#[test]
fn ime_char_matches_hint_f_without_keyboard_event_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseImeCharHintF);
    world
        .run_system_once(ime_char_matches_hint_f_system)
        .unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseChordOrTextUnidentified;

#[cfg(test)]
fn chord_or_text_matches_f_unidentified_system(_: Res<CaseChordOrTextUnidentified>) {
    let chord = parse_key_chord("f").unwrap();
    let mods = ModifiersState::default();
    let phys = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
    let logical = Key::Character("f".into());
    assert!(chord_matches_winit_or_text(
        &chord,
        mods,
        &phys,
        Some("f"),
        &logical
    ));
    assert!(chord_matches_winit_or_text(
        &chord, mods, &phys, None, &logical
    ));
}

#[cfg(test)]
#[test]
fn chord_or_text_matches_f_when_physical_unidentified_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseChordOrTextUnidentified);
    world
        .run_system_once(chord_or_text_matches_f_unidentified_system)
        .unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseChordOrTextShiftSpurious;

#[cfg(test)]
fn chord_or_text_plain_f_shift_spurious_system(_: Res<CaseChordOrTextShiftSpurious>) {
    let chord = parse_key_chord("f").unwrap();
    let mut mods = ModifiersState::default();
    mods.set(ModifiersState::SHIFT, true);
    let phys = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
    let logical = Key::Character("f".into());
    assert!(chord_matches_winit_or_text(
        &chord,
        mods,
        &phys,
        Some("f"),
        &logical
    ));
}

#[cfg(test)]
#[test]
fn chord_or_text_matches_plain_f_when_shift_spurious_with_unidentified_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseChordOrTextShiftSpurious);
    world
        .run_system_once(chord_or_text_plain_f_shift_spurious_system)
        .unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseChordOrTextUppercaseF;

#[cfg(test)]
fn chord_or_text_no_match_uppercase_f_system(_: Res<CaseChordOrTextUppercaseF>) {
    let chord = parse_key_chord("f").unwrap();
    let mut mods = ModifiersState::default();
    mods.set(ModifiersState::SHIFT, true);
    let phys = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
    let logical = Key::Character("F".into());
    assert!(!chord_matches_winit_or_text(
        &chord,
        mods,
        &phys,
        Some("F"),
        &logical
    ));
}

#[cfg(test)]
#[test]
fn chord_or_text_does_not_match_shift_uppercase_f_as_plain_hint_chord_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseChordOrTextUppercaseF);
    world
        .run_system_once(chord_or_text_no_match_uppercase_f_system)
        .unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct CaseImeVimiumScrollModes;

#[cfg(test)]
fn ime_char_vimium_scroll_modes_system(_: Res<CaseImeVimiumScrollModes>) {
    let mods = ModifiersState::default();
    let j = parse_key_chord("j").unwrap();
    assert!(chord_matches_winit_ime_char(&j, mods, 'j'));

    let slash = parse_key_chord("/").unwrap();
    assert!(chord_matches_winit_ime_char(&slash, mods, '/'));

    let mut shift = ModifiersState::default();
    shift.set(ModifiersState::SHIFT, true);
    let shift_n = parse_key_chord("shift+n").unwrap();
    assert!(chord_matches_winit_ime_char(&shift_n, shift, 'N'));
}

#[cfg(test)]
#[test]
fn ime_char_matches_default_vimium_scroll_and_modes_via_system() {
    let mut world = World::default();
    world.insert_resource(CaseImeVimiumScrollModes);
    world
        .run_system_once(ime_char_vimium_scroll_modes_system)
        .unwrap();
}
