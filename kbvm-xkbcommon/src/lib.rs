//! A drop-in subset of libxkbcommon implemented on top of kbvm.
//!
//! This crate implements exactly the C ABI used by Chromium's Wayland
//! keyboard handling. The keymap and state semantics follow libxkbcommon.
//!
//! All `unsafe` code lives in [`abi`].

pub mod abi;
mod context;
mod keymap;
mod parse;
mod state;
