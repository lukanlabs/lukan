use std::ffi::{c_char, c_float, c_int, c_void};

extern "C" {
    pub fn whisper_shim_init(model_path: *const c_char, use_gpu: c_int) -> *mut c_void;

    pub fn whisper_shim_free(ctx: *mut c_void);

    pub fn whisper_shim_transcribe(
        ctx: *mut c_void,
        samples: *const c_float,
        n_samples: c_int,
        language: *const c_char,
        n_threads: c_int,
    ) -> c_int;

    pub fn whisper_shim_n_segments(ctx: *mut c_void) -> c_int;

    pub fn whisper_shim_segment_text(ctx: *mut c_void, i: c_int) -> *const c_char;
}
