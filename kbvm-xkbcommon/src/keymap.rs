use crate::context::ContextHandle;
use crate::parse;
use kbvm::Keysym;
use kbvm::builder::Redirect;
use kbvm::xkb::Keymap;
use kbvm::xkb::diagnostic::WriteToLog;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;

pub(crate) struct KeymapHandle {
    #[allow(dead_code)]
    pub(crate) context: Arc<ContextHandle>,
    pub(crate) keymap: Keymap,
    pub(crate) keys: HashMap<u32, KeyInfo>,
    pub(crate) mods: Vec<ModInfo>,
    pub(crate) min_keycode: u32,
    pub(crate) max_keycode: u32,
    pub(crate) num_groups: u32,
}

pub(crate) struct ModInfo {
    pub(crate) name: CString,
    pub(crate) mapping: u32,
}

pub(crate) struct KeyInfo {
    pub(crate) redirect: Redirect,
    pub(crate) groups: Vec<Option<GroupInfo>>,
}

#[derive(Clone)]
pub(crate) struct GroupInfo {
    pub(crate) ty: TypeInfo,
    pub(crate) levels: Vec<LevelInfo>,
}

#[derive(Clone)]
pub(crate) struct TypeInfo {
    mask: u32,
    mappings: Vec<MappingInfo>,
    pub(crate) num_levels: usize,
}

#[derive(Clone)]
struct MappingInfo {
    mods: u32,
    level: usize,
    preserved: u32,
}

#[derive(Clone)]
pub(crate) struct LevelInfo {
    pub(crate) syms: Vec<u32>,
    pub(crate) upper: Option<Vec<u32>>,
}

const REAL_MODS: [(&str, u32); 8] = [
    ("Shift", 1 << 0),
    ("Lock", 1 << 1),
    ("Control", 1 << 2),
    ("Mod1", 1 << 3),
    ("Mod2", 1 << 4),
    ("Mod3", 1 << 5),
    ("Mod4", 1 << 6),
    ("Mod5", 1 << 7),
];

impl TypeInfo {
    fn new(ty: &kbvm::xkb::keymap::KeyType) -> Self {
        let mut mappings = Vec::new();
        let mut num_levels = 1usize;
        for mapping in ty.mappings() {
            let level = mapping.level();
            num_levels = num_levels.max(level + 1);
            mappings.push(MappingInfo {
                mods: mapping.mask().0,
                level,
                preserved: mapping.preserved().0,
            });
        }
        Self {
            mask: ty.mask().0,
            mappings,
            num_levels,
        }
    }

    /// Mirrors libxkbcommon's `get_entry_for_key_state` and
    /// `key_get_consumed(XKB_CONSUMED_MODE_XKB)`.
    pub(crate) fn level_and_consumed(&self, mods: u32) -> (usize, u32) {
        let masked = mods & self.mask;
        for mapping in &self.mappings {
            if mapping.mods == masked {
                return (mapping.level, self.mask & !mapping.preserved);
            }
        }
        (0, self.mask)
    }
}

impl GroupInfo {
    fn new(group: &kbvm::xkb::keymap::KeyGroup) -> Self {
        let levels = group
            .levels()
            .map(|level| {
                let syms: Vec<u32> = level.symbols().iter().map(|s| s.0).collect();
                let upper: Vec<u32> = syms
                    .iter()
                    .map(|&s| Keysym(s).to_uppercase().0)
                    .collect();
                let upper = (upper != syms).then_some(upper);
                LevelInfo { syms, upper }
            })
            .collect();
        Self {
            ty: TypeInfo::new(group.ty()),
            levels,
        }
    }
}

impl KeyInfo {
    /// Mirrors libxkbcommon's `XkbWrapGroupIntoRange` with the key's
    /// out-of-range group action.
    pub(crate) fn layout(&self, group: u32) -> Option<usize> {
        let num_groups = self.groups.len();
        if num_groups == 0 {
            return None;
        }
        let group = group as i32;
        if group >= 0 && (group as usize) < num_groups {
            return Some(group as usize);
        }
        Some(match self.redirect {
            Redirect::Wrap => {
                let rem = group % num_groups as i32;
                if rem >= 0 {
                    rem as usize
                } else {
                    (rem + num_groups as i32) as usize
                }
            }
            Redirect::Clamp => {
                if group < 0 {
                    0
                } else {
                    num_groups - 1
                }
            }
            Redirect::Fixed(n) => {
                if n < num_groups {
                    n
                } else {
                    0
                }
            }
        })
    }

    /// Returns the group used for `layout`, falling back to the first group
    /// like libxkbcommon does for undefined groups in the middle of a key.
    pub(crate) fn group(&self, layout: usize) -> Option<&GroupInfo> {
        let group = self.groups.get(layout)?.as_ref();
        group.or_else(|| self.groups.first()?.as_ref())
    }
}

impl KeymapHandle {
    pub(crate) fn new(context: Arc<ContextHandle>, text: &[u8]) -> Option<Self> {
        let keymap = context
            .kbvm_context()
            .keymap_from_bytes(WriteToLog, None, text)
            .ok()?;
        let mut keys = HashMap::new();
        let mut num_groups = 0u32;
        for key in keymap.keys() {
            let groups: Vec<_> = key.groups().map(|g| g.map(GroupInfo::new)).collect();
            num_groups = num_groups.max(groups.len() as u32);
            keys.insert(
                key.keycode().raw(),
                KeyInfo {
                    redirect: key.redirect(),
                    groups,
                },
            );
        }
        let (min_keycode, max_keycode) = parse::keycode_range(text);
        let mut mods = Vec::new();
        for (name, mapping) in REAL_MODS {
            mods.push(ModInfo {
                name: CString::new(name).unwrap_or_default(),
                mapping,
            });
        }
        let virtual_modifiers: Vec<(String, u32)> = keymap
            .virtual_modifiers()
            .map(|m| (m.name().to_string(), m.mask().0))
            .collect();
        let order = parse::virtual_modifier_order(text);
        let synthetic_dummy = virtual_modifiers.len() == 1
            && virtual_modifiers[0].0 == "Dummy"
            && virtual_modifiers[0].1 == 0
            && !order.iter().any(|name| name == "Dummy");
        let virtual_modifiers = if synthetic_dummy {
            &virtual_modifiers[..0]
        } else {
            &virtual_modifiers[..]
        };
        let mut used = vec![false; virtual_modifiers.len()];
        for name in &order {
            if let Some(pos) = virtual_modifiers.iter().position(|(n, _)| n == name)
                && !used[pos]
            {
                used[pos] = true;
                mods.push(ModInfo {
                    name: CString::new(name.as_str()).unwrap_or_default(),
                    mapping: virtual_modifiers[pos].1,
                });
            }
        }
        for (pos, (name, mapping)) in virtual_modifiers.iter().enumerate() {
            if !used[pos] {
                mods.push(ModInfo {
                    name: CString::new(name.as_str()).unwrap_or_default(),
                    mapping: *mapping,
                });
            }
        }
        Some(Self {
            context,
            keymap,
            keys,
            mods,
            min_keycode,
            max_keycode,
            num_groups,
        })
    }

    pub(crate) fn num_mods(&self) -> u32 {
        self.mods.len() as u32
    }

    pub(crate) fn mod_name(&self, idx: u32) -> Option<&CString> {
        self.mods.get(idx as usize).map(|m| &m.name)
    }

    pub(crate) fn num_layouts_for_key(&self, keycode: u32) -> u32 {
        self.keys
            .get(&keycode)
            .map(|key| key.groups.len() as u32)
            .unwrap_or(0)
    }

    pub(crate) fn num_levels_for_key(&self, keycode: u32, layout: u32) -> u32 {
        let Some(key) = self.keys.get(&keycode) else {
            return 0;
        };
        let Some(layout) = key.layout(layout) else {
            return 0;
        };
        key.group(layout)
            .map(|group| group.ty.num_levels as u32)
            .unwrap_or(0)
    }

    pub(crate) fn key_syms_by_level(
        &self,
        keycode: u32,
        layout: u32,
        level: u32,
    ) -> Option<&[u32]> {
        let key = self.keys.get(&keycode)?;
        let layout = key.layout(layout)?;
        let group = key.group(layout)?;
        let level = level as usize;
        if level >= group.ty.num_levels {
            return None;
        }
        let level = group.levels.get(level)?;
        if level.syms.is_empty() {
            return None;
        }
        Some(&level.syms)
    }
}
