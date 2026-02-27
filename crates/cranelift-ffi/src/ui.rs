//! UI and metadata callback dispatch helpers for `cranelift-ffi`.
//!
//! The Cranelift FFI layer reuses interpreter-style UI/meta instruction lists
//! (stored as a sidecar in factories) to preserve callback semantics while the
//! native backend runtime path focuses on executable DSP compute.

use std::ffi::CString;

use codegen::backends::interp::{FbcMetaInstruction, FbcOpcode, FbcUiInstruction};

use crate::types::{FaustFloat, MetaGlue, UIGlue};

/// Dispatches UI instructions to a `UIGlue` callback table.
///
/// # Safety
/// - `glue` must be non-null and point to a valid `UIGlue` table.
/// - `real_heap` must provide valid zones for instruction offsets.
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
                _ => {}
            }
        }
    }
}

/// Dispatches metadata instructions to `MetaGlue::declare`.
///
/// # Safety
/// `glue` must be non-null and valid.
pub(crate) unsafe fn dispatch_meta(meta_block: &[FbcMetaInstruction], glue: *mut MetaGlue) {
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

fn c_str(s: &str) -> CString {
    let safe = s.replace('\0', "\\0");
    CString::new(safe).unwrap_or_else(|_| CString::new("").expect("empty CString"))
}

/// UI/meta scaffold status string.
#[must_use]
pub fn ui_status() -> &'static str {
    "cranelift-ffi ui runtime helpers"
}

#[cfg(test)]
mod tests {
    use super::ui_status;

    #[test]
    fn ui_status_is_stable() {
        assert_eq!(ui_status(), "cranelift-ffi ui runtime helpers");
    }
}
