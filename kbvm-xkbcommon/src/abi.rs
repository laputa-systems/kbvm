//! The C ABI.
//!
//! This is the only module that uses `unsafe`. Every unsafe block states the
//! invariant it relies on.

#![allow(non_camel_case_types)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::context::ContextHandle;
use crate::keymap::KeymapHandle;
use crate::names::RuleNames;
use crate::state::StateHandle;
use kbvm::Keysym;
use kbvm::state_machine::Direction;
use libc::c_char;
use libc::c_int;
use libc::c_uint;
use std::ffi::CStr;
use std::path::Path;
use std::ptr;
use std::sync::Arc;

#[repr(C)]
pub struct xkb_context {
    _private: [u8; 0],
}

#[repr(C)]
pub struct xkb_keymap {
    _private: [u8; 0],
}

#[repr(C)]
pub struct xkb_state {
    _private: [u8; 0],
}

#[repr(C)]
pub struct xkb_rule_names {
    pub rules: *const c_char,
    pub model: *const c_char,
    pub layout: *const c_char,
    pub variant: *const c_char,
    pub options: *const c_char,
}

const XKB_KEYMAP_FORMAT_TEXT_V1: c_int = 1;
const XKB_KEY_DOWN: c_int = 1;
const XKB_MOD_INVALID: u32 = u32::MAX;
const XKB_LAYOUT_INVALID: u32 = u32::MAX;
const XKB_LEVEL_INVALID: u32 = u32::MAX;

/// # Safety
///
/// `ptr` must be null or point to a NUL-terminated string that stays valid
/// for `'a`.
unsafe fn optional_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: The caller guarantees a NUL-terminated string valid for `'a`.
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}

/// # Safety
///
/// `ptr` must be null or point to a NUL-terminated string that stays valid
/// for `'a`.
unsafe fn optional_bytes<'a>(ptr: *const c_char) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: The caller guarantees a NUL-terminated string valid for `'a`.
    Some(unsafe { CStr::from_ptr(ptr) }.to_bytes())
}

/// # Safety
///
/// `ptr` must be null or point to a live `T`.
unsafe fn borrow<'a, T>(ptr: *mut T) -> Option<&'a T> {
    // SAFETY: The caller guarantees that the pointer is null or points to a
    // live object that outlives `'a`.
    unsafe { ptr.as_ref() }
}

/// # Safety
///
/// `ptr` must be null or point to a live `T` inside a mutable borrow.
unsafe fn borrow_mut<'a, T>(ptr: *mut T) -> Option<&'a mut T> {
    // SAFETY: The caller guarantees that the pointer is null or points to a
    // live object that outlives `'a` and that no other reference to it exists.
    unsafe { ptr.as_mut() }
}

/// # Safety
///
/// `ptr` must point to a live `T` that is managed by an `Arc` and must remain
/// alive for the duration of the call.
unsafe fn clone_arc<T>(ptr: *mut T) -> Arc<T> {
    // SAFETY: The caller guarantees that the pointer was produced by
    // `Arc::into_raw` and that its strong count may be incremented.
    unsafe {
        Arc::increment_strong_count(ptr as *const T);
        Arc::from_raw(ptr as *const T)
    }
}

/// # Safety
///
/// `ptr` must be null or have been produced by `Arc::into_raw` for a live `T`,
/// with a strong count that includes the reference being released.
unsafe fn unref_arc<T>(ptr: *mut T) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: The caller guarantees that the pointer is a live `Arc::into_raw`
    // pointer and that this call releases exactly one strong reference.
    unsafe {
        drop(Arc::from_raw(ptr as *const T));
    }
}

fn context_ref(context: *mut xkb_context) -> Option<&'static ContextHandle> {
    // SAFETY: A context pointer handed to the C API is either null or points
    // to a live context. The returned reference is only used by the caller,
    // which cannot outlive the context by contract.
    unsafe { borrow(context as *mut ContextHandle) }
}

fn keymap_ref(keymap: *mut xkb_keymap) -> Option<&'static KeymapHandle> {
    // SAFETY: A keymap pointer handed to the C API is either null or points to
    // a live keymap. The returned reference is only used by the caller, which
    // cannot outlive the keymap by contract.
    unsafe { borrow(keymap as *mut KeymapHandle) }
}

fn state_ref(state: *mut xkb_state) -> Option<&'static StateHandle> {
    // SAFETY: A state pointer handed to the C API is either null or points to
    // a live state. The returned reference is only used by the caller, which
    // cannot outlive the state by contract.
    unsafe { borrow(state as *mut StateHandle) }
}

fn state_mut_ref(state: *mut xkb_state) -> Option<&'static mut StateHandle> {
    // SAFETY: A state pointer handed to the C API is either null or points to
    // a live state. The C caller must not alias the state concurrently. The
    // returned reference is only used by the caller.
    unsafe { borrow_mut(state as *mut StateHandle) }
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_context_new(flags: c_uint) -> *mut xkb_context {
    let handle = Arc::new(ContextHandle::new(flags));
    Arc::into_raw(handle) as *mut ContextHandle as *mut xkb_context
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_context_unref(context: *mut xkb_context) {
    // SAFETY: The pointer is null or a live context created by
    // `xkb_context_new` and this call releases its reference.
    unsafe { unref_arc(context as *mut ContextHandle) }
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_context_include_path_append(
    context: *mut xkb_context,
    path: *const c_char,
) -> c_int {
    if context.is_null() || path.is_null() {
        return 0;
    }
    let Some(context) = context_ref(context) else {
        return 0;
    };
    // SAFETY: The C contract requires `path` to be a NUL-terminated string
    // that remains valid for the duration of this call.
    let path = unsafe { CStr::from_ptr(path) };
    let path = path.to_string_lossy();
    i32::from(context.append_include_path(Path::new(path.as_ref())))
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_new_from_buffer(
    context: *mut xkb_context,
    buffer: *const c_char,
    length: usize,
    format: c_int,
    flags: c_int,
) -> *mut xkb_keymap {
    if context.is_null() || buffer.is_null() || format != XKB_KEYMAP_FORMAT_TEXT_V1 || flags != 0 {
        return ptr::null_mut();
    }
    if context_ref(context).is_none() {
        return ptr::null_mut();
    }
    // SAFETY: The C contract requires `buffer` to point to `length` readable
    // bytes.
    let mut text = unsafe { std::slice::from_raw_parts(buffer as *const u8, length) };
    if let Some((0, rest)) = text.split_last() {
        text = rest;
    }
    // SAFETY: The context pointer is non-null and live. The new keymap keeps
    // the context alive through its own strong reference.
    let context = unsafe { clone_arc(context as *mut ContextHandle) };
    match KeymapHandle::new(context, text) {
        Some(keymap) => Arc::into_raw(Arc::new(keymap)) as *mut KeymapHandle as *mut xkb_keymap,
        None => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_new_from_string(
    context: *mut xkb_context,
    string: *const c_char,
    format: c_int,
    flags: c_int,
) -> *mut xkb_keymap {
    if string.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: The C contract requires `string` to be NUL-terminated.
    let length = unsafe { CStr::from_ptr(string) }.to_bytes().len();
    xkb_keymap_new_from_buffer(context, string, length, format, flags)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_new_from_names(
    context: *mut xkb_context,
    names: *const xkb_rule_names,
    flags: c_int,
) -> *mut xkb_keymap {
    if context.is_null() || flags != 0 || context_ref(context).is_none() {
        return ptr::null_mut();
    }
    // SAFETY: `names` is null or points to an `xkb_rule_names` whose strings
    // are null or NUL-terminated for the duration of this call.
    let names = match unsafe { names.as_ref() } {
        None => RuleNames::default(),
        Some(names) => unsafe {
            RuleNames {
                rules: optional_str(names.rules),
                model: optional_str(names.model),
                layout: optional_str(names.layout),
                variant: optional_str(names.variant),
                options: optional_str(names.options),
            }
        },
    };
    // SAFETY: The context pointer is non-null and live. The new keymap keeps
    // the context alive through its own strong reference.
    let context = unsafe { clone_arc(context as *mut ContextHandle) };
    let keymap = crate::names::keymap_from_names(context, names);
    Arc::into_raw(Arc::new(keymap)) as *mut KeymapHandle as *mut xkb_keymap
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_unref(keymap: *mut xkb_keymap) {
    // SAFETY: The pointer is null or a live keymap created by
    // `xkb_keymap_new_from_buffer` and this call releases its reference.
    unsafe { unref_arc(keymap as *mut KeymapHandle) }
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_min_keycode(keymap: *mut xkb_keymap) -> u32 {
    keymap_ref(keymap).map(|k| k.min_keycode).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_max_keycode(keymap: *mut xkb_keymap) -> u32 {
    keymap_ref(keymap).map(|k| k.max_keycode).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_num_mods(keymap: *mut xkb_keymap) -> u32 {
    keymap_ref(keymap).map(|k| k.num_mods()).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_mod_get_index(keymap: *mut xkb_keymap, name: *const c_char) -> u32 {
    // SAFETY: The C contract requires `name` to be NUL-terminated.
    let Some(name) = (unsafe { optional_bytes(name) }) else {
        return XKB_MOD_INVALID;
    };
    keymap_ref(keymap)
        .and_then(|k| k.mod_index(name))
        .unwrap_or(XKB_MOD_INVALID)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_num_layouts(keymap: *mut xkb_keymap) -> u32 {
    keymap_ref(keymap).map(|k| k.num_layouts()).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_key_repeats(keymap: *mut xkb_keymap, keycode: u32) -> c_int {
    keymap_ref(keymap)
        .map(|k| c_int::from(k.key_repeats(keycode)))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keysym_to_utf32(keysym: u32) -> u32 {
    Keysym(keysym).char().map_or(0, |c| c as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_mod_get_name(keymap: *mut xkb_keymap, idx: u32) -> *const c_char {
    keymap_ref(keymap)
        .and_then(|k| k.mod_name(idx))
        .map(|name| name.as_ptr())
        .unwrap_or(ptr::null())
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_num_layouts_for_key(keymap: *mut xkb_keymap, keycode: u32) -> u32 {
    keymap_ref(keymap)
        .map(|k| k.num_layouts_for_key(keycode))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_num_levels_for_key(
    keymap: *mut xkb_keymap,
    keycode: u32,
    layout: u32,
) -> u32 {
    keymap_ref(keymap)
        .map(|k| k.num_levels_for_key(keycode, layout))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_keymap_key_get_syms_by_level(
    keymap: *mut xkb_keymap,
    keycode: u32,
    layout: u32,
    level: u32,
    syms_out: *mut *const u32,
) -> c_int {
    if syms_out.is_null() {
        return 0;
    }
    // SAFETY: `syms_out` is non-null and writable per the C contract.
    unsafe {
        *syms_out = ptr::null();
    }
    let Some(keymap) = keymap_ref(keymap) else {
        return 0;
    };
    match keymap.key_syms_by_level(keycode, layout, level) {
        Some(syms) => {
            // SAFETY: `syms_out` is non-null and writable per the C contract.
            // The pointer remains valid as long as the keymap is alive.
            unsafe {
                *syms_out = syms.as_ptr();
            }
            syms.len() as c_int
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_new(keymap: *mut xkb_keymap) -> *mut xkb_state {
    if keymap.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: The keymap pointer is non-null and live. The new state keeps the
    // keymap alive through its own strong reference.
    let keymap = unsafe { clone_arc(keymap as *mut KeymapHandle) };
    Arc::into_raw(Arc::new(StateHandle::new(keymap))) as *mut StateHandle as *mut xkb_state
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_unref(state: *mut xkb_state) {
    // SAFETY: The pointer is null or a live state created by
    // `xkb_state_new` and this call releases its reference.
    unsafe { unref_arc(state as *mut StateHandle) }
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_get_keymap(state: *mut xkb_state) -> *mut xkb_keymap {
    let Some(state) = state_ref(state) else {
        return ptr::null_mut();
    };
    Arc::as_ptr(&state.keymap) as *mut KeymapHandle as *mut xkb_keymap
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_update_mask(
    state: *mut xkb_state,
    base_mods: u32,
    latched_mods: u32,
    locked_mods: u32,
    base_group: u32,
    latched_group: u32,
    locked_group: u32,
) -> c_int {
    let Some(state) = state_mut_ref(state) else {
        return 0;
    };
    state.update_mask(
        base_mods,
        latched_mods,
        locked_mods,
        base_group,
        latched_group,
        locked_group,
    ) as c_int
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_key_get_one_sym(state: *mut xkb_state, keycode: u32) -> u32 {
    state_ref(state).map(|s| s.key_get_one_sym(keycode)).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_key_get_syms(
    state: *mut xkb_state,
    keycode: u32,
    syms_out: *mut *const u32,
) -> c_int {
    if syms_out.is_null() {
        return 0;
    }
    // SAFETY: `syms_out` is non-null and writable per the C contract.
    unsafe {
        *syms_out = ptr::null();
    }
    let Some(state) = state_ref(state) else {
        return 0;
    };
    match state.key_get_syms(keycode) {
        Some(syms) => {
            // SAFETY: `syms_out` is non-null and writable per the C contract.
            // The pointer remains valid as long as the state (and with it the
            // keymap) is alive.
            unsafe {
                *syms_out = syms.as_ptr();
            }
            syms.len() as c_int
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_key_get_utf32(state: *mut xkb_state, keycode: u32) -> u32 {
    state_ref(state).map(|s| s.key_get_utf32(keycode)).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_mod_index_is_active(
    state: *mut xkb_state,
    idx: u32,
    affect: c_int,
) -> c_int {
    let Some(state) = state_ref(state) else {
        return -1;
    };
    state.mod_index_is_active(idx, affect as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_update_key(state: *mut xkb_state, keycode: u32, direction: c_int) -> c_int {
    let Some(state) = state_mut_ref(state) else {
        return 0;
    };
    let direction = if direction == XKB_KEY_DOWN {
        Direction::Down
    } else {
        Direction::Up
    };
    state.update_key(keycode, direction) as c_int
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_serialize_mods(state: *mut xkb_state, components: c_int) -> u32 {
    state_ref(state)
        .map(|s| s.serialize_mods(components as u32))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_layout_index_is_active(
    state: *mut xkb_state,
    idx: u32,
    components: c_int,
) -> c_int {
    state_ref(state)
        .map(|s| s.layout_index_is_active(idx, components as u32))
        .unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_led_name_is_active(state: *mut xkb_state, name: *const c_char) -> c_int {
    // SAFETY: The C contract requires `name` to be NUL-terminated.
    let Some(name) = (unsafe { optional_bytes(name) }) else {
        return -1;
    };
    state_ref(state)
        .map(|s| s.led_name_is_active(name))
        .unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_key_get_layout(state: *mut xkb_state, keycode: u32) -> u32 {
    state_ref(state)
        .and_then(|s| s.key_get_layout(keycode))
        .unwrap_or(XKB_LAYOUT_INVALID)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_key_get_level(state: *mut xkb_state, keycode: u32, layout: u32) -> u32 {
    state_ref(state)
        .and_then(|s| s.key_get_level(keycode, layout))
        .unwrap_or(XKB_LEVEL_INVALID)
}

#[unsafe(no_mangle)]
pub extern "C" fn xkb_state_mod_mask_remove_consumed(
    state: *mut xkb_state,
    keycode: u32,
    mask: u32,
) -> u32 {
    state_ref(state)
        .map(|s| s.mod_mask_remove_consumed(keycode, mask))
        .unwrap_or(0)
}
