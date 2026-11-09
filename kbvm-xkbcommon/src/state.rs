use crate::keymap::KeymapHandle;
use kbvm::Components;
use kbvm::GroupDelta;
use kbvm::GroupIndex;
use kbvm::Keycode;
use kbvm::Keysym;
use kbvm::ModifierMask;
use kbvm::state_machine;
use kbvm::state_machine::Direction;
use std::sync::Arc;

const CAPS: u32 = 1 << 1;
const CTRL: u32 = 1 << 2;

const MODS_DEPRESSED: u32 = 1 << 0;
const MODS_LATCHED: u32 = 1 << 1;
const MODS_LOCKED: u32 = 1 << 2;
const MODS_EFFECTIVE: u32 = 1 << 3;
const LAYOUT_DEPRESSED: u32 = 1 << 4;
const LAYOUT_LATCHED: u32 = 1 << 5;
const LAYOUT_LOCKED: u32 = 1 << 6;
const LAYOUT_EFFECTIVE: u32 = 1 << 7;
const LEDS: u32 = 1 << 8;

pub(crate) struct StateHandle {
    pub(crate) keymap: Arc<KeymapHandle>,
    pub(crate) base_mods: u32,
    pub(crate) latched_mods: u32,
    pub(crate) locked_mods: u32,
    pub(crate) mods: u32,
    pub(crate) base_group: u32,
    pub(crate) latched_group: u32,
    pub(crate) locked_group: u32,
    pub(crate) group: u32,
    leds: Vec<bool>,
    machine_state: state_machine::State,
    events: Vec<state_machine::Event>,
}

struct Snapshot {
    base_mods: u32,
    latched_mods: u32,
    locked_mods: u32,
    mods: u32,
    base_group: u32,
    latched_group: u32,
    locked_group: u32,
    group: u32,
    leds: Vec<bool>,
}

impl Snapshot {
    fn changes(&self, next: &Snapshot) -> u32 {
        let mut changes = 0;
        if self.group != next.group {
            changes |= LAYOUT_EFFECTIVE;
        }
        if self.base_group != next.base_group {
            changes |= LAYOUT_DEPRESSED;
        }
        if self.latched_group != next.latched_group {
            changes |= LAYOUT_LATCHED;
        }
        if self.locked_group != next.locked_group {
            changes |= LAYOUT_LOCKED;
        }
        if self.mods != next.mods {
            changes |= MODS_EFFECTIVE;
        }
        if self.base_mods != next.base_mods {
            changes |= MODS_DEPRESSED;
        }
        if self.latched_mods != next.latched_mods {
            changes |= MODS_LATCHED;
        }
        if self.locked_mods != next.locked_mods {
            changes |= MODS_LOCKED;
        }
        if self.leds != next.leds {
            changes |= LEDS;
        }
        changes
    }
}

impl StateHandle {
    pub(crate) fn new(keymap: Arc<KeymapHandle>) -> Self {
        let machine_state = keymap.state_machine.create_state();
        let mut state = Self {
            keymap,
            base_mods: 0,
            latched_mods: 0,
            locked_mods: 0,
            mods: 0,
            base_group: 0,
            latched_group: 0,
            locked_group: 0,
            group: 0,
            leds: Vec::new(),
            machine_state,
            events: Vec::new(),
        };
        state.leds = state.active_leds();
        state
    }

    fn mod_mask(&self) -> u32 {
        let num_mods = self.keymap.num_mods();
        if num_mods >= 32 {
            u32::MAX
        } else {
            (1u32 << num_mods) - 1
        }
    }

    /// libxkbcommon's `mod_mask_get_effective`.
    fn resolve_mods(&self, mods: u32) -> u32 {
        let mut mask = mods & 0xff;
        for (idx, m) in self.keymap.mods.iter().enumerate().skip(8) {
            if idx < 32 && mods & (1u32 << idx) != 0 {
                mask |= m.mapping;
            }
        }
        mask
    }

    fn components(&self) -> Components {
        let mut components = Components::default();
        components.mods_pressed = ModifierMask(self.base_mods);
        components.mods_latched = ModifierMask(self.latched_mods);
        components.mods_locked = ModifierMask(self.locked_mods);
        components.mods = ModifierMask(self.mods);
        components.group_pressed = GroupDelta(self.base_group);
        components.group_latched = GroupDelta(self.latched_group);
        components.group_locked = GroupIndex(self.locked_group);
        components.group = GroupIndex(self.group);
        components
    }

    fn active_leds(&self) -> Vec<bool> {
        let components = self.components();
        self.keymap
            .keymap
            .indicators()
            .map(|indicator| indicator.matcher().matches(&components))
            .collect()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            base_mods: self.base_mods,
            latched_mods: self.latched_mods,
            locked_mods: self.locked_mods,
            mods: self.mods,
            base_group: self.base_group,
            latched_group: self.latched_group,
            locked_group: self.locked_group,
            group: self.group,
            leds: self.leds.clone(),
        }
    }

    pub(crate) fn update_mask(
        &mut self,
        base_mods: u32,
        latched_mods: u32,
        locked_mods: u32,
        base_group: u32,
        latched_group: u32,
        locked_group: u32,
    ) -> u32 {
        let prev = self.snapshot();
        let mask = self.mod_mask();
        self.base_mods = base_mods & mask;
        self.latched_mods = latched_mods & mask;
        self.locked_mods = locked_mods & mask;
        self.base_mods |= self.resolve_mods(self.base_mods);
        self.latched_mods |= self.resolve_mods(self.latched_mods);
        self.locked_mods |= self.resolve_mods(self.locked_mods);
        self.base_group = base_group;
        self.latched_group = latched_group;
        self.locked_group = locked_group;
        self.update_derived();
        prev.changes(&self.snapshot())
    }

    pub(crate) fn update_key(&mut self, keycode: u32, direction: Direction) -> u32 {
        if !self.keymap.in_keycode_range(keycode) {
            return 0;
        }
        let prev = self.snapshot();
        let mut components = self.components();
        self.events.clear();
        self.keymap.state_machine.handle_key(
            &mut self.machine_state,
            &mut self.events,
            Keycode::from_x11(keycode),
            direction,
        );
        for event in self.events.drain(..) {
            components.apply_event(event);
        }
        self.base_mods = components.mods_pressed.0;
        self.latched_mods = components.mods_latched.0;
        self.locked_mods = components.mods_locked.0;
        self.base_group = components.group_pressed.0;
        self.latched_group = components.group_latched.0;
        self.locked_group = components.group_locked.0;
        self.update_derived();
        prev.changes(&self.snapshot())
    }

    fn update_derived(&mut self) {
        self.locked_group = wrap_group(self.locked_group as i32, self.keymap.num_groups);
        let sum = (self.base_group as i32)
            .wrapping_add(self.latched_group as i32)
            .wrapping_add(self.locked_group as i32);
        self.group = wrap_group(sum, self.keymap.num_groups);
        self.mods = self.base_mods | self.latched_mods | self.locked_mods;
        self.leds = self.active_leds();
    }

    pub(crate) fn serialize_mods(&self, components: u32) -> u32 {
        if components & MODS_EFFECTIVE != 0 {
            return self.mods;
        }
        let mut mask = 0;
        if components & MODS_DEPRESSED != 0 {
            mask |= self.base_mods;
        }
        if components & MODS_LATCHED != 0 {
            mask |= self.latched_mods;
        }
        if components & MODS_LOCKED != 0 {
            mask |= self.locked_mods;
        }
        mask
    }

    pub(crate) fn layout_index_is_active(&self, idx: u32, components: u32) -> i32 {
        if idx >= self.keymap.num_groups {
            return -1;
        }
        let active = (components & LAYOUT_EFFECTIVE != 0 && self.group == idx)
            || (components & LAYOUT_DEPRESSED != 0 && self.base_group == idx)
            || (components & LAYOUT_LATCHED != 0 && self.latched_group == idx)
            || (components & LAYOUT_LOCKED != 0 && self.locked_group == idx);
        i32::from(active)
    }

    pub(crate) fn led_name_is_active(&self, name: &[u8]) -> i32 {
        match self.keymap.led_index(name) {
            Some(idx) => i32::from(self.leds[idx]),
            None => -1,
        }
    }

    pub(crate) fn key_get_layout(&self, keycode: u32) -> Option<u32> {
        let key = self.keymap.keys.get(&keycode)?;
        key.layout(self.group).map(|layout| layout as u32)
    }

    pub(crate) fn key_get_level(&self, keycode: u32, layout: u32) -> Option<u32> {
        let key = self.keymap.keys.get(&keycode)?;
        if layout as usize >= key.groups.len() {
            return None;
        }
        let group = key.group(layout as usize)?;
        Some(group.ty.level_and_consumed(self.mods).0 as u32)
    }

    pub(crate) fn mod_mask_remove_consumed(&self, keycode: u32, mask: u32) -> u32 {
        if !self.keymap.in_keycode_range(keycode) {
            return 0;
        }
        let effective = self.resolve_mods(mask);
        let consumed = self
            .keymap
            .keys
            .get(&keycode)
            .and_then(|key| key.group(key.layout(self.group)?))
            .map_or(0, |group| group.ty.level_and_consumed(self.mods).1);
        effective & !consumed
    }

    pub(crate) fn key_get_syms(&self, keycode: u32) -> Option<&[u32]> {
        let key = self.keymap.keys.get(&keycode)?;
        let layout = key.layout(self.group)?;
        let group = key.group(layout)?;
        let (level, consumed) = group.ty.level_and_consumed(self.mods);
        if level >= group.ty.num_levels {
            return None;
        }
        let level = group.levels.get(level)?;
        if level.syms.is_empty() {
            return None;
        }
        let caps = self.mods & CAPS != 0 && consumed & CAPS == 0;
        Some(if caps {
            level.upper.as_deref().unwrap_or(&level.syms)
        } else {
            &level.syms
        })
    }

    pub(crate) fn key_get_one_sym(&self, keycode: u32) -> u32 {
        match self.key_get_syms(keycode) {
            Some(syms) if syms.len() == 1 => syms[0],
            _ => 0,
        }
    }

    /// libxkbcommon's `xkb_state_key_get_utf32`.
    pub(crate) fn key_get_utf32(&self, keycode: u32) -> u32 {
        let Some(key) = self.keymap.keys.get(&keycode) else {
            return 0;
        };
        let num_layouts = key.groups.len();
        let Some(layout) = key.layout(self.group) else {
            return 0;
        };
        let Some(group) = key.group(layout) else {
            return 0;
        };
        let (level, consumed) = group.ty.level_and_consumed(self.mods);
        let Some(level_info) = group.levels.get(level) else {
            return 0;
        };
        if level_info.syms.len() != 1 {
            return 0;
        }
        let mut sym = level_info.syms[0];
        let ctrl = self.mods & CTRL != 0 && consumed & CTRL == 0;
        let caps = self.mods & CAPS != 0 && consumed & CAPS == 0;
        if ctrl && sym > 127 {
            for i in 0..num_layouts {
                let Some(group) = key.group(i) else {
                    continue;
                };
                let (level, _) = group.ty.level_and_consumed(self.mods);
                let Some(level_info) = group.levels.get(level) else {
                    continue;
                };
                if level_info.syms.len() == 1 && level_info.syms[0] <= 127 {
                    sym = level_info.syms[0];
                    break;
                }
            }
        }
        if caps {
            sym = Keysym(sym).to_uppercase().0;
        }
        let mut cp = Keysym(sym).char().map(|c| c as u32).unwrap_or(0);
        if cp <= 127 && ctrl {
            cp = xkb_to_control(cp as u8) as u32;
        }
        cp
    }

    pub(crate) fn mod_index_is_active(&self, idx: u32, affect: u32) -> i32 {
        let Some(info) = self.keymap.mods.get(idx as usize) else {
            return -1;
        };
        let mapping = info.mapping;
        if mapping == 0 {
            return 0;
        }
        let serialized = if affect & MODS_EFFECTIVE != 0 {
            self.mods
        } else {
            let mut mask = 0;
            if affect & MODS_DEPRESSED != 0 {
                mask |= self.base_mods;
            }
            if affect & MODS_LATCHED != 0 {
                mask |= self.latched_mods;
            }
            if affect & MODS_LOCKED != 0 {
                mask |= self.locked_mods;
            }
            mask
        };
        i32::from(serialized & mapping == mapping)
    }
}

fn wrap_group(group: i32, num_groups: u32) -> u32 {
    if num_groups == 0 {
        0
    } else {
        group.rem_euclid(num_groups as i32) as u32
    }
}

fn xkb_to_control(c: u8) -> u8 {
    match c {
        b'@'..=b'~' | b' ' => c & 0x1f,
        b'2' => 0,
        b'3'..=b'7' => c - 0x18,
        b'8' => 0x7f,
        b'/' => 0x1f,
        _ => c,
    }
}
