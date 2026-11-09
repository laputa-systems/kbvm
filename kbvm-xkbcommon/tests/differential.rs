//! Differential tests against an upstream libxkbcommon built from source.
//!
//! See README.md for how to build the oracle. Its location can be overridden
//! with `KBVM_XKBCOMMON_ORACLE`.

#![allow(unused_unsafe)]
#![allow(clippy::type_complexity)]

use libc::RTLD_LOCAL;
use libc::RTLD_NOW;
use libc::c_char;
use libc::c_int;
use libc::c_uint;
use libc::c_void;
use libc::dlopen;
use libc::dlsym;
use std::ffi::CStr;
use std::ffi::CString;
use std::path::Path;
use std::path::PathBuf;
use std::ptr;
use std::sync::OnceLock;

#[repr(C)]
struct RuleNames {
    rules: *const c_char,
    model: *const c_char,
    layout: *const c_char,
    variant: *const c_char,
    options: *const c_char,
}

struct Oracle {
    context_new: extern "C" fn(c_uint) -> *mut c_void,
    context_unref: extern "C" fn(*mut c_void),
    context_include_path_append: extern "C" fn(*mut c_void, *const c_char) -> c_int,
    keymap_new_from_buffer: extern "C" fn(*mut c_void, *const c_char, usize, c_int, c_int) -> *mut c_void,
    keymap_unref: extern "C" fn(*mut c_void),
    keymap_min_keycode: extern "C" fn(*mut c_void) -> u32,
    keymap_max_keycode: extern "C" fn(*mut c_void) -> u32,
    keymap_num_mods: extern "C" fn(*mut c_void) -> u32,
    keymap_mod_get_name: extern "C" fn(*mut c_void, u32) -> *const c_char,
    keymap_num_layouts_for_key: extern "C" fn(*mut c_void, u32) -> u32,
    keymap_num_levels_for_key: extern "C" fn(*mut c_void, u32, u32) -> u32,
    keymap_key_get_syms_by_level:
        extern "C" fn(*mut c_void, u32, u32, u32, *mut *const u32) -> c_int,
    state_new: extern "C" fn(*mut c_void) -> *mut c_void,
    state_unref: extern "C" fn(*mut c_void),
    state_get_keymap: extern "C" fn(*mut c_void) -> *mut c_void,
    state_update_mask: extern "C" fn(*mut c_void, u32, u32, u32, u32, u32, u32) -> c_int,
    state_key_get_one_sym: extern "C" fn(*mut c_void, u32) -> u32,
    state_key_get_syms: extern "C" fn(*mut c_void, u32, *mut *const u32) -> c_int,
    state_key_get_utf32: extern "C" fn(*mut c_void, u32) -> u32,
    state_mod_index_is_active: extern "C" fn(*mut c_void, u32, c_int) -> c_int,
    keymap_new_from_names: extern "C" fn(*mut c_void, *const RuleNames, c_int) -> *mut c_void,
    keymap_get_as_string: extern "C" fn(*mut c_void, c_int) -> *mut c_char,
}

fn oracle_path() -> PathBuf {
    if let Ok(path) = std::env::var("KBVM_XKBCOMMON_ORACLE") {
        return PathBuf::from(path);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let candidates = [
        root.join(".work/src/libxkbcommon-build/libxkbcommon.0.dylib"),
        root.join(".work/src/libxkbcommon-build/libxkbcommon.dylib"),
        root.join(".work/src/libxkbcommon-build/libxkbcommon.so.0"),
        root.join(".work/src/libxkbcommon-build/libxkbcommon.so"),
        root.join(".work/oracle/lib/libxkbcommon.so.0"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return candidate;
        }
    }
    panic!(
        "libxkbcommon oracle not found. Build it as described in \
         kbvm-xkbcommon/README.md or set KBVM_XKBCOMMON_ORACLE."
    );
}

unsafe fn load_symbol<F: Copy>(lib: *mut c_void, name: &str) -> F {
    let c_name = CString::new(name).unwrap();
    // SAFETY: `lib` is a live handle from `dlopen` and `c_name` is a
    // NUL-terminated string that lives for the call.
    let symbol = unsafe { dlsym(lib, c_name.as_ptr()) };
    assert!(!symbol.is_null(), "oracle is missing symbol {name}");
    assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*mut c_void>());
    // SAFETY: The symbol exists and has the signature of `F`, which is a
    // function pointer with the same size as the loaded address.
    unsafe { std::mem::transmute_copy(&symbol) }
}

fn oracle() -> &'static Oracle {
    static ORACLE: OnceLock<Oracle> = OnceLock::new();
    ORACLE.get_or_init(|| {
        let path = oracle_path();
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        // SAFETY: `c_path` is a valid NUL-terminated path and the handle is
        // kept open for the lifetime of the process.
        let lib = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if lib.is_null() {
            // SAFETY: `dlerror` returns a NUL-terminated string or NULL.
            let error = unsafe { libc::dlerror() };
            let error = if error.is_null() {
                String::from("unknown error")
            } else {
                // SAFETY: The pointer came from `dlerror`.
                unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned()
            };
            panic!("failed to load oracle {}: {error}", path.display());
        }
        macro_rules! load {
            ($($field:ident => $name:literal),* $(,)?) => {
                Oracle {
                    $($field: unsafe { load_symbol(lib, $name) },)*
                }
            };
        }
        load!(
            context_new => "xkb_context_new",
            context_unref => "xkb_context_unref",
            context_include_path_append => "xkb_context_include_path_append",
            keymap_new_from_buffer => "xkb_keymap_new_from_buffer",
            keymap_unref => "xkb_keymap_unref",
            keymap_min_keycode => "xkb_keymap_min_keycode",
            keymap_max_keycode => "xkb_keymap_max_keycode",
            keymap_num_mods => "xkb_keymap_num_mods",
            keymap_mod_get_name => "xkb_keymap_mod_get_name",
            keymap_num_layouts_for_key => "xkb_keymap_num_layouts_for_key",
            keymap_num_levels_for_key => "xkb_keymap_num_levels_for_key",
            keymap_key_get_syms_by_level => "xkb_keymap_key_get_syms_by_level",
            state_new => "xkb_state_new",
            state_unref => "xkb_state_unref",
            state_get_keymap => "xkb_state_get_keymap",
            state_update_mask => "xkb_state_update_mask",
            state_key_get_one_sym => "xkb_state_key_get_one_sym",
            state_key_get_syms => "xkb_state_key_get_syms",
            state_key_get_utf32 => "xkb_state_key_get_utf32",
            state_mod_index_is_active => "xkb_state_mod_index_is_active",
            keymap_new_from_names => "xkb_keymap_new_from_names",
            keymap_get_as_string => "xkb_keymap_get_as_string",
        )
    })
}

struct KeymapCase {
    name: &'static str,
    text: Vec<u8>,
}

fn optional_cstring(value: Option<&str>) -> Option<CString> {
    value.map(|v| CString::new(v).unwrap())
}

fn generate_keymap(
    oracle: &Oracle,
    rules: &str,
    model: &str,
    layout: &str,
    variant: Option<&str>,
    options: Option<&str>,
) -> Vec<u8> {
    let rules = CString::new(rules).unwrap();
    let model = CString::new(model).unwrap();
    let layout = CString::new(layout).unwrap();
    let variant = optional_cstring(variant);
    let options = optional_cstring(options);
    let names = RuleNames {
        rules: rules.as_ptr(),
        model: model.as_ptr(),
        layout: layout.as_ptr(),
        variant: variant.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
        options: options.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
    };
    // SAFETY: The oracle context is live and the rule names are valid
    // NUL-terminated strings for the duration of the calls.
    let context = unsafe { (oracle.context_new)(0) };
    assert!(!context.is_null(), "oracle xkb_context_new failed");
    // SAFETY: `context` and `names` are valid.
    let keymap = unsafe { (oracle.keymap_new_from_names)(context, &names, 0) };
    assert!(!keymap.is_null(), "oracle keymap compilation failed");
    // SAFETY: `keymap` is a live keymap and TEXT_V1 is format 1.
    let string = unsafe { (oracle.keymap_get_as_string)(keymap, 1) };
    assert!(!string.is_null(), "oracle xkb_keymap_get_as_string failed");
    // SAFETY: The returned string is NUL-terminated and owned by us.
    let text = unsafe { CStr::from_ptr(string) }.to_bytes().to_vec();
    // SAFETY: The string was allocated by the oracle with malloc.
    unsafe { libc::free(string as *mut c_void) };
    // SAFETY: Both pointers are live and unref'd exactly once.
    unsafe {
        (oracle.keymap_unref)(keymap);
        (oracle.context_unref)(context);
    }
    text
}

fn keymap_cases() -> &'static [KeymapCase] {
    static CASES: OnceLock<Vec<KeymapCase>> = OnceLock::new();
    CASES.get_or_init(|| {
        let oracle = oracle();
        let definitions: &[(&str, &str, &str, &str, Option<&str>, Option<&str>)] = &[
            ("us", "evdev", "pc105", "us", None, None),
            ("de", "evdev", "pc105", "de", None, None),
            ("fr", "evdev", "pc105", "fr", None, None),
            ("us-dvorak", "evdev", "pc105", "us", Some("dvorak"), None),
            (
                "us-altgr-intl-lv3",
                "evdev",
                "pc105",
                "us",
                Some("altgr-intl"),
                Some("lv3:ralt_switch"),
            ),
            ("us-intl-dead", "evdev", "pc105", "us", Some("intl"), None),
        ];
        let mut cases: Vec<KeymapCase> = definitions
            .iter()
            .map(|&(name, rules, model, layout, variant, options)| KeymapCase {
                name,
                text: generate_keymap(oracle, rules, model, layout, variant, options),
            })
            .collect();
        cases.extend(synthetic_keymaps());
        cases
    })
}

fn synthetic_keymaps() -> Vec<KeymapCase> {
    let sparse = br#"
        xkb_keymap {
            xkb_keycodes {
                <ESC> = 9;
                <AE01> = 10;
                <AE02> = 11;
                <AE03> = 12;
                <AE04> = 13;
                <AE05> = 14;
                <AE06> = 15;
                <LFSH> = 50;
                <RALT> = 108;
            };
            xkb_types {
                virtual_modifiers LevelThree = Mod5;
                type "ONE_LEVEL" {
                    modifiers = None;
                    map[None] = Level1;
                };
                type "TWO_LEVEL" {
                    modifiers = Shift;
                    map[Shift] = Level2;
                };
                type "FOUR_LEVEL" {
                    modifiers = Shift+LevelThree;
                    map[Shift] = Level2;
                    map[LevelThree] = Level3;
                    map[Shift+LevelThree] = Level4;
                };
            };
            xkb_compat {
                interpret ISO_Level3_Shift { virtualmodifier = LevelThree; };
            };
            xkb_symbols {
                key <ESC> { type[Group1] = "TWO_LEVEL", [ Escape ] };
                key <AE01> { type[Group1] = "FOUR_LEVEL", [ 1, exclam ] };
                key <AE02> { type[Group1] = "TWO_LEVEL", symbols[Group1] = [ { a, b }, { C, D } ] };
                key <AE03> { type[Group1] = "FOUR_LEVEL", symbols[Group1] = [ 3, numbersign ], symbols[Group3] = [ 4, dollar ] };
                key <AE04> { type[Group1] = "ONE_LEVEL", type[Group2] = "ONE_LEVEL", symbols[Group1] = [ ssharp ], symbols[Group2] = [ s ] };
                key <AE05> { groupsClamp = true, type[Group1] = "ONE_LEVEL", type[Group2] = "ONE_LEVEL", symbols[Group1] = [ x ], symbols[Group2] = [ y ] };
                key <AE06> { groupsRedirect = Group2, type[Group1] = "ONE_LEVEL", type[Group2] = "ONE_LEVEL", symbols[Group1] = [ p ], symbols[Group2] = [ q ] };
                key <LFSH> { type[Group1] = "TWO_LEVEL", [ Shift_L ] };
                key <RALT> { type[Group1] = "TWO_LEVEL", [ ISO_Level3_Shift ] };
            };
        };
    "#;
    let no_vmods = br#"
        xkb_keymap {
            xkb_keycodes {
                <a> = 38;
                <lshift> = 50;
            };
            xkb_types {
                type "TWO_LEVEL" {
                    modifiers = Shift;
                    map[Shift] = Level2;
                };
            };
            xkb_compat { };
            xkb_symbols {
                key <a> { type[Group1] = "TWO_LEVEL", [ a, A ] };
                key <lshift> { type[Group1] = "TWO_LEVEL", [ Shift_L ] };
            };
        };
    "#;
    vec![
        KeymapCase {
            name: "synthetic-sparse-groups",
            text: sparse.to_vec(),
        },
        KeymapCase {
            name: "synthetic-no-vmods",
            text: no_vmods.to_vec(),
        },
    ]
}

fn hex_keymap(name: &str) -> Vec<u8> {
    keymap_cases()
        .iter()
        .find(|c| c.name == name)
        .unwrap()
        .text
        .clone()
}

fn create_keymap(oracle: &Oracle, context: *mut c_void, text: &[u8]) -> *mut c_void {
    // SAFETY: The pointers are valid; `text` lives for the duration of the
    // call and TEXT_V1 is format 1.
    unsafe {
        (oracle.keymap_new_from_buffer)(
            context,
            text.as_ptr() as *const c_char,
            text.len(),
            1,
            0,
        )
    }
}

fn our_keymap(
    context: *mut xkbcommon::abi::xkb_context,
    text: &[u8],
) -> *mut xkbcommon::abi::xkb_keymap {
    xkbcommon::abi::xkb_keymap_new_from_buffer(
        context,
        text.as_ptr() as *const c_char,
        text.len(),
        1,
        0,
    )
}

fn syms_at(syms: *const u32, count: c_int) -> Option<Vec<u32>> {
    if count <= 0 {
        return None;
    }
    assert!(!syms.is_null(), "non-zero keysym count with NULL pointer");
    // SAFETY: The implementation promises `count` keysyms at the pointer.
    Some(unsafe { std::slice::from_raw_parts(syms, count as usize) }.to_vec())
}

fn c_string(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: The implementation returns NUL-terminated strings or NULL.
    Some(
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn assert_same<T: PartialEq + std::fmt::Debug>(what: &str, case: &str, oracle: T, ours: T) {
    assert_eq!(oracle, ours, "{what} differs for {case}");
}

#[test]
fn differential_context() {
    let oracle = oracle();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"));
    let existing = CString::new(path.to_str().unwrap()).unwrap();
    let missing = CString::new("/definitely/not/a/real/xkb/directory").unwrap();
    let file = CString::new(path.join("Cargo.toml").to_str().unwrap()).unwrap();

    for flags in [0u32, 1, 2, 3] {
        // SAFETY: The argument is a plain integer flag.
        let oracle_context = unsafe { (oracle.context_new)(flags) };
        let our_context = xkbcommon::abi::xkb_context_new(flags);
        assert_eq!(
            oracle_context.is_null(),
            our_context.is_null(),
            "xkb_context_new({flags}) nullness differs"
        );
        if !oracle_context.is_null() {
            // SAFETY: The contexts are live and unref'd once.
            unsafe {
                (oracle.context_unref)(oracle_context);
                xkbcommon::abi::xkb_context_unref(our_context);
            }
        }
    }

    // SAFETY: All pointers are live context handles.
    unsafe {
        let oracle_context = (oracle.context_new)(0);
        let our_context = xkbcommon::abi::xkb_context_new(0);
        for candidate in [&existing, &missing, &file] {
            let oracle_result =
                (oracle.context_include_path_append)(oracle_context, candidate.as_ptr());
            let our_result = xkbcommon::abi::xkb_context_include_path_append(
                our_context,
                candidate.as_ptr(),
            );
            assert_eq!(
                oracle_result, our_result,
                "xkb_context_include_path_append({}) differs",
                candidate.to_string_lossy()
            );
            assert_eq!(oracle_result, i32::from(candidate == &existing));
        }
        // The appended include path does not interfere with map compilation.
        let text = hex_keymap("us");
        let oracle_keymap = create_keymap(oracle, oracle_context, &text);
        let our_keymap_ptr = our_keymap(our_context, &text);
        assert!(!oracle_keymap.is_null());
        assert!(!our_keymap_ptr.is_null());
        assert_eq!(
            (oracle.keymap_min_keycode)(oracle_keymap),
            xkbcommon::abi::xkb_keymap_min_keycode(our_keymap_ptr)
        );
        assert_eq!(
            (oracle.keymap_max_keycode)(oracle_keymap),
            xkbcommon::abi::xkb_keymap_max_keycode(our_keymap_ptr)
        );
        (oracle.keymap_unref)(oracle_keymap);
        xkbcommon::abi::xkb_keymap_unref(our_keymap_ptr);
        // NULL is accepted by both unref implementations.
        (oracle.context_unref)(ptr::null_mut());
        xkbcommon::abi::xkb_context_unref(ptr::null_mut());
        (oracle.context_unref)(oracle_context);
        xkbcommon::abi::xkb_context_unref(our_context);
    }
}

#[test]
fn differential_keymap_properties() {
    let oracle = oracle();
    for case in keymap_cases() {
        // SAFETY: All pointers are valid and released once.
        unsafe {
            let oracle_context = (oracle.context_new)(0);
            let our_context = xkbcommon::abi::xkb_context_new(0);
            assert!(!oracle_context.is_null());
            assert!(!our_context.is_null());
            let oracle_keymap = create_keymap(oracle, oracle_context, &case.text);
            let our_keymap_ptr = our_keymap(our_context, &case.text);
            assert!(!oracle_keymap.is_null(), "oracle failed on {}", case.name);
            assert!(
                !our_keymap_ptr.is_null(),
                "our implementation failed on {}",
                case.name
            );
            // The context is referenced by the keymap and can be released.
            (oracle.context_unref)(oracle_context);
            xkbcommon::abi::xkb_context_unref(our_context);

            let what = |s: &str| format!("{} ({})", s, case.name);
            assert_same(
                &what("xkb_keymap_min_keycode"),
                case.name,
                (oracle.keymap_min_keycode)(oracle_keymap),
                xkbcommon::abi::xkb_keymap_min_keycode(
                    our_keymap_ptr,
                ),
            );
            assert_same(
                &what("xkb_keymap_max_keycode"),
                case.name,
                (oracle.keymap_max_keycode)(oracle_keymap),
                xkbcommon::abi::xkb_keymap_max_keycode(
                    our_keymap_ptr,
                ),
            );
            let oracle_mods = (oracle.keymap_num_mods)(oracle_keymap);
            let our_mods =
                xkbcommon::abi::xkb_keymap_num_mods(our_keymap_ptr);
            assert_same(&what("xkb_keymap_num_mods"), case.name, oracle_mods, our_mods);
            for idx in 0..oracle_mods + 2 {
                assert_same(
                    &what(&format!("xkb_keymap_mod_get_name({idx})")),
                    case.name,
                    c_string((oracle.keymap_mod_get_name)(oracle_keymap, idx)),
                    c_string(xkbcommon::abi::xkb_keymap_mod_get_name(
                        our_keymap_ptr,
                        idx,
                    )),
                );
            }

            // A terminating NUL is not part of the keymap.
            let mut with_nul = case.text.clone();
            with_nul.push(0);
            let oracle_context = (oracle.context_new)(0);
            let oracle_nul = (oracle.keymap_new_from_buffer)(
                oracle_context,
                with_nul.as_ptr() as *const c_char,
                with_nul.len(),
                1,
                0,
            );
            assert!(!oracle_nul.is_null(), "oracle rejected trailing NUL");
            let our_context = xkbcommon::abi::xkb_context_new(0);
            let our_nul = xkbcommon::abi::xkb_keymap_new_from_buffer(
                our_context,
                with_nul.as_ptr() as *const c_char,
                with_nul.len(),
                1,
                0,
            );
            assert!(!our_nul.is_null(), "we rejected trailing NUL");
            (oracle.keymap_unref)(oracle_nul);
            xkbcommon::abi::xkb_keymap_unref(our_nul);
            (oracle.context_unref)(oracle_context);
            xkbcommon::abi::xkb_context_unref(our_context);

            // Invalid inputs must yield NULL without crashing.
            let garbage = b"this is not a keymap";
            let empty: [u8; 0] = [];
            let invalid_context_oracle = (oracle.context_new)(0);
            let invalid_context_ours = xkbcommon::abi::xkb_context_new(0);
            let oracle_null = (oracle.keymap_new_from_buffer)(
                invalid_context_oracle,
                ptr::null(),
                0,
                1,
                0,
            );
            let our_null = xkbcommon::abi::xkb_keymap_new_from_buffer(
                invalid_context_ours,
                ptr::null(),
                0,
                1,
                0,
            );
            assert_eq!(oracle_null.is_null(), our_null.is_null());
            assert!(oracle_null.is_null());
            let oracle_garbage = (oracle.keymap_new_from_buffer)(
                invalid_context_oracle,
                garbage.as_ptr() as *const c_char,
                garbage.len(),
                1,
                0,
            );
            let our_garbage = xkbcommon::abi::xkb_keymap_new_from_buffer(
                invalid_context_ours,
                garbage.as_ptr() as *const c_char,
                garbage.len(),
                1,
                0,
            );
            assert_eq!(oracle_garbage.is_null(), our_garbage.is_null());
            assert!(oracle_garbage.is_null());
            let oracle_empty = (oracle.keymap_new_from_buffer)(
                invalid_context_oracle,
                empty.as_ptr() as *const c_char,
                0,
                1,
                0,
            );
            let our_empty = xkbcommon::abi::xkb_keymap_new_from_buffer(
                invalid_context_ours,
                empty.as_ptr() as *const c_char,
                0,
                1,
                0,
            );
            assert_eq!(oracle_empty.is_null(), our_empty.is_null());
            assert!(oracle_empty.is_null());
            for (format, flags) in [(0, 0), (2, 0), (1, 1)] {
                let oracle_invalid = (oracle.keymap_new_from_buffer)(
                    invalid_context_oracle,
                    case.text.as_ptr() as *const c_char,
                    case.text.len(),
                    format,
                    flags,
                );
                let our_invalid = xkbcommon::abi::xkb_keymap_new_from_buffer(
                    invalid_context_ours,
                    case.text.as_ptr() as *const c_char,
                    case.text.len(),
                    format,
                    flags,
                );
                assert_eq!(
                    oracle_invalid.is_null(),
                    our_invalid.is_null(),
                    "format={format} flags={flags}"
                );
                assert!(oracle_invalid.is_null());
            }
            (oracle.context_unref)(invalid_context_oracle);
            xkbcommon::abi::xkb_context_unref(invalid_context_ours);

            (oracle.keymap_unref)(oracle_keymap);
            xkbcommon::abi::xkb_keymap_unref(our_keymap_ptr);
        }
    }
}

#[test]
fn differential_keymap_layouts_and_levels() {
    let oracle = oracle();
    for case in keymap_cases() {
        // SAFETY: All pointers are valid and released once.
        unsafe {
            let oracle_context = (oracle.context_new)(0);
            let our_context = xkbcommon::abi::xkb_context_new(0);
            let oracle_keymap = create_keymap(oracle, oracle_context, &case.text);
            let our_keymap_ptr = our_keymap(our_context, &case.text);
            assert!(!oracle_keymap.is_null());
            assert!(!our_keymap_ptr.is_null());
            let keycodes = (0u32..=300).chain(700..=710).chain([1000]);
            for keycode in keycodes {
                assert_same(
                    "xkb_keymap_num_layouts_for_key",
                    &format!("{} kc={keycode}", case.name),
                    (oracle.keymap_num_layouts_for_key)(oracle_keymap, keycode),
                    xkbcommon::abi::xkb_keymap_num_layouts_for_key(our_keymap_ptr, keycode),
                );
                for layout in [0u32, 1, 2, 3, u32::MAX] {
                    assert_same(
                        "xkb_keymap_num_levels_for_key",
                        &format!("{} kc={keycode} layout={layout}", case.name),
                        (oracle.keymap_num_levels_for_key)(oracle_keymap, keycode, layout),
                        xkbcommon::abi::xkb_keymap_num_levels_for_key(
                            our_keymap_ptr,
                            keycode,
                            layout,
                        ),
                    );
                }
            }
            (oracle.keymap_unref)(oracle_keymap);
            xkbcommon::abi::xkb_keymap_unref(our_keymap_ptr);
            (oracle.context_unref)(oracle_context);
            xkbcommon::abi::xkb_context_unref(our_context);
        }
    }
}

#[test]
fn differential_key_get_syms_by_level() {
    let oracle = oracle();
    for case in keymap_cases() {
        // SAFETY: All pointers are valid and released once.
        unsafe {
            let oracle_context = (oracle.context_new)(0);
            let our_context = xkbcommon::abi::xkb_context_new(0);
            let oracle_keymap = create_keymap(oracle, oracle_context, &case.text);
            let our_keymap_ptr = our_keymap(our_context, &case.text);
            assert!(!oracle_keymap.is_null());
            assert!(!our_keymap_ptr.is_null());
            for keycode in 8u32..=255 {
                for layout in [0u32, 1, 2, 3, u32::MAX] {
                    for level in 0u32..=5 {
                        let mut oracle_syms: *const u32 = ptr::null();
                        let mut our_syms: *const u32 = ptr::null();
                        let oracle_count = (oracle.keymap_key_get_syms_by_level)(
                            oracle_keymap,
                            keycode,
                            layout,
                            level,
                            &mut oracle_syms,
                        );
                        let our_count = xkbcommon::abi::xkb_keymap_key_get_syms_by_level(
                            our_keymap_ptr,
                            keycode,
                            layout,
                            level,
                            &mut our_syms,
                        );
                        assert_eq!(
                            oracle_count, our_count,
                            "count differs for {} kc={keycode} layout={layout} level={level}",
                            case.name
                        );
                        assert_eq!(
                            oracle_syms.is_null(),
                            our_syms.is_null(),
                            "pointer nullness differs for {} kc={keycode} layout={layout} level={level}",
                            case.name
                        );
                        assert_eq!(
                            syms_at(oracle_syms, oracle_count),
                            syms_at(our_syms, our_count),
                            "syms differ for {} kc={keycode} layout={layout} level={level}",
                            case.name
                        );
                    }
                }
            }
            (oracle.keymap_unref)(oracle_keymap);
            xkbcommon::abi::xkb_keymap_unref(our_keymap_ptr);
            (oracle.context_unref)(oracle_context);
            xkbcommon::abi::xkb_context_unref(our_context);
        }
    }
}

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    base_mods: u32,
    latched_mods: u32,
    locked_mods: u32,
    base_group: u32,
    latched_group: u32,
    locked_group: u32,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "none",
        base_mods: 0,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "shift",
        base_mods: 1,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "caps",
        base_mods: 0,
        latched_mods: 0,
        locked_mods: 2,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "level3",
        base_mods: 0,
        latched_mods: 0,
        locked_mods: 0x80,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "ctrl",
        base_mods: 4,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "shift+caps",
        base_mods: 1,
        latched_mods: 0,
        locked_mods: 2,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "caps+ctrl",
        base_mods: 4,
        latched_mods: 0,
        locked_mods: 2,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "shift+level3",
        base_mods: 1,
        latched_mods: 0,
        locked_mods: 0x80,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "latched-mods",
        base_mods: 1,
        latched_mods: 0x80,
        locked_mods: 0,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "groups",
        base_mods: 0,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 1,
        latched_group: 1,
        locked_group: 1,
    },
    Scenario {
        name: "group-1",
        base_mods: 0,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 1,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "group-2",
        base_mods: 0,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 2,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "out-of-range-groups",
        base_mods: 0,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 7,
        latched_group: 3,
        locked_group: 0xffff_ffff,
    },
    Scenario {
        name: "vmod-8",
        base_mods: 1 << 8,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "vmod-9",
        base_mods: 1 << 9,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "vmod-10",
        base_mods: 1 << 10,
        latched_mods: 0,
        locked_mods: 0,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "vmod-11-latched",
        base_mods: 0,
        latched_mods: 1 << 11,
        locked_mods: 0,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
    Scenario {
        name: "vmod-12-locked",
        base_mods: 0,
        latched_mods: 0,
        locked_mods: 1 << 12,
        base_group: 0,
        latched_group: 0,
        locked_group: 0,
    },
];

#[test]
fn differential_state_queries() {
    let oracle = oracle();
    for case in keymap_cases() {
        // SAFETY: All pointers are valid and released once.
        unsafe {
            let oracle_context = (oracle.context_new)(0);
            let our_context = xkbcommon::abi::xkb_context_new(0);
            let oracle_keymap = create_keymap(oracle, oracle_context, &case.text);
            let our_keymap_ptr = our_keymap(our_context, &case.text);
            assert!(!oracle_keymap.is_null());
            assert!(!our_keymap_ptr.is_null());
            let oracle_state = (oracle.state_new)(oracle_keymap);
            let our_state_ptr = xkbcommon::abi::xkb_state_new(our_keymap_ptr);
            assert!(!oracle_state.is_null());
            assert!(!our_state_ptr.is_null());
            assert_eq!(
                (oracle.state_get_keymap)(oracle_state),
                oracle_keymap,
                "oracle state does not return its keymap"
            );
            assert_eq!(
                xkbcommon::abi::xkb_state_get_keymap(our_state_ptr),
                our_keymap_ptr,
                "our state does not return its keymap"
            );

            let num_mods = (oracle.keymap_num_mods)(oracle_keymap);
            for scenario in SCENARIOS {
                let oracle_change = (oracle.state_update_mask)(
                    oracle_state,
                    scenario.base_mods,
                    scenario.latched_mods,
                    scenario.locked_mods,
                    scenario.base_group,
                    scenario.latched_group,
                    scenario.locked_group,
                );
                let our_change = xkbcommon::abi::xkb_state_update_mask(
                    our_state_ptr,
                    scenario.base_mods,
                    scenario.latched_mods,
                    scenario.locked_mods,
                    scenario.base_group,
                    scenario.latched_group,
                    scenario.locked_group,
                );
                assert_same(
                    "xkb_state_update_mask",
                    &format!("{} scenario={}", case.name, scenario.name),
                    oracle_change,
                    our_change,
                );
                // Updating with the same values again changes nothing.
                let oracle_again = (oracle.state_update_mask)(
                    oracle_state,
                    scenario.base_mods,
                    scenario.latched_mods,
                    scenario.locked_mods,
                    scenario.base_group,
                    scenario.latched_group,
                    scenario.locked_group,
                );
                let our_again = xkbcommon::abi::xkb_state_update_mask(
                    our_state_ptr,
                    scenario.base_mods,
                    scenario.latched_mods,
                    scenario.locked_mods,
                    scenario.base_group,
                    scenario.latched_group,
                    scenario.locked_group,
                );
                assert_same(
                    "xkb_state_update_mask (repeat)",
                    &format!("{} scenario={}", case.name, scenario.name),
                    oracle_again,
                    our_again,
                );
                assert_eq!(oracle_again, 0);
                for idx in 0..num_mods + 2 {
                    for affect in [1i32, 2, 4, 8, 7, 15, 24] {
                        assert_same(
                            "xkb_state_mod_index_is_active",
                            &format!(
                                "{} scenario={} idx={idx} affect={affect}",
                                case.name, scenario.name
                            ),
                            (oracle.state_mod_index_is_active)(oracle_state, idx, affect),
                            xkbcommon::abi::xkb_state_mod_index_is_active(
                                our_state_ptr, idx, affect,
                            ),
                        );
                    }
                }
                for keycode in 8u32..=255 {
                    assert_same(
                        "xkb_state_key_get_one_sym",
                        &format!(
                            "{} scenario={} kc={keycode}",
                            case.name, scenario.name
                        ),
                        (oracle.state_key_get_one_sym)(oracle_state, keycode),
                        xkbcommon::abi::xkb_state_key_get_one_sym(our_state_ptr, keycode),
                    );
                    let mut oracle_syms: *const u32 = ptr::null();
                    let mut our_syms: *const u32 = ptr::null();
                    let oracle_count =
                        (oracle.state_key_get_syms)(oracle_state, keycode, &mut oracle_syms);
                    let our_count = xkbcommon::abi::xkb_state_key_get_syms(
                        our_state_ptr,
                        keycode,
                        &mut our_syms,
                    );
                    assert_eq!(
                        oracle_count, our_count,
                        "state syms count differs for {} scenario={} kc={keycode}",
                        case.name, scenario.name
                    );
                    assert_eq!(
                        oracle_syms.is_null(),
                        our_syms.is_null(),
                        "state syms pointer differs for {} scenario={} kc={keycode}",
                        case.name,
                        scenario.name
                    );
                    assert_eq!(
                        syms_at(oracle_syms, oracle_count),
                        syms_at(our_syms, our_count),
                        "state syms differ for {} scenario={} kc={keycode}",
                        case.name,
                        scenario.name
                    );
                    assert_same(
                        "xkb_state_key_get_utf32",
                        &format!(
                            "{} scenario={} kc={keycode}",
                            case.name, scenario.name
                        ),
                        (oracle.state_key_get_utf32)(oracle_state, keycode),
                        xkbcommon::abi::xkb_state_key_get_utf32(our_state_ptr, keycode),
                    );
                }
            }
            (oracle.state_unref)(oracle_state);
            xkbcommon::abi::xkb_state_unref(our_state_ptr);
            (oracle.keymap_unref)(oracle_keymap);
            xkbcommon::abi::xkb_keymap_unref(our_keymap_ptr);
            (oracle.context_unref)(oracle_context);
            xkbcommon::abi::xkb_context_unref(our_context);
        }
    }
}

#[test]
fn differential_null_and_invalid_inputs() {
    let oracle = oracle();
    // SAFETY: NULL is explicitly accepted by all unref functions.
    unsafe {
        (oracle.state_unref)(ptr::null_mut());
        xkbcommon::abi::xkb_state_unref(ptr::null_mut());
        (oracle.keymap_unref)(ptr::null_mut());
        xkbcommon::abi::xkb_keymap_unref(ptr::null_mut());
        (oracle.context_unref)(ptr::null_mut());
        xkbcommon::abi::xkb_context_unref(ptr::null_mut());
    }
    // Our implementation must be safe on NULL handles; upstream would
    // dereference them, so there is nothing to compare against.
    assert_eq!(
        xkbcommon::abi::xkb_keymap_min_keycode(ptr::null_mut()),
        0
    );
    assert_eq!(
        xkbcommon::abi::xkb_keymap_max_keycode(ptr::null_mut()),
        0
    );
    assert_eq!(xkbcommon::abi::xkb_keymap_num_mods(ptr::null_mut()), 0);
    assert!(xkbcommon::abi::xkb_keymap_mod_get_name(ptr::null_mut(), 0).is_null());
    assert_eq!(
        xkbcommon::abi::xkb_keymap_num_layouts_for_key(ptr::null_mut(), 8),
        0
    );
    assert_eq!(
        xkbcommon::abi::xkb_keymap_num_levels_for_key(ptr::null_mut(), 8, 0),
        0
    );
    let mut syms = ptr::null();
    assert_eq!(
        xkbcommon::abi::xkb_keymap_key_get_syms_by_level(
            ptr::null_mut(),
            8,
            0,
            0,
            &mut syms
        ),
        0
    );
    assert!(syms.is_null());
    assert!(xkbcommon::abi::xkb_state_new(ptr::null_mut()).is_null());
    assert!(xkbcommon::abi::xkb_state_get_keymap(ptr::null_mut()).is_null());
    assert_eq!(
        xkbcommon::abi::xkb_state_update_mask(ptr::null_mut(), 0, 0, 0, 0, 0, 0),
        0
    );
    assert_eq!(
        xkbcommon::abi::xkb_state_key_get_one_sym(ptr::null_mut(), 8),
        0
    );
    let mut syms = ptr::null();
    assert_eq!(
        xkbcommon::abi::xkb_state_key_get_syms(ptr::null_mut(), 8, &mut syms),
        0
    );
    assert!(syms.is_null());
    assert_eq!(xkbcommon::abi::xkb_state_key_get_utf32(ptr::null_mut(), 8), 0);
    assert_eq!(
        xkbcommon::abi::xkb_state_mod_index_is_active(ptr::null_mut(), 0, 8),
        -1
    );
    assert_eq!(
        xkbcommon::abi::xkb_context_include_path_append(ptr::null_mut(), ptr::null()),
        0
    );

    let text = hex_keymap("us");
    // SAFETY: All pointers are valid and released once.
    unsafe {
        let oracle_context = (oracle.context_new)(0);
        let our_context = xkbcommon::abi::xkb_context_new(0);
        let oracle_keymap = create_keymap(oracle, oracle_context, &text);
        let our_keymap_ptr = our_keymap(our_context, &text);
        assert!(!oracle_keymap.is_null());
        assert!(!our_keymap_ptr.is_null());
        let num_mods = (oracle.keymap_num_mods)(oracle_keymap);
        assert!(
            (oracle.keymap_mod_get_name)(oracle_keymap, num_mods).is_null(),
            "oracle mod_get_name did not return NULL"
        );
        assert!(
            xkbcommon::abi::xkb_keymap_mod_get_name(our_keymap_ptr, num_mods).is_null()
        );
        let oracle_state = (oracle.state_new)(oracle_keymap);
        let our_state = xkbcommon::abi::xkb_state_new(our_keymap_ptr);
        assert!(!oracle_state.is_null());
        assert!(!our_state.is_null());
        assert_eq!(
            (oracle.state_mod_index_is_active)(oracle_state, num_mods, 8),
            -1
        );
        assert_eq!(
            xkbcommon::abi::xkb_state_mod_index_is_active(our_state, num_mods, 8),
            -1
        );
        for keycode in [0u32, 7, 709, 1000, u32::MAX] {
            let mut oracle_syms: *const u32 = ptr::dangling::<u32>();
            let mut our_syms: *const u32 = ptr::dangling::<u32>();
            let oracle_count =
                (oracle.state_key_get_syms)(oracle_state, keycode, &mut oracle_syms);
            let our_count =
                xkbcommon::abi::xkb_state_key_get_syms(our_state, keycode, &mut our_syms);
            assert_eq!(oracle_count, 0, "oracle count for kc={keycode}");
            assert_eq!(our_count, 0, "our count for kc={keycode}");
            assert!(oracle_syms.is_null(), "oracle pointer for kc={keycode}");
            assert!(our_syms.is_null(), "our pointer for kc={keycode}");
            assert_eq!((oracle.state_key_get_one_sym)(oracle_state, keycode), 0);
            assert_eq!(
                xkbcommon::abi::xkb_state_key_get_one_sym(our_state, keycode),
                0
            );
            assert_eq!((oracle.state_key_get_utf32)(oracle_state, keycode), 0);
            assert_eq!(
                xkbcommon::abi::xkb_state_key_get_utf32(our_state, keycode),
                0
            );
            let mut oracle_syms: *const u32 = ptr::dangling::<u32>();
            let mut our_syms: *const u32 = ptr::dangling::<u32>();
            let oracle_count =
                (oracle.keymap_key_get_syms_by_level)(oracle_keymap, keycode, 0, 0, &mut oracle_syms);
            let our_count = xkbcommon::abi::xkb_keymap_key_get_syms_by_level(
                our_keymap_ptr,
                keycode,
                0,
                0,
                &mut our_syms,
            );
            assert_eq!(oracle_count, 0);
            assert_eq!(our_count, 0);
            assert!(oracle_syms.is_null());
            assert!(our_syms.is_null());
        }
        (oracle.state_unref)(oracle_state);
        xkbcommon::abi::xkb_state_unref(our_state);
        (oracle.keymap_unref)(oracle_keymap);
        xkbcommon::abi::xkb_keymap_unref(our_keymap_ptr);
        (oracle.context_unref)(oracle_context);
        xkbcommon::abi::xkb_context_unref(our_context);
    }
}

#[test]
fn differential_lifetimes() {
    let oracle = oracle();
    let text = hex_keymap("us");
    // SAFETY: Every object is live when used and released exactly once.
    unsafe {
        let oracle_context = (oracle.context_new)(0);
        let our_context = xkbcommon::abi::xkb_context_new(0);
        let oracle_keymap = create_keymap(oracle, oracle_context, &text);
        let our_keymap_ptr = our_keymap(our_context, &text);
        assert!(!oracle_keymap.is_null());
        assert!(!our_keymap_ptr.is_null());
        // Keymaps keep their context alive.
        (oracle.context_unref)(oracle_context);
        xkbcommon::abi::xkb_context_unref(our_context);
        assert_eq!(
            (oracle.keymap_min_keycode)(oracle_keymap),
            xkbcommon::abi::xkb_keymap_min_keycode(our_keymap_ptr)
        );
        let oracle_state = (oracle.state_new)(oracle_keymap);
        let our_state = xkbcommon::abi::xkb_state_new(our_keymap_ptr);
        // States keep their keymap alive and return the borrowed pointer.
        (oracle.keymap_unref)(oracle_keymap);
        xkbcommon::abi::xkb_keymap_unref(our_keymap_ptr);
        assert_eq!((oracle.state_get_keymap)(oracle_state), oracle_keymap);
        assert_eq!(
            xkbcommon::abi::xkb_state_get_keymap(our_state),
            our_keymap_ptr
        );
        assert_eq!(
            (oracle.keymap_min_keycode)((oracle.state_get_keymap)(oracle_state)),
            xkbcommon::abi::xkb_keymap_min_keycode(
                xkbcommon::abi::xkb_state_get_keymap(our_state)
            )
        );
        let oracle_change = (oracle.state_update_mask)(oracle_state, 1, 0, 0, 0, 0, 0);
        let our_change = xkbcommon::abi::xkb_state_update_mask(our_state, 1, 0, 0, 0, 0, 0);
        assert_eq!(oracle_change, our_change);
        for keycode in 8u32..=255 {
            assert_eq!(
                (oracle.state_key_get_one_sym)(oracle_state, keycode),
                xkbcommon::abi::xkb_state_key_get_one_sym(our_state, keycode)
            );
            assert_eq!(
                (oracle.state_key_get_utf32)(oracle_state, keycode),
                xkbcommon::abi::xkb_state_key_get_utf32(our_state, keycode)
            );
        }
        (oracle.state_unref)(oracle_state);
        xkbcommon::abi::xkb_state_unref(our_state);
    }

    // Refcount balance stress: create and release in every order.
    for _ in 0..200 {
        // SAFETY: All objects are released exactly once in each ordering.
        unsafe {
            let context = xkbcommon::abi::xkb_context_new(0);
            let keymap = our_keymap(context, &text);
            let state = xkbcommon::abi::xkb_state_new(keymap);
            xkbcommon::abi::xkb_state_unref(state);
            xkbcommon::abi::xkb_keymap_unref(keymap);
            xkbcommon::abi::xkb_context_unref(context);

            let context = xkbcommon::abi::xkb_context_new(0);
            let keymap = our_keymap(context, &text);
            let state = xkbcommon::abi::xkb_state_new(keymap);
            xkbcommon::abi::xkb_context_unref(context);
            xkbcommon::abi::xkb_keymap_unref(keymap);
            xkbcommon::abi::xkb_state_unref(state);

            let context = xkbcommon::abi::xkb_context_new(0);
            let keymap = our_keymap(context, &text);
            let state = xkbcommon::abi::xkb_state_new(keymap);
            xkbcommon::abi::xkb_keymap_unref(keymap);
            xkbcommon::abi::xkb_state_unref(state);
            xkbcommon::abi::xkb_context_unref(context);
        }
    }
}

#[test]
fn fixtures_exercise_levels_and_dead_keys() {
    let oracle = oracle();
    let mut saw_level3_shift = false;
    let mut saw_dead_key = false;
    let mut level3_changed_text = false;
    for case in keymap_cases() {
        // SAFETY: All pointers are valid and released once.
        unsafe {
            let context = (oracle.context_new)(0);
            let keymap = create_keymap(oracle, context, &case.text);
            assert!(!keymap.is_null());
            let state = (oracle.state_new)(keymap);
            (oracle.state_update_mask)(state, 0x80, 0, 0, 0, 0, 0);
            for keycode in 8u32..=255 {
                let layouts = (oracle.keymap_num_layouts_for_key)(keymap, keycode);
                for layout in 0..layouts {
                    let levels = (oracle.keymap_num_levels_for_key)(keymap, keycode, layout);
                    for level in 0..levels {
                        let mut syms: *const u32 = ptr::null();
                        let count = (oracle.keymap_key_get_syms_by_level)(
                            keymap, keycode, layout, level, &mut syms,
                        );
                        if count <= 0 {
                            continue;
                        }
                        for &sym in std::slice::from_raw_parts(syms, count as usize) {
                            if sym == 0xfe03 {
                                saw_level3_shift = true;
                            }
                            if let Some(name) = kbvm::Keysym(sym).name()
                                && name.starts_with("dead_")
                            {
                                saw_dead_key = true;
                            }
                        }
                    }
                }
            }
            (oracle.state_update_mask)(state, 0, 0, 0, 0, 0, 0);
            for keycode in 8u32..=255 {
                if (oracle.state_key_get_utf32)(state, keycode) != 0 {
                    let with_level3 = {
                        (oracle.state_update_mask)(state, 0x80, 0, 0, 0, 0, 0);
                        let value = (oracle.state_key_get_utf32)(state, keycode);
                        (oracle.state_update_mask)(state, 0, 0, 0, 0, 0, 0);
                        value
                    };
                    if with_level3 != (oracle.state_key_get_utf32)(state, keycode) {
                        level3_changed_text = true;
                    }
                }
            }
            (oracle.state_unref)(state);
            (oracle.keymap_unref)(keymap);
            (oracle.context_unref)(context);
        }
    }
    assert!(saw_level3_shift, "no fixture contains ISO_Level3_Shift");
    assert!(saw_dead_key, "no fixture contains a dead key");
    assert!(
        level3_changed_text,
        "level3 modifier does not change any UTF-32 output"
    );
}
