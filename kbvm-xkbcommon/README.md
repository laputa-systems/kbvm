# kbvm-xkbcommon

A drop-in subset of `libxkbcommon.so.0` implemented in Rust on top of
[`kbvm`](../kbvm), the XKB implementation in this workspace. It exists so a
Wayland client (Chromium 142 in the bctl browser appliance) can link against
`libxkbcommon` without shipping the C library.

## Symbols

The cdylib exports exactly the symbols Chromium 142 imports:

```
xkb_context_new
xkb_context_unref
xkb_context_include_path_append
xkb_keymap_new_from_buffer
xkb_keymap_unref
xkb_keymap_min_keycode
xkb_keymap_max_keycode
xkb_keymap_num_mods
xkb_keymap_mod_get_name
xkb_keymap_num_layouts_for_key
xkb_keymap_num_levels_for_key
xkb_keymap_key_get_syms_by_level
xkb_state_new
xkb_state_unref
xkb_state_get_keymap
xkb_state_update_mask
xkb_state_key_get_one_sym
xkb_state_key_get_syms
xkb_state_key_get_utf32
xkb_state_mod_index_is_active
```

No other `xkb_*` symbols are defined. The C declarations match
`xkbcommon/xkbcommon.h` for these functions.

## Chromium/Wayland usage

The implementation targets the client side of `wl_keyboard`:

1. The compositor sends a keymap as a text buffer (`XKB_KEYMAP_FORMAT_TEXT_V1`).
2. The client calls `xkb_keymap_new_from_buffer` and `xkb_state_new`.
3. Modifier masks from `wl_keyboard.modifiers` are fed through
   `xkb_state_update_mask`; `xkb_state_update_key` is not needed.
4. Keycodes are XKB keycodes (evdev + 8). Lookups use
   `xkb_state_key_get_one_sym`, `xkb_state_key_get_syms`,
   `xkb_state_key_get_utf32`, and `xkb_state_mod_index_is_active`.

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
  keymap/state query functions return `0`/`NULL`; libxkbcommon leaves those
  inputs undefined.

The keymap text is assumed to be a fully resolved map as emitted by
`xkb_keymap_get_as_string` (which is what Wayland compositors send). The
keycode range and modifier order are read from the keymap text because
`kbvm`'s public API does not expose them; a hand-written keymap that only
makes sense through libxkbcommon's include resolution or its fallback to
`types[0]` may differ. `xkb_context_include_path_append` still accepts an
include path and it is used for keymap compilation, but the keycode range is
not recomputed from included files. Unlike upstream, `xkb_context_new` does
not fail when the default XKB data directories are absent; that is required
for the appliance, which has no XKB data installed.

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
Both implementations then receive the same text buffers. It compares:

- keymap creation, trailing NUL handling, invalid buffers, formats, and flags;
- min/max keycode, modifier count/names, layouts and levels for keycodes
  0..=300 plus boundaries;
- keysyms for every keycode 8..=255 at every layout and level;
- `xkb_state_update_mask` return values and all key lookups and modifier
  activity queries for masks covering none, Shift, Caps, Level3 (Mod5), Ctrl,
  virtual modifiers, and multi-group states;
- context include-path return values and NULL/unref behavior;
- refcount balance and borrowed `xkb_state_get_keymap` pointers.

Additional synthetic keymaps exercise sparse groups, multi-symbol levels,
explicit types with fewer symbols than levels, and a keymap with no virtual
modifiers.
