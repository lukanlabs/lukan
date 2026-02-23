/*
 * Simplified C shim over whisper.cpp API.
 *
 * Avoids exposing the large whisper_full_params struct to Rust FFI.
 * All complex struct creation happens here in C.
 */

#include "whisper.h"
#include <stdlib.h>

struct whisper_context *whisper_shim_init(const char *model_path, int use_gpu) {
    struct whisper_context_params params = whisper_context_default_params();
    params.use_gpu = (bool)use_gpu;
    return whisper_init_from_file_with_params(model_path, params);
}

void whisper_shim_free(struct whisper_context *ctx) {
    if (ctx) {
        whisper_free(ctx);
    }
}

int whisper_shim_transcribe(
    struct whisper_context *ctx,
    const float *samples,
    int n_samples,
    const char *language,
    int n_threads
) {
    struct whisper_full_params params = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);

    params.print_special    = false;
    params.print_progress   = false;
    params.print_realtime   = false;
    params.print_timestamps = false;
    params.no_timestamps    = true;
    params.single_segment   = false;
    params.language         = language;  /* NULL or "auto" for auto-detect */
    params.n_threads        = n_threads;

    return whisper_full(ctx, params, samples, n_samples);
}

int whisper_shim_n_segments(struct whisper_context *ctx) {
    return whisper_full_n_segments(ctx);
}

const char *whisper_shim_segment_text(struct whisper_context *ctx, int i) {
    return whisper_full_get_segment_text(ctx, i);
}
