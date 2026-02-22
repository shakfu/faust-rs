//! UI and metadata dispatch helpers.
//!
//! Iterates `FbcUiInstruction` and `FbcMetaInstruction` lists and calls the
//! corresponding C callbacks in `UIGlue` / `MetaGlue`.

use std::ffi::CString;

use codegen::backends::interp::{FbcMetaInstruction, FbcUiInstruction, FbcOpcode};

use crate::types::{FaustFloat, MetaGlue, UIGlue};

/// Dispatch a slice of `FbcUiInstruction<f32>` to a `UIGlue` callback table.
///
/// Each instruction maps to the corresponding `UIGlue` function pointer.
/// The `zone` pointer for widgets points into `real_heap` at `instr.offset`.
///
/// # Safety
/// - `glue` must be non-null and point to a valid `UIGlue`.
/// - `real_heap` must have at least `instr.offset + 1` elements for widget instructions.
pub(crate) unsafe fn dispatch_ui(
    ui: &[FbcUiInstruction<FaustFloat>],
    real_heap: &mut [FaustFloat],
    glue: *mut UIGlue,
) {
    unsafe {
        let glue = &*glue;
        for instr in ui {
            let label = c_str(&instr.label);
            let zone: *mut FaustFloat = if instr.offset >= 0 {
                real_heap
                    .get_mut(instr.offset as usize)
                    .map_or(std::ptr::null_mut(), |r| r as *mut FaustFloat)
            } else {
                std::ptr::null_mut()
            };

            match instr.opcode {
                FbcOpcode::OpenTabBox => {
                    if let Some(f) = glue.open_tab_box {
                        f(glue.ui_interface, label.as_ptr());
                    }
                }
                FbcOpcode::OpenHorizontalBox => {
                    if let Some(f) = glue.open_horizontal_box {
                        f(glue.ui_interface, label.as_ptr());
                    }
                }
                FbcOpcode::OpenVerticalBox => {
                    if let Some(f) = glue.open_vertical_box {
                        f(glue.ui_interface, label.as_ptr());
                    }
                }
                FbcOpcode::CloseBox => {
                    if let Some(f) = glue.close_box {
                        f(glue.ui_interface);
                    }
                }
                FbcOpcode::AddButton => {
                    if let Some(f) = glue.add_button {
                        f(glue.ui_interface, label.as_ptr(), zone);
                    }
                }
                FbcOpcode::AddCheckButton => {
                    if let Some(f) = glue.add_check_button {
                        f(glue.ui_interface, label.as_ptr(), zone);
                    }
                }
                FbcOpcode::AddVerticalSlider => {
                    if let Some(f) = glue.add_vertical_slider {
                        f(
                            glue.ui_interface,
                            label.as_ptr(),
                            zone,
                            instr.init,
                            instr.min,
                            instr.max,
                            instr.step,
                        );
                    }
                }
                FbcOpcode::AddHorizontalSlider => {
                    if let Some(f) = glue.add_horizontal_slider {
                        f(
                            glue.ui_interface,
                            label.as_ptr(),
                            zone,
                            instr.init,
                            instr.min,
                            instr.max,
                            instr.step,
                        );
                    }
                }
                FbcOpcode::AddNumEntry => {
                    if let Some(f) = glue.add_num_entry {
                        f(
                            glue.ui_interface,
                            label.as_ptr(),
                            zone,
                            instr.init,
                            instr.min,
                            instr.max,
                            instr.step,
                        );
                    }
                }
                FbcOpcode::AddHorizontalBargraph => {
                    if let Some(f) = glue.add_horizontal_bargraph {
                        f(
                            glue.ui_interface,
                            label.as_ptr(),
                            zone,
                            instr.min,
                            instr.max,
                        );
                    }
                }
                FbcOpcode::AddVerticalBargraph => {
                    if let Some(f) = glue.add_vertical_bargraph {
                        f(
                            glue.ui_interface,
                            label.as_ptr(),
                            zone,
                            instr.min,
                            instr.max,
                        );
                    }
                }
                FbcOpcode::AddSoundfile => {
                    // Soundfile** zone — not supported in this port; pass null.
                    if let Some(f) = glue.add_soundfile {
                        let url = c_str(&instr.key);
                        f(
                            glue.ui_interface,
                            label.as_ptr(),
                            url.as_ptr(),
                            std::ptr::null_mut(),
                        );
                    }
                }
                FbcOpcode::Declare => {
                    if let Some(f) = glue.declare {
                        let key = c_str(&instr.key);
                        let val = c_str(&instr.value);
                        f(glue.ui_interface, zone, key.as_ptr(), val.as_ptr());
                    }
                }
                _ => {} // non-UI opcodes are silently ignored
            }
        }
    }
}

/// Dispatch `FbcMetaInstruction` list to a `MetaGlue` callback.
///
/// # Safety
/// `meta` must be non-null and point to a valid `MetaGlue`.
pub(crate) unsafe fn dispatch_meta(
    meta_block: &[FbcMetaInstruction],
    glue: *mut MetaGlue,
) {
    unsafe {
        let glue = &*glue;
        if let Some(declare) = glue.declare {
            for instr in meta_block {
                let key = c_str(&instr.key);
                let val = c_str(&instr.value);
                declare(glue.meta_interface, key.as_ptr(), val.as_ptr());
            }
        }
    }
}

/// Converts a Rust `&str` to a temporary `CString` for passing to C callbacks.
///
/// Embedded NUL bytes are replaced with `\0` literals.
fn c_str(s: &str) -> CString {
    let safe = s.replace('\0', "\\0");
    CString::new(safe).unwrap_or_else(|_| CString::new("").unwrap())
}
