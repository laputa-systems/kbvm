# kbvm-xkbcommon

A drop-in subset of `libxkbcommon.so.0` implemented in Rust on top of
[`kbvm`](../kbvm), the XKB implementation in this workspace. It exists so a
Wayland client (Chromium 142 in the bctl browser appliance) can link against
`libxkbcommon` without shipping the C library.

## Symbols

The cdylib exports exactly the 33 symbols the Chromium 142 browser closure
imports: the 20 that `chromium` itself imports, and the 13 more that GTK's
`libgdk-3.so.0` imports when Chromium loads it (`nm -D --undefined-only`).

```
xkb_context_new                     chromium, libgdk-3
xkb_context_unref                   chromium, libgdk-3
xkb_context_include_path_append     chromium
xkb_keymap_new_from_buffer          chromium
xkb_keymap_new_from_string          libgdk-3
xkb_keymap_new_from_names           libgdk-3
xkb_keymap_unref                    chromium, libgdk-3
xkb_keymap_min_keycode              chromium, libgdk-3
xkb_keymap_max_keycode              chromium, libgdk-3
xkb_keymap_num_mods                 chromium
xkb_keymap_mod_get_name             chromium
xkb_keymap_mod_get_index            libgdk-3
xkb_keymap_num_layouts              libgdk-3
xkb_keymap_num_layouts_for_key      chromium, libgdk-3
xkb_keymap_num_levels_for_key       chromium, libgdk-3
xkb_keymap_key_get_syms_by_level    chromium, libgdk-3
xkb_keymap_key_repeats              libgdk-3
xkb_keysym_to_utf32                 libgdk-3
xkb_state_new                       chromium, libgdk-3
xkb_state_unref                     chromium, libgdk-3
xkb_state_get_keymap                chromium
xkb_state_update_mask               chromium, libgdk-3
xkb_state_update_key                libgdk-3
xkb_state_serialize_mods            libgdk-3
xkb_state_key_get_one_sym           chromium, libgdk-3
xkb_state_key_get_syms              chromium
xkb_state_key_get_utf32             chromium
xkb_state_key_get_layout            libgdk-3
xkb_state_key_get_level             libgdk-3
xkb_state_mod_index_is_active       chromium
xkb_state_layout_index_is_active    libgdk-3
xkb_state_led_name_is_active        libgdk-3
xkb_state_mod_mask_remove_consumed  libgdk-3
```

No other `xkb_*` symbols are defined. The C declarations match
`xkbcommon/xkbcommon.h` for these functions.

## Chromium/Wayland usage

The implementation targets the client side of `wl_keyboard`:

1. The compositor sends a keymap as a text buffer (`XKB_KEYMAP_FORMAT_TEXT_V1`).
2. The client calls `xkb_keymap_new_from_buffer` (Chromium) or
   `xkb_keymap_new_from_string` (GDK) and `xkb_state_new`.
3. Modifier masks from `wl_keyboard.modifiers` are fed through
   `xkb_state_update_mask`.
4. Keycodes are XKB keycodes (evdev + 8). Lookups use
   `xkb_state_key_get_one_sym`, `xkb_state_key_get_syms`,
   `xkb_state_key_get_utf32`, and `xkb_state_mod_index_is_active`.

GDK also builds a keymap from RMLVO names when its Wayland display opens,
before the compositor's keymap arrives, and asks `xkb_state_update_key` on a
fresh state whether a key is a modifier.

Reference counting matches libxkbcommon:

- Contexts, keymaps, and states are refcounted objects. Each `_new` returns a
  reference that must be released with one `_unref`.
- A keymap holds a reference to its context; a state holds a reference to its
  keymap. Releasing the context or keymap before the child object is safe.
- `xkb_state_get_keymap` returns a borrowed pointer that stays valid as long as
  the state is alive.
- `_unref(NULL)` is a no-op.
- Invalid keymap buffers, unsupported formats/flags, and out-of-range indices
  return `NULL`, `0`, or `-1` rather than crashing. NULL handles to the
  keymap/state query functions return `0`, `-1`, `NULL`, or the `*_INVALID`
  index; libxkbcommon leaves those inputs undefined.

The keymap text is assumed to be a fully resolved map as emitted by
`xkb_keymap_get_as_string` or by `kbvm`'s formatter (what a Wayland
compositor sends). The keycode range and modifier order are read from the
keymap text because `kbvm`'s public API does not expose them; a hand-written
keymap that only makes sense through libxkbcommon's include resolution or its
fallback to `types[0]` may differ. `xkb_context_include_path_append` still
accepts an include path and it is used for keymap compilation, but the keycode
range is not recomputed from included files.

## Keymaps from names and the built-in US keymap

`xkb_keymap_new_from_names` sanitizes the names like libxkbcommon (empty or
NULL rules and model take the defaults, an empty layout takes the default
layout and variant, NULL options take the default options, `XKB_DEFAULT_*`
unless `XKB_CONTEXT_NO_ENVIRONMENT_NAMES`) and compiles them with `kbvm`.

When the rules file cannot be found in the context's include paths, which is
the case on a system without xkeyboard-config, it does not fail as upstream
does. It returns the built-in evdev/pc105/us keymap,
[`data/us.xkb`](./data/us.xkb), whatever names were requested, and writes

```
xkbcommon: WARNING: XKB rules 'evdev' not found in the include paths; using the built-in evdev/pc105/us keymap
```

to stderr. `xkb_context_new` likewise succeeds when the default XKB
directories are absent. `data/us.xkb` is libxkbcommon's own serialization of
evdev/pc105/us from xkeyboard-config 2.44, so the fallback behaves exactly like
upstream's keymap for those names. Regenerate it with
`cargo test -p kbvm-xkbcommon --test differential -- --ignored regenerate_builtin_us_keymap`.

Known differences from upstream:

- With rules present, `kbvm` orders virtual modifiers by name, so the indices
  of virtual modifiers (8 and up) differ from upstream's declaration order.
  Names, masks, and every lookup by name agree. Keymaps from text keep the
  order of the text.
- `xkb_state_update_key` runs `kbvm`'s state machine. For `LatchMods` with
  `latchToLock`, pressing the key while its modifiers are latched locks them
  on release; upstream locks on press. The state after the release is the
  same. Mixing `xkb_state_update_key` and `xkb_state_update_mask` on one
  state is unsupported, as upstream documents.

## Building

```sh
cargo build -p kbvm-xkbcommon
```

This produces the Rust `rlib` and the C library (`libxkbcommon.dylib` on macOS,
`libxkbcommon.so` on Linux). [`build.rs`](./build.rs) adds the ELF SONAME
`libxkbcommon.so.0` on Linux.

For `aarch64-unknown-linux-musl`, Rust's default static-musl configuration
refuses to emit `cdylib`. Disable static crt so the shared library is built:

```sh
RUSTFLAGS='-C target-feature=-crt-static' \
  cargo build -p kbvm-xkbcommon --release --target aarch64-unknown-linux-musl
```

The result is `target/aarch64-unknown-linux-musl/release/libxkbcommon.so` with
SONAME `libxkbcommon.so.0`. Consumers link against the SONAME, so install it
under that name (for example a `libxkbcommon.so.0 -> libxkbcommon.so` symlink).
The same command works in an arm64 Alpine container, for example:

```sh
docker run --rm --platform linux/arm64 -v "$PWD":/src -w /src \
  -e RUSTFLAGS='-C target-feature=-crt-static' rust:1.99-alpine \
  sh -c 'apk add --no-cache build-base && cargo build -p kbvm-xkbcommon --release --target aarch64-unknown-linux-musl'
```

## Differential tests

`tests/differential.rs` loads an upstream `libxkbcommon` built from source and
compares every exported function against it. The oracle must be libxkbcommon
1.9.2 or compatible.

1. Build the oracle and an XKB config tree in the worktree scratch directory:

   ```sh
   mkdir -p .work/src
   git clone --depth 1 --branch xkbcommon-1.9.2 \
     https://github.com/xkbcommon/libxkbcommon .work/src/libxkbcommon
   curl -fsSL -o .work/src/xkeyboard-config.tar.gz \
     https://gitlab.freedesktop.org/xkeyboard-config/xkeyboard-config/-/archive/xkeyboard-config-2.44/xkeyboard-config-xkeyboard-config-2.44.tar.gz
   tar -xzf .work/src/xkeyboard-config.tar.gz -C .work/src

   meson setup .work/src/xkeyboard-config-build \
     .work/src/xkeyboard-config-xkeyboard-config-2.44 -Dnls=false
   ninja -C .work/src/xkeyboard-config-build
   meson install -C .work/src/xkeyboard-config-build \
     --destdir "$PWD/.work/src/xkc-install"

   XKBROOT="$PWD/.work/src/xkc-install/opt/homebrew/share/X11/xkb"
   meson setup .work/src/libxkbcommon-build .work/src/libxkbcommon \
     -Denable-x11=false -Denable-wayland=false -Denable-docs=false \
     -Denable-tools=false -Denable-xkbregistry=false \
     -Denable-bash-completion=false -Dxkb-config-root="$XKBROOT"
   ninja -C .work/src/libxkbcommon-build
   ```

   On macOS, use a Bison newer than the system 2.3 (for example
   `brew install bison` and prepend `/opt/homebrew/opt/bison/bin` to `PATH`).
   The install destination depends on the configured prefix; adjust `XKBROOT`
   to where `rules/evdev`, `symbols/`, and `types/` end up.

2. Run the suite:

   ```sh
   cargo test -p kbvm-xkbcommon
   ```

   The oracle is located automatically at
   `.work/src/libxkbcommon-build/libxkbcommon.0.dylib` or
   `.work/src/libxkbcommon-build/libxkbcommon.so`, or at the path in
   `KBVM_XKBCOMMON_ORACLE`.

The suite builds keymaps with the oracle from RMLVO names (`us`, `de`, `fr`,
`us(dvorak)`, `us(altgr-intl)` with `lv3:ralt_switch`, and `us(intl)` for dead
keys) and asks the oracle to serialize them with `xkb_keymap_get_as_string`.
Both implementations then receive the same text buffers, and the same maps
again after a round trip through `kbvm`'s formatter (the compositor's wire
format). It compares, for every exported function:

- keymap creation from buffers, strings, and names, trailing NUL handling,
  invalid buffers, formats, and flags;
- min/max keycode, modifier count, names, and indices, layouts, levels,
  keysyms, and key repeat for keycodes 0..=300 at every layout and level;
- after `xkb_state_update_mask` for masks covering none, Shift, Caps, Level3
  (Mod5), Ctrl, virtual modifiers, and multi-group states: serialized
  modifiers, modifier, layout, and LED activity, key layout and level,
  consumed-modifier removal, and all key lookups;
- the same after every step of `xkb_state_update_key` sequences (Shift, two
  Shifts, Caps Lock, Ctrl/Alt/Super, Level3, Num Lock, group toggle, mod and
  group latches, unknown keys), and a single press on a fresh state for
  every keycode;
- `xkb_keymap_new_from_names` with xkeyboard-config present, and the built-in
  keymap without it;
- `xkb_keysym_to_utf32` over the Latin, Unicode, and vendor keysym ranges;
- context include-path return values and NULL/unref behavior;
- refcount balance and borrowed `xkb_state_get_keymap` pointers.

Additional synthetic keymaps exercise sparse groups, multi-symbol levels,
explicit types with fewer symbols than levels, a keymap with no virtual
modifiers, and mod and group latches. The oracle tree defaults to
`.work/src/xkc-install/<prefix>/share/X11/xkb`; set `KBVM_XKBCOMMON_XKB_ROOT`
to use another.
