use core::ffi::c_int;

extern "C" {
    pub fn malloc_trim(__pad: usize) -> c_int;
}
