#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
use std::{ffi::CStr, path::Path};

include!("./bindings.rs");

pub struct HandheldSet {
    raw_handhelds: *mut *mut Handheld,
    pub num_handhelds: u32,
}

impl HandheldSet {
    pub fn new() -> HandheldSet {
        let mut handheld_set: HandheldSet;

        unsafe {
            handheld_set = HandheldSet {
                raw_handhelds: std::ptr::null_mut() as *mut *mut Handheld,
                num_handhelds: 0,
            };

            let result = miuchiz_handheld_create_all(&mut handheld_set.raw_handhelds);
            if result >= 0 {
                handheld_set.num_handhelds = result as u32;
            }
        }

        handheld_set
    }

    pub fn get_handheld_paths(&self) -> Vec<&Path> {
        let mut result: Vec<&Path> = Vec::new();
        result.reserve(self.num_handhelds as usize);

        for i in 0..self.num_handhelds {
            let slice: &CStr;

            unsafe {
                let handheld = *self.raw_handhelds.offset(i as isize);
                let path_i8 = (*handheld).device;
                slice = CStr::from_ptr(path_i8);
            }

            if let Ok(str) = std::str::from_utf8(slice.to_bytes()) {
                let path = Path::new(str);
                result.push(path);
            }
        }
        result
    }

    pub fn destroy_all(&mut self) {
        if !self.raw_handhelds.is_null() {
            unsafe {
                miuchiz_handheld_destroy_all(self.raw_handhelds);
            }
            self.raw_handhelds = std::ptr::null_mut::<*mut Handheld>();
        }
    }

    fn get_handheld_by_path(&self, path: &Path) -> Option<*mut Handheld> {
        let mut handheld: Option<*mut Handheld> = None;
        for i in 0..self.num_handhelds {
            let slice: &CStr;
            let raw_handheld: *mut Handheld;

            unsafe {
                raw_handheld = *self.raw_handhelds.offset(i as isize);
                let path_i8 = (*raw_handheld).device;
                slice = CStr::from_ptr(path_i8);
            }

            if let Ok(str) = std::str::from_utf8(slice.to_bytes()) {
                let this_path = Path::new(str);

                if path == this_path {
                    handheld = Some(raw_handheld);
                    break;
                }
            }
        }

        handheld
    }

    pub fn write_page(&self, path: &Path, page: u32, buf: &[u8]) -> Result<(), String> {
        if let Some(raw_handheld) = self.get_handheld_by_path(path) {
            unsafe {
                let result = miuchiz_handheld_write_page(
                    raw_handheld,
                    page as i32,
                    buf.as_ptr() as *const ::std::os::raw::c_void,
                    buf.len() as size_t,
                );
                if result < 0 {
                    return Err("Error writing page".to_string());
                }
            }
            Ok(())
        } else {
            Err("Could not find handheld".to_string())
        }
    }

    pub fn read_page(&self, path: &Path, page: u32) -> Result<Vec<u8>, String> {
        let result: Vec<u8> = vec![0u8; 0x1000];
        if let Some(raw_handheld) = self.get_handheld_by_path(path) {
            unsafe {
                let result = miuchiz_handheld_read_page(
                    raw_handheld,
                    page as i32,
                    result.as_ptr() as *mut ::std::os::raw::c_void,
                    result.len() as size_t,
                );
                if result < 0 {
                    return Err("Error reading page".to_string());
                }
            }
            Ok(result)
        } else {
            Err("Could not find handheld".to_string())
        }
    }

    pub fn eject(&self, path: &Path) {
        self.read_page(path, 0x200).ok();
    }
}

impl Drop for HandheldSet {
    fn drop(&mut self) {
        self.destroy_all();
    }
}
