//! Validated keyboard and controller chords shared by settings and input polling.

/// Parses a key with optional Ctrl, Alt, and Shift modifiers into Win32 virtual keys.
pub fn keyboard(value: &str) -> Option<(i32, [bool; 3])> {
    let mut modifiers = [false; 3];
    let mut key = None;
    for part in value.split('+') {
        match part.to_ascii_uppercase().as_str() {
            "CTRL" => modifiers[0] = true,
            "ALT" => modifiers[1] = true,
            "SHIFT" => modifiers[2] = true,
            name => {
                if key.is_some() {
                    return None;
                }
                key = Some(match name {
                    "SPACE" => 0x20,
                    "TAB" => 9,
                    "ENTER" => 13,
                    "ESCAPE" => 27,
                    "BACKSPACE" => 8,
                    "INSERT" => 0x2d,
                    "DELETE" => 0x2e,
                    "HOME" => 0x24,
                    "END" => 0x23,
                    "PAGEUP" => 0x21,
                    "PAGEDOWN" => 0x22,
                    "LEFT" => 0x25,
                    "UP" => 0x26,
                    "RIGHT" => 0x27,
                    "DOWN" => 0x28,
                    n if n.starts_with('F') && n.len() > 1 => {
                        let number = n[1..].parse::<i32>().ok()?;
                        if !(1..=24).contains(&number) {
                            return None;
                        }
                        0x6f + number
                    }
                    n if n.starts_with("NUMPAD") => {
                        let number = n[6..].parse::<i32>().ok()?;
                        if !(0..=9).contains(&number) {
                            return None;
                        }
                        0x60 + number
                    }
                    n if n.len() == 1 && n.as_bytes()[0].is_ascii_alphanumeric() => {
                        n.as_bytes()[0] as i32
                    }
                    _ => return None,
                });
            }
        }
    }
    Some((key?, modifiers))
}

/// Parses XInput button chords and trigger combinations, retaining old settings names.
pub fn gamepad(value: &str) -> Option<(u16, bool, bool)> {
    let value = match value {
        "shoulders" => "LB+RB",
        "back" => "Back",
        "start" => "Start",
        "ls" => "L3",
        "rs" => "R3",
        "off" => return Some((0, false, false)),
        other => other,
    };
    let mut mask = 0;
    let mut lt = false;
    let mut rt = false;
    for part in value.split('+') {
        mask |= match part.to_ascii_uppercase().as_str() {
            "A" => 0x1000,
            "B" => 0x2000,
            "X" => 0x4000,
            "Y" => 0x8000,
            "LB" => 0x100,
            "RB" => 0x200,
            "BACK" => 0x20,
            "START" => 0x10,
            "L3" => 0x40,
            "R3" => 0x80,
            "UP" => 1,
            "DOWN" => 2,
            "LEFT" => 4,
            "RIGHT" => 8,
            "LT" => {
                lt = true;
                0
            }
            "RT" => {
                rt = true;
                0
            }
            _ => return None,
        };
    }
    Some((mask, lt, rt))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_and_chords_are_validated() {
        assert_eq!(
            keyboard("Ctrl+Shift+F24"),
            Some((0x87, [true, false, true]))
        );
        assert_eq!(keyboard("Alt+Q"), Some((81, [false, true, false])));
        assert!(keyboard("F25").is_none());
        assert!(keyboard("A+B").is_none());
        assert_eq!(gamepad("LT+RB"), Some((0x200, true, false)));
        assert_eq!(gamepad("shoulders"), Some((0x300, false, false)));
        assert!(gamepad("Guide").is_none());
    }
}
