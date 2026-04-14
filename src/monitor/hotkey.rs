use rdev::{listen, Event, EventType, Key};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Parsed hotkey: a set of modifier keys + one regular key.
#[derive(Clone)]
pub struct Hotkey {
    pub modifiers: HashSet<Key>,
    pub key: Key,
}

/// Start a background thread that listens for a global hotkey.
/// When pressed, runs `callback` (which should focus C4's terminal).
pub fn start_hotkey_listener(hotkey: Hotkey, callback: Arc<dyn Fn() + Send + Sync>) {
    std::thread::spawn(move || {
        let held: Arc<Mutex<HashSet<Key>>> = Arc::new(Mutex::new(HashSet::new()));
        let held2 = held.clone();

        let result = listen(move |event: Event| {
            match event.event_type {
                EventType::KeyPress(key) => {
                    let mut h = held2.lock().unwrap();
                    h.insert(key);

                    // Check if all required modifiers + the key are held
                    if key == hotkey.key
                        && hotkey.modifiers.iter().all(|m| h.contains(m))
                    {
                        callback();
                    }
                }
                EventType::KeyRelease(key) => {
                    let mut h = held2.lock().unwrap();
                    h.remove(&key);
                }
                _ => {}
            }
        });

        if let Err(e) = result {
            eprintln!("Hotkey listener error: {:?}", e);
            eprintln!("On macOS, grant Accessibility permission in System Settings > Privacy & Security > Accessibility");
        }
    });
}

/// Parse a hotkey string like "ctrl+shift+space" into a Hotkey struct.
pub fn parse_hotkey(s: &str) -> Result<Hotkey, String> {
    let parts: Vec<String> = s.split('+').map(|p| p.trim().to_lowercase()).collect();

    if parts.is_empty() {
        return Err("Empty hotkey string".into());
    }

    let mut modifiers = HashSet::new();
    let mut key = None;

    for part in &parts {
        match part.as_str() {
            "ctrl" | "control" => { modifiers.insert(Key::ControlLeft); }
            "shift" => { modifiers.insert(Key::ShiftLeft); }
            "alt" | "option" => { modifiers.insert(Key::Alt); }
            "cmd" | "command" | "meta" | "super" => { modifiers.insert(Key::MetaLeft); }
            k => {
                if key.is_some() {
                    return Err(format!("Multiple non-modifier keys: '{}'", k));
                }
                key = Some(str_to_key(k)?);
            }
        }
    }

    let key = key.ok_or("No key specified (only modifiers)")?;

    if modifiers.is_empty() {
        return Err("At least one modifier required (ctrl, shift, alt, cmd)".into());
    }

    Ok(Hotkey { modifiers, key })
}

fn str_to_key(s: &str) -> Result<Key, String> {
    match s {
        "a" => Ok(Key::KeyA), "b" => Ok(Key::KeyB), "c" => Ok(Key::KeyC),
        "d" => Ok(Key::KeyD), "e" => Ok(Key::KeyE), "f" => Ok(Key::KeyF),
        "g" => Ok(Key::KeyG), "h" => Ok(Key::KeyH), "i" => Ok(Key::KeyI),
        "j" => Ok(Key::KeyJ), "k" => Ok(Key::KeyK), "l" => Ok(Key::KeyL),
        "m" => Ok(Key::KeyM), "n" => Ok(Key::KeyN), "o" => Ok(Key::KeyO),
        "p" => Ok(Key::KeyP), "q" => Ok(Key::KeyQ), "r" => Ok(Key::KeyR),
        "s" => Ok(Key::KeyS), "t" => Ok(Key::KeyT), "u" => Ok(Key::KeyU),
        "v" => Ok(Key::KeyV), "w" => Ok(Key::KeyW), "x" => Ok(Key::KeyX),
        "y" => Ok(Key::KeyY), "z" => Ok(Key::KeyZ),
        "0" => Ok(Key::Num0), "1" => Ok(Key::Num1), "2" => Ok(Key::Num2),
        "3" => Ok(Key::Num3), "4" => Ok(Key::Num4), "5" => Ok(Key::Num5),
        "6" => Ok(Key::Num6), "7" => Ok(Key::Num7), "8" => Ok(Key::Num8),
        "9" => Ok(Key::Num9),
        "space" => Ok(Key::Space),
        "enter" | "return" => Ok(Key::Return),
        "tab" => Ok(Key::Tab),
        "escape" | "esc" => Ok(Key::Escape),
        "backspace" => Ok(Key::Backspace),
        "`" | "backtick" | "grave" => Ok(Key::BackQuote),
        "-" | "minus" => Ok(Key::Minus),
        "=" | "equal" => Ok(Key::Equal),
        "[" => Ok(Key::LeftBracket),
        "]" => Ok(Key::RightBracket),
        "\\" | "backslash" => Ok(Key::BackSlash),
        ";" | "semicolon" => Ok(Key::SemiColon),
        "'" | "quote" => Ok(Key::Quote),
        "," | "comma" => Ok(Key::Comma),
        "." | "dot" | "period" => Ok(Key::Dot),
        "/" | "slash" => Ok(Key::Slash),
        "f1" => Ok(Key::F1), "f2" => Ok(Key::F2), "f3" => Ok(Key::F3),
        "f4" => Ok(Key::F4), "f5" => Ok(Key::F5), "f6" => Ok(Key::F6),
        "f7" => Ok(Key::F7), "f8" => Ok(Key::F8), "f9" => Ok(Key::F9),
        "f10" => Ok(Key::F10), "f11" => Ok(Key::F11), "f12" => Ok(Key::F12),
        _ => Err(format!("Unknown key: '{}'", s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hotkey_ctrl_shift_a() {
        let hk = parse_hotkey("ctrl+shift+a").unwrap();
        assert!(hk.modifiers.contains(&Key::ControlLeft));
        assert!(hk.modifiers.contains(&Key::ShiftLeft));
        assert_eq!(hk.key, Key::KeyA);
    }

    #[test]
    fn parse_hotkey_cmd_option_ctrl_equal() {
        let hk = parse_hotkey("cmd+option+ctrl+=").unwrap();
        assert!(hk.modifiers.contains(&Key::MetaLeft));
        assert!(hk.modifiers.contains(&Key::Alt));
        assert!(hk.modifiers.contains(&Key::ControlLeft));
        assert_eq!(hk.key, Key::Equal);
    }

    #[test]
    fn parse_hotkey_space_key() {
        let hk = parse_hotkey("ctrl+space").unwrap();
        assert_eq!(hk.key, Key::Space);
    }

    #[test]
    fn parse_hotkey_function_key() {
        let hk = parse_hotkey("ctrl+f5").unwrap();
        assert_eq!(hk.key, Key::F5);
    }

    #[test]
    fn parse_hotkey_option_alias_for_alt() {
        let hk = parse_hotkey("option+a").unwrap();
        assert!(hk.modifiers.contains(&Key::Alt));
    }

    #[test]
    fn parse_hotkey_command_alias_for_cmd() {
        let hk = parse_hotkey("command+a").unwrap();
        assert!(hk.modifiers.contains(&Key::MetaLeft));
    }

    #[test]
    fn parse_hotkey_no_modifier_fails() {
        assert!(parse_hotkey("a").is_err());
    }

    #[test]
    fn parse_hotkey_only_modifiers_fails() {
        assert!(parse_hotkey("ctrl+shift").is_err());
    }

    #[test]
    fn parse_hotkey_multiple_non_modifier_keys_fails() {
        assert!(parse_hotkey("ctrl+a+b").is_err());
    }

    #[test]
    fn parse_hotkey_unknown_key_fails() {
        assert!(parse_hotkey("ctrl+foobar123").is_err());
    }

    #[test]
    fn parse_hotkey_digit_key() {
        let hk = parse_hotkey("ctrl+5").unwrap();
        assert_eq!(hk.key, Key::Num5);
    }
}
