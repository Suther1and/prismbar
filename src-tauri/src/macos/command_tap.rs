/// Pure Rust lone-Command tap state machine for macOS (`aarch64-apple-darwin`),
/// compiled on all platforms so unit tests execute on both Windows and macOS.

pub const KEY_CODE_LEFT_COMMAND: u16 = 55;
pub const KEY_CODE_RIGHT_COMMAND: u16 = 54;

/// CoreGraphics `CGEventFlags` bitmasks (`CGEventTypes.h`).
pub const CG_FLAG_MASK_SHIFT: u64 = 0x0002_0000;
pub const CG_FLAG_MASK_CONTROL: u64 = 0x0004_0000;
pub const CG_FLAG_MASK_ALTERNATE: u64 = 0x0008_0000;
pub const CG_FLAG_MASK_COMMAND: u64 = 0x0010_0000;
pub const CG_FLAG_MASK_OTHER_MODIFIERS: u64 =
    CG_FLAG_MASK_SHIFT | CG_FLAG_MASK_CONTROL | CG_FLAG_MASK_ALTERNATE;

pub const MAX_COMMAND_TAP_DURATION_MS: u64 = 280;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTapMode {
    Any,
    Left,
    Right,
    Disabled,
}

impl CommandTapMode {
    pub fn from_config_str(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "left" => Self::Left,
            "right" => Self::Right,
            "disabled" => Self::Disabled,
            _ => Self::Any,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Left => "left",
            Self::Right => "right",
            Self::Disabled => "disabled",
        }
    }

    fn matches_key_code(self, key_code: u16) -> bool {
        match self {
            Self::Any => key_code == KEY_CODE_LEFT_COMMAND || key_code == KEY_CODE_RIGHT_COMMAND,
            Self::Left => key_code == KEY_CODE_LEFT_COMMAND,
            Self::Right => key_code == KEY_CODE_RIGHT_COMMAND,
            Self::Disabled => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveCommandPress {
    key_code: u16,
    pressed_at_ms: u64,
    tainted: bool,
}

/// Tracks lone-Command tap gestures (`FlagsChanged`, `KeyDown`, `MouseDown`) and emits
/// `true` solely when a matching Command key is pressed cleanly and released within 320ms
/// without any intervening key presses, other modifier keys, or mouse clicks.
#[derive(Debug, Clone)]
pub struct CommandTapMachine {
    mode: CommandTapMode,
    cmd_was_down: bool,
    active: Option<ActiveCommandPress>,
}

impl Default for CommandTapMachine {
    fn default() -> Self {
        Self::new(CommandTapMode::Any)
    }
}

impl CommandTapMachine {
    pub fn new(mode: CommandTapMode) -> Self {
        Self {
            mode,
            cmd_was_down: false,
            active: None,
        }
    }

    pub fn mode(&self) -> CommandTapMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: CommandTapMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if mode == CommandTapMode::Disabled {
            self.active = None;
        } else if let Some(active) = &mut self.active {
            if !mode.matches_key_code(active.key_code) {
                active.tainted = true;
            }
        }
    }

    pub fn on_key_down(&mut self) {
        if let Some(active) = &mut self.active {
            active.tainted = true;
        }
    }

    pub fn on_key_up(&mut self) {
        if let Some(active) = &mut self.active {
            active.tainted = true;
        }
    }

    pub fn on_mouse_down(&mut self) {
        if let Some(active) = &mut self.active {
            active.tainted = true;
        }
    }

    pub fn on_secure_input(&mut self) {
        if let Some(active) = &mut self.active {
            active.tainted = true;
        }
        self.active = None;
        self.cmd_was_down = false;
    }

    pub fn on_flags_changed(&mut self, key_code: u16, flags: u64, now_ms: u64) -> bool {
        let cmd_down = (flags & CG_FLAG_MASK_COMMAND) != 0;
        let other_mods = (flags & CG_FLAG_MASK_OTHER_MODIFIERS) != 0;

        if self.mode == CommandTapMode::Disabled {
            self.cmd_was_down = cmd_down;
            self.active = None;
            return false;
        }

        match (self.cmd_was_down, cmd_down) {
            (false, true) => {
                self.cmd_was_down = true;
                if self.mode.matches_key_code(key_code) && !other_mods {
                    self.active = Some(ActiveCommandPress {
                        key_code,
                        pressed_at_ms: now_ms,
                        tainted: false,
                    });
                } else {
                    self.active = None;
                }
                false
            }
            (true, true) => {
                if let Some(active) = &mut self.active {
                    if other_mods || active.key_code != key_code {
                        active.tainted = true;
                    }
                }
                false
            }
            (true, false) => {
                self.cmd_was_down = false;
                let Some(press) = self.active.take() else {
                    return false;
                };
                if press.tainted || other_mods {
                    return false;
                }
                if press.key_code != key_code || !self.mode.matches_key_code(key_code) {
                    return false;
                }
                now_ms >= press.pressed_at_ms
                    && (now_ms - press.pressed_at_ms) <= MAX_COMMAND_TAP_DURATION_MS
            }
            (false, false) => {
                self.active = None;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NON_COALESCED_BIT: u64 = 0x0000_0100;

    #[test]
    fn any_mode_triggers_for_both_left_and_right_command_taps() {
        let mut machine = CommandTapMachine::new(CommandTapMode::Any);

        // Left Command tap (120ms) with macOS NonCoalesced bit set
        assert!(!machine.on_flags_changed(
            KEY_CODE_LEFT_COMMAND,
            CG_FLAG_MASK_COMMAND | NON_COALESCED_BIT,
            1000
        ));
        assert!(machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, NON_COALESCED_BIT, 1120));

        // Right Command tap (exactly at 280ms boundary)
        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, CG_FLAG_MASK_COMMAND, 2000));
        assert!(machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, 0, 2280));
    }

    #[test]
    fn left_right_and_disabled_modes_filter_strictly() {
        let mut machine = CommandTapMachine::new(CommandTapMode::Left);

        // Left triggers
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 100));
        assert!(machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 180));

        // Right ignored in Left mode
        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, CG_FLAG_MASK_COMMAND, 300));
        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, 0, 360));

        // Switch to Right mode
        machine.set_mode(CommandTapMode::Right);
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 500));
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 560));

        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, CG_FLAG_MASK_COMMAND, 700));
        assert!(machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, 0, 790));

        // Disabled mode never triggers
        machine.set_mode(CommandTapMode::Disabled);
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 900));
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 950));
        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, CG_FLAG_MASK_COMMAND, 1000));
        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, 0, 1050));
    }

    #[test]
    fn cmd_c_cmd_tab_and_cmd_click_taint_and_suppress_trigger() {
        let mut machine = CommandTapMachine::new(CommandTapMode::Any);

        // Cmd+C (key down while Command is held)
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 100));
        machine.on_key_down();
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 190));

        // Cmd+Click (mouse down while Command is held)
        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, CG_FLAG_MASK_COMMAND, 300));
        machine.on_mouse_down();
        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, 0, 380));

        // Subsequent clean tap works normally
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 500));
        assert!(machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 575));
    }

    #[test]
    fn cmd_shift_option_control_taint_both_press_orders() {
        let mut machine = CommandTapMachine::new(CommandTapMode::Any);

        // Order A: Command pressed -> Shift pressed -> Shift released -> Command released
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 100));
        assert!(!machine.on_flags_changed(
            56,
            CG_FLAG_MASK_COMMAND | CG_FLAG_MASK_SHIFT,
            130
        ));
        assert!(!machine.on_flags_changed(56, CG_FLAG_MASK_COMMAND, 160));
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 200));

        // Order B: Option already held -> Command pressed -> Option released -> Command released
        assert!(!machine.on_flags_changed(58, CG_FLAG_MASK_ALTERNATE, 300));
        assert!(!machine.on_flags_changed(
            KEY_CODE_LEFT_COMMAND,
            CG_FLAG_MASK_ALTERNATE | CG_FLAG_MASK_COMMAND,
            320
        ));
        assert!(!machine.on_flags_changed(58, CG_FLAG_MASK_COMMAND, 350));
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 390));

        // Control held during Command release
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 500));
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_CONTROL, 550));
    }

    #[test]
    fn hold_over_280ms_times_out_and_suppresses() {
        let mut machine = CommandTapMachine::new(CommandTapMode::Any);

        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 1000));
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 1281));
    }

    #[test]
    fn rapid_double_tap_triggers_each_clean_release() {
        let mut machine = CommandTapMachine::new(CommandTapMode::Any);

        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 100));
        assert!(machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 160));

        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 210));
        assert!(machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 270));
    }

    #[test]
    fn non_command_modifiers_and_capslock_do_not_break_or_spuriously_trigger() {
        let mut machine = CommandTapMachine::new(CommandTapMode::Any);

        // Shift tap alone
        assert!(!machine.on_flags_changed(56, CG_FLAG_MASK_SHIFT, 100));
        assert!(!machine.on_flags_changed(56, 0, 150));

        // CapsLock (0x0001_0000) active while tapping Command still triggers cleanly
        const CAPS_LOCK: u64 = 0x0001_0000;
        assert!(!machine.on_flags_changed(
            KEY_CODE_LEFT_COMMAND,
            CG_FLAG_MASK_COMMAND | CAPS_LOCK,
            300
        ));
        assert!(machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CAPS_LOCK, 380));
    }

    #[test]
    fn both_command_keys_pressed_together_suppresses_trigger() {
        let mut machine = CommandTapMachine::new(CommandTapMode::Any);

        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 100));
        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, CG_FLAG_MASK_COMMAND, 130));
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 170));
        assert!(!machine.on_flags_changed(KEY_CODE_RIGHT_COMMAND, 0, 200));
    }

    #[test]
    fn key_up_and_secure_input_taint_active_press_and_reset_cleanly() {
        let mut machine = CommandTapMachine::new(CommandTapMode::Any);

        // KeyUp while Command is held taints the tap
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 100));
        machine.on_key_up();
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 180));

        // Secure Input active while Command is held clears active tap & state
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 300));
        machine.on_secure_input();
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 360));

        // Subsequent normal tap works cleanly
        assert!(!machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, CG_FLAG_MASK_COMMAND, 500));
        assert!(machine.on_flags_changed(KEY_CODE_LEFT_COMMAND, 0, 580));
    }
}
