use cef::sys::cef_event_flags_t;
use cef::*;
use winit::event::ElementState;
use winit::keyboard::{Key, KeyCode, PhysicalKey};

pub fn update_mods_from_winit(m: winit::keyboard::ModifiersState) -> cef_event_flags_t {
    let mut flags = cef_event_flags_t::EVENTFLAG_NONE;
    if m.shift_key() {
        flags |= cef_event_flags_t::EVENTFLAG_SHIFT_DOWN;
    }
    if m.control_key() {
        flags |= cef_event_flags_t::EVENTFLAG_CONTROL_DOWN;
    }
    if m.alt_key() {
        flags |= cef_event_flags_t::EVENTFLAG_ALT_DOWN;
    }
    if m.super_key() {
        flags |= cef_event_flags_t::EVENTFLAG_COMMAND_DOWN;
    }
    flags
}

/// Winit may set `KeyEvent::text` to the same control characters we already send via keydown
/// (`Backspace`, `Enter`, …). Forwarding those again as [`KeyEventType::CHAR`] duplicates editor
/// actions (multiple deletes per keypress in `<input>`).
#[inline]
pub fn keyboard_text_char_duplicates_keydown(ch: char) -> bool {
    ch.is_control()
}

pub fn key_codes_from_physical(physical: &PhysicalKey) -> Option<(i32, i32, u16)> {
    // (windows_key_code, native_key_code, character_u16)
    let PhysicalKey::Code(code) = physical else {
        return None;
    };

    let (vk, native, ch) = match code {
        // Letters
        KeyCode::KeyA => (0x41, 0x00, b'a' as u16),
        KeyCode::KeyB => (0x42, 0x0B, b'b' as u16),
        KeyCode::KeyC => (0x43, 0x08, b'c' as u16),
        KeyCode::KeyD => (0x44, 0x02, b'd' as u16),
        KeyCode::KeyE => (0x45, 0x0E, b'e' as u16),
        KeyCode::KeyF => (0x46, 0x03, b'f' as u16),
        KeyCode::KeyG => (0x47, 0x05, b'g' as u16),
        KeyCode::KeyH => (0x48, 0x04, b'h' as u16),
        KeyCode::KeyI => (0x49, 0x22, b'i' as u16),
        KeyCode::KeyJ => (0x4A, 0x26, b'j' as u16),
        KeyCode::KeyK => (0x4B, 0x28, b'k' as u16),
        KeyCode::KeyL => (0x4C, 0x25, b'l' as u16),
        KeyCode::KeyM => (0x4D, 0x2E, b'm' as u16),
        KeyCode::KeyN => (0x4E, 0x2D, b'n' as u16),
        KeyCode::KeyO => (0x4F, 0x1F, b'o' as u16),
        KeyCode::KeyP => (0x50, 0x23, b'p' as u16),
        KeyCode::KeyQ => (0x51, 0x0C, b'q' as u16),
        KeyCode::KeyR => (0x52, 0x0F, b'r' as u16),
        KeyCode::KeyS => (0x53, 0x01, b's' as u16),
        KeyCode::KeyT => (0x54, 0x11, b't' as u16),
        KeyCode::KeyU => (0x55, 0x20, b'u' as u16),
        KeyCode::KeyV => (0x56, 0x09, b'v' as u16),
        KeyCode::KeyW => (0x57, 0x0D, b'w' as u16),
        KeyCode::KeyX => (0x58, 0x07, b'x' as u16),
        KeyCode::KeyY => (0x59, 0x10, b'y' as u16),
        KeyCode::KeyZ => (0x5A, 0x06, b'z' as u16),

        // Digits
        KeyCode::Digit0 => (0x30, 0x1D, b'0' as u16),
        KeyCode::Digit1 => (0x31, 0x12, b'1' as u16),
        KeyCode::Digit2 => (0x32, 0x13, b'2' as u16),
        KeyCode::Digit3 => (0x33, 0x14, b'3' as u16),
        KeyCode::Digit4 => (0x34, 0x15, b'4' as u16),
        KeyCode::Digit5 => (0x35, 0x17, b'5' as u16),
        KeyCode::Digit6 => (0x36, 0x16, b'6' as u16),
        KeyCode::Digit7 => (0x37, 0x1A, b'7' as u16),
        KeyCode::Digit8 => (0x38, 0x1C, b'8' as u16),
        KeyCode::Digit9 => (0x39, 0x19, b'9' as u16),

        KeyCode::Backspace => (0x08, 0x33, 0),
        KeyCode::Tab => (0x09, 0x30, 0),
        KeyCode::Enter => (0x0D, 0x24, b'\r' as u16),
        KeyCode::Escape => (0x1B, 0x35, 0),
        KeyCode::Space => (0x20, 0x31, b' ' as u16),
        KeyCode::ArrowLeft => (0x25, 0x7B, 0),
        KeyCode::ArrowUp => (0x26, 0x7E, 0),
        KeyCode::ArrowRight => (0x27, 0x7C, 0),
        KeyCode::ArrowDown => (0x28, 0x7D, 0),
        KeyCode::Delete => (0x2E, 0x75, 0),
        KeyCode::Home => (0x24, 0x73, 0),
        KeyCode::End => (0x23, 0x77, 0),
        KeyCode::PageUp => (0x21, 0x74, 0),
        KeyCode::PageDown => (0x22, 0x79, 0),
        _ => return None,
    };
    Some((vk, native, ch))
}

pub fn send_char(host: &BrowserHost, mods: cef_event_flags_t, ch: char) {
    let mut buf = [0u16; 2];
    let _n = ch.encode_utf16(&mut buf);
    let first = buf.get(0).copied().unwrap_or(0);
    let ev = KeyEvent {
        type_: KeyEventType::CHAR,
        modifiers: mods.0,
        windows_key_code: first as i32,
        native_key_code: 0,
        is_system_key: 0,
        character: first,
        unmodified_character: first,
        focus_on_editable_field: 1,
        ..Default::default()
    };
    host.send_key_event(Some(&ev));
}

pub fn send_key(
    host: &BrowserHost,
    type_: KeyEventType,
    mods: cef_event_flags_t,
    vk: i32,
    native: i32,
    ch: u16,
) {
    let ev = KeyEvent {
        type_,
        modifiers: mods.0,
        windows_key_code: vk,
        native_key_code: native,
        is_system_key: 0,
        character: ch,
        unmodified_character: ch,
        focus_on_editable_field: 1,
        ..Default::default()
    };
    host.send_key_event(Some(&ev));
}

pub fn send_key_press_and_release(
    host: &BrowserHost,
    mods: cef_event_flags_t,
    vk: i32,
    native: i32,
    ch: u16,
) {
    for type_ in [KeyEventType::RAWKEYDOWN, KeyEventType::KEYDOWN] {
        send_key(host, type_, mods, vk, native, ch);
    }
    send_key(host, KeyEventType::KEYUP, mods, vk, native, ch);
}

/// Letter key down with empty / missing [`winit::event::KeyEvent::text`].
///
/// macOS + web content often deliver printable characters only through [`winit::event::Ime`] after
/// this event. If we still treat the page as vimium-safe because the DOM probe missed the field, we
/// consume `d` / `g` / `r` as bindings while the user is typing (e.g. "ledger" → "lee").
pub fn physical_letter_press_without_winit_text(event: &winit::event::KeyEvent) -> bool {
    event.state == ElementState::Pressed
        && lowercase_letter_from_physical(&event.physical_key).is_some()
        && !event.text.as_ref().is_some_and(|t| !t.is_empty())
}

/// KeyDown delivered a non-control character (user is typing into the page, not a bare shortcut).
pub fn keyevent_has_printable_text(event: &winit::event::KeyEvent) -> bool {
    event.state == ElementState::Pressed
        && event
            .text
            .as_ref()
            .is_some_and(|t| t.chars().any(|c| !c.is_control()))
}

/// Physical letter key → lowercase ASCII (layout-independent), for link-hint feeding.
pub fn lowercase_letter_from_physical(physical: &PhysicalKey) -> Option<char> {
    let PhysicalKey::Code(code) = physical else {
        return None;
    };
    match code {
        KeyCode::KeyA => Some('a'),
        KeyCode::KeyB => Some('b'),
        KeyCode::KeyC => Some('c'),
        KeyCode::KeyD => Some('d'),
        KeyCode::KeyE => Some('e'),
        KeyCode::KeyF => Some('f'),
        KeyCode::KeyG => Some('g'),
        KeyCode::KeyH => Some('h'),
        KeyCode::KeyI => Some('i'),
        KeyCode::KeyJ => Some('j'),
        KeyCode::KeyK => Some('k'),
        KeyCode::KeyL => Some('l'),
        KeyCode::KeyM => Some('m'),
        KeyCode::KeyN => Some('n'),
        KeyCode::KeyO => Some('o'),
        KeyCode::KeyP => Some('p'),
        KeyCode::KeyQ => Some('q'),
        KeyCode::KeyR => Some('r'),
        KeyCode::KeyS => Some('s'),
        KeyCode::KeyT => Some('t'),
        KeyCode::KeyU => Some('u'),
        KeyCode::KeyV => Some('v'),
        KeyCode::KeyW => Some('w'),
        KeyCode::KeyX => Some('x'),
        KeyCode::KeyY => Some('y'),
        KeyCode::KeyZ => Some('z'),
        _ => None,
    }
}

/// Link-hint label character: physical letter keys, or a single `text` / `logical_key` character
/// when macOS reports [`PhysicalKey::Unidentified`] for focused web controls (digits allowed).
pub fn hint_label_char_from_key_event(event: &winit::event::KeyEvent) -> Option<char> {
    if let Some(ch) = lowercase_letter_from_physical(&event.physical_key) {
        return Some(ch);
    }
    let ch = event
        .text
        .as_ref()
        .and_then(|t| {
            let mut it = t.chars();
            let c = it.next()?;
            if it.next().is_some() {
                return None;
            }
            Some(c)
        })
        .or_else(|| match &event.logical_key {
            Key::Character(s) => {
                let mut it = s.chars();
                let c = it.next()?;
                if it.next().is_some() {
                    return None;
                }
                Some(c)
            }
            _ => None,
        })?;
    if ch.is_ascii_alphabetic() {
        Some(ch.to_ascii_lowercase())
    } else if ch.is_ascii_digit() {
        Some(ch)
    } else {
        None
    }
}

pub fn is_cmd_q_pressed(
    mods: cef_event_flags_t,
    state: ElementState,
    physical: &PhysicalKey,
) -> bool {
    let cmd = (mods.0 & cef_event_flags_t::EVENTFLAG_COMMAND_DOWN.0) != 0;
    cmd && state == ElementState::Pressed
        && match physical {
            PhysicalKey::Code(KeyCode::KeyQ) => true,
            _ => false,
        }
}

#[cfg(test)]
use bevy_ecs::prelude::{Res, ResMut, Resource, World};
#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;

#[cfg(test)]
#[derive(Resource)]
struct KbDupCharIn(char);

#[cfg(test)]
#[derive(Resource, Default)]
struct KbDupCharOut(bool);

#[cfg(test)]
fn keyboard_dup_char_system(inp: Res<KbDupCharIn>, mut out: ResMut<KbDupCharOut>) {
    out.0 = keyboard_text_char_duplicates_keydown(inp.0);
}

#[cfg(test)]
#[test]
fn keyboard_text_skip_matches_backspace_delete_control_chars_via_system() {
    let mut world = World::default();

    world.insert_resource(KbDupCharIn('\u{8}'));
    world.insert_resource(KbDupCharOut::default());
    world.run_system_once(keyboard_dup_char_system).unwrap();
    assert!(world.resource::<KbDupCharOut>().0);

    world.insert_resource(KbDupCharIn('\u{7f}'));
    world.insert_resource(KbDupCharOut::default());
    world.run_system_once(keyboard_dup_char_system).unwrap();
    assert!(world.resource::<KbDupCharOut>().0);

    world.insert_resource(KbDupCharIn('a'));
    world.insert_resource(KbDupCharOut::default());
    world.run_system_once(keyboard_dup_char_system).unwrap();
    assert!(!world.resource::<KbDupCharOut>().0);

    world.insert_resource(KbDupCharIn(' '));
    world.insert_resource(KbDupCharOut::default());
    world.run_system_once(keyboard_dup_char_system).unwrap();
    assert!(!world.resource::<KbDupCharOut>().0);
}
