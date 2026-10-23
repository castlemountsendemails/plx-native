//! Android `KeyEvent`s to the app's key vocabulary, as the Fire TV remote mapping sets it. No JNI or
//! SDL in here, so the host unit suite tests it.

/// Mirrors `plx_ui::consts::Key`, which this crate does not depend on yet. CH and PAGE get their own
/// variants because the app reads those from raw codes (`consts::page_dir`), not from `Key`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Up,
    Down,
    Left { alt: bool },
    Right { alt: bool },
    Ok,
    Back,
    Play,
    Pause,
    PlayPause,
    Stop,
    Exit,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Down,
    Repeat,
    Up,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Press {
    pub key: Key,
    pub edge: Edge,
    /// The Android keycode the press came from.
    pub code: i32,
}

// android.view.KeyEvent
pub const ACTION_DOWN: i32 = 0;
pub const ACTION_UP: i32 = 1;
pub const KEYCODE_BACK: i32 = 4;
pub const KEYCODE_DPAD_UP: i32 = 19;
pub const KEYCODE_DPAD_DOWN: i32 = 20;
pub const KEYCODE_DPAD_LEFT: i32 = 21;
pub const KEYCODE_DPAD_RIGHT: i32 = 22;
pub const KEYCODE_DPAD_CENTER: i32 = 23;
pub const KEYCODE_ENTER: i32 = 66;
pub const KEYCODE_MENU: i32 = 82;
pub const KEYCODE_MEDIA_PLAY_PAUSE: i32 = 85;
pub const KEYCODE_MEDIA_STOP: i32 = 86;
pub const KEYCODE_MEDIA_REWIND: i32 = 89;
pub const KEYCODE_MEDIA_FAST_FORWARD: i32 = 90;
pub const KEYCODE_PAGE_UP: i32 = 92;
pub const KEYCODE_PAGE_DOWN: i32 = 93;
pub const KEYCODE_MEDIA_PLAY: i32 = 126;
pub const KEYCODE_MEDIA_PAUSE: i32 = 127;
pub const KEYCODE_CHANNEL_UP: i32 = 166;
pub const KEYCODE_CHANNEL_DOWN: i32 = 167;

pub const LONG_BACK_MS: u64 = 1000;

/// The key a keycode stands for, or `None` for a key the app leaves to Android (volume, HOME, ...).
pub fn map(code: i32) -> Option<Key> {
    Some(match code {
        KEYCODE_DPAD_UP => Key::Up,
        KEYCODE_DPAD_DOWN => Key::Down,
        KEYCODE_DPAD_LEFT => Key::Left { alt: false },
        KEYCODE_DPAD_RIGHT => Key::Right { alt: false },
        KEYCODE_DPAD_CENTER | KEYCODE_ENTER => Key::Ok,
        KEYCODE_BACK => Key::Back,
        KEYCODE_MEDIA_PLAY_PAUSE => Key::PlayPause,
        KEYCODE_MEDIA_REWIND => Key::Left { alt: true },
        KEYCODE_MEDIA_FAST_FORWARD => Key::Right { alt: true },
        KEYCODE_MEDIA_PLAY => Key::Play,
        KEYCODE_MEDIA_PAUSE => Key::Pause,
        KEYCODE_MEDIA_STOP => Key::Stop,
        KEYCODE_CHANNEL_UP | KEYCODE_PAGE_UP => Key::PageUp,
        KEYCODE_CHANNEL_DOWN | KEYCODE_PAGE_DOWN => Key::PageDown,
        KEYCODE_MENU => Key::Exit,
        _ => return None,
    })
}

/// Turns `KeyEvent`s into presses. BACK is held back until it is released (a tap: `Back`) or held
/// for `LONG_BACK_MS` (`Exit`, once, with no repeats); every other key passes its edges through.
#[derive(Default)]
pub struct Keys {
    back_down_at: Option<u64>,
    back_fired: bool,
}

impl Keys {
    pub const fn new() -> Self {
        Self {
            back_down_at: None,
            back_fired: false,
        }
    }

    /// `now_ms` is any monotonic clock in milliseconds, the same one `tick` is given.
    pub fn event(&mut self, code: i32, action: i32, repeat: i32, now_ms: u64) -> Vec<Press> {
        let Some(key) = map(code) else {
            return Vec::new();
        };
        if code == KEYCODE_BACK {
            return self.back(action, repeat, now_ms);
        }
        let edge = match action {
            ACTION_DOWN if repeat > 0 => Edge::Repeat,
            ACTION_DOWN => Edge::Down,
            ACTION_UP => Edge::Up,
            _ => return Vec::new(),
        };
        vec![Press { key, edge, code }]
    }

    /// Raises the long-BACK `Exit` on time even from a remote that sends no repeats, so call it
    /// every few tens of milliseconds.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Press> {
        match self.back_down_at {
            Some(at) if !self.back_fired && now_ms.saturating_sub(at) >= LONG_BACK_MS => {
                self.back_fired = true;
                vec![press(Key::Exit, Edge::Down)]
            }
            _ => Vec::new(),
        }
    }

    fn back(&mut self, action: i32, repeat: i32, now_ms: u64) -> Vec<Press> {
        match action {
            ACTION_DOWN if repeat == 0 => {
                self.back_down_at = Some(now_ms);
                self.back_fired = false;
                Vec::new()
            }
            ACTION_DOWN => self.tick(now_ms),
            ACTION_UP => {
                let held = self.back_down_at.take().is_some();
                let fired = std::mem::take(&mut self.back_fired);
                match (held, fired) {
                    (true, true) => vec![press(Key::Exit, Edge::Up)],
                    (true, false) => vec![press(Key::Back, Edge::Down), press(Key::Back, Edge::Up)],
                    // The press began before the app had focus.
                    (false, _) => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }
}

fn press(key: Key, edge: Edge) -> Press {
    Press {
        key,
        edge,
        code: KEYCODE_BACK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_of_the_remote_mapping_table() {
        let table = [
            (KEYCODE_DPAD_UP, Key::Up),
            (KEYCODE_DPAD_DOWN, Key::Down),
            (KEYCODE_DPAD_LEFT, Key::Left { alt: false }),
            (KEYCODE_DPAD_RIGHT, Key::Right { alt: false }),
            (KEYCODE_DPAD_CENTER, Key::Ok),
            (KEYCODE_ENTER, Key::Ok),
            (KEYCODE_BACK, Key::Back),
            (KEYCODE_MEDIA_PLAY_PAUSE, Key::PlayPause),
            (KEYCODE_MEDIA_REWIND, Key::Left { alt: true }),
            (KEYCODE_MEDIA_FAST_FORWARD, Key::Right { alt: true }),
            (KEYCODE_MEDIA_PLAY, Key::Play),
            (KEYCODE_MEDIA_PAUSE, Key::Pause),
            (KEYCODE_MEDIA_STOP, Key::Stop),
            (KEYCODE_CHANNEL_UP, Key::PageUp),
            (KEYCODE_PAGE_UP, Key::PageUp),
            (KEYCODE_CHANNEL_DOWN, Key::PageDown),
            (KEYCODE_PAGE_DOWN, Key::PageDown),
            (KEYCODE_MENU, Key::Exit),
        ];
        for (code, key) in table {
            assert_eq!(map(code), Some(key), "keycode {code}");
        }
    }

    #[test]
    fn keys_android_keeps_are_not_mapped_or_pressed() {
        // HOME, VOLUME_UP, VOLUME_DOWN, MUTE
        for code in [3, 24, 25, 164] {
            assert_eq!(map(code), None, "keycode {code}");
            assert!(Keys::new().event(code, ACTION_DOWN, 0, 0).is_empty());
        }
    }

    #[test]
    fn other_keys_pass_down_repeat_and_up_through() {
        let mut keys = Keys::new();
        let at = |key, edge| {
            vec![Press {
                key,
                edge,
                code: KEYCODE_DPAD_LEFT,
            }]
        };
        let left = Key::Left { alt: false };
        assert_eq!(
            keys.event(KEYCODE_DPAD_LEFT, ACTION_DOWN, 0, 0),
            at(left, Edge::Down)
        );
        assert_eq!(
            keys.event(KEYCODE_DPAD_LEFT, ACTION_DOWN, 1, 500),
            at(left, Edge::Repeat)
        );
        assert_eq!(
            keys.event(KEYCODE_DPAD_LEFT, ACTION_UP, 0, 600),
            at(left, Edge::Up)
        );
    }

    #[test]
    fn menu_is_exit_at_once() {
        let pressed = Keys::new().event(KEYCODE_MENU, ACTION_DOWN, 0, 0);
        assert_eq!(
            pressed,
            vec![Press {
                key: Key::Exit,
                edge: Edge::Down,
                code: KEYCODE_MENU
            }]
        );
    }

    #[test]
    fn a_tapped_back_is_back_on_release() {
        let mut keys = Keys::new();
        assert!(keys.event(KEYCODE_BACK, ACTION_DOWN, 0, 0).is_empty());
        assert!(keys.tick(999).is_empty());
        assert_eq!(
            keys.event(KEYCODE_BACK, ACTION_UP, 0, 999),
            vec![press(Key::Back, Edge::Down), press(Key::Back, Edge::Up)]
        );
    }

    #[test]
    fn a_held_back_is_one_exit_from_the_tick() {
        let mut keys = Keys::new();
        assert!(keys.event(KEYCODE_BACK, ACTION_DOWN, 0, 100).is_empty());
        assert!(keys.tick(1099).is_empty());
        assert_eq!(keys.tick(1100), vec![press(Key::Exit, Edge::Down)]);
        assert!(keys.tick(1150).is_empty());
        assert!(keys.event(KEYCODE_BACK, ACTION_DOWN, 30, 2500).is_empty());
        assert_eq!(
            keys.event(KEYCODE_BACK, ACTION_UP, 0, 3000),
            vec![press(Key::Exit, Edge::Up)]
        );
    }

    #[test]
    fn a_held_back_is_one_exit_from_repeats_alone() {
        let mut keys = Keys::new();
        assert!(keys.event(KEYCODE_BACK, ACTION_DOWN, 0, 0).is_empty());
        assert!(keys.event(KEYCODE_BACK, ACTION_DOWN, 1, 500).is_empty());
        assert_eq!(
            keys.event(KEYCODE_BACK, ACTION_DOWN, 2, 1000),
            vec![press(Key::Exit, Edge::Down)]
        );
        assert!(keys.event(KEYCODE_BACK, ACTION_DOWN, 3, 1050).is_empty());
    }

    #[test]
    fn back_after_a_long_back_taps_again() {
        let mut keys = Keys::new();
        keys.event(KEYCODE_BACK, ACTION_DOWN, 0, 0);
        keys.tick(1000);
        keys.event(KEYCODE_BACK, ACTION_UP, 0, 1200);
        assert!(keys.event(KEYCODE_BACK, ACTION_DOWN, 0, 2000).is_empty());
        assert!(keys.tick(2500).is_empty());
        assert_eq!(
            keys.event(KEYCODE_BACK, ACTION_UP, 0, 2600),
            vec![press(Key::Back, Edge::Down), press(Key::Back, Edge::Up)]
        );
    }

    #[test]
    fn a_back_release_without_its_press_is_nothing() {
        assert!(Keys::new().event(KEYCODE_BACK, ACTION_UP, 0, 0).is_empty());
    }
}
