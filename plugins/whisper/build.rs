use std::path::Path;

fn main() {
    let whisper_dir = Path::new("whisper.cpp");
    if !whisper_dir.exists() {
        panic!(
            "\n\nwhisper.cpp source not found!\n\
             Run: git submodule update --init\n\n"
        );
    }

    // Build whisper.cpp with cmake
    let mut cfg = cmake::Config::new(whisper_dir);
    cfg.define("BUILD_SHARED_LIBS", "OFF")
        .define("WHISPER_BUILD_EXAMPLES", "OFF")
        .define("WHISPER_BUILD_TESTS", "OFF")
        .define("WHISPER_BUILD_SERVER", "OFF")
        .define("CMAKE_BUILD_TYPE", "Release")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");

    // Auto-detect GPU support
    if cfg!(target_os = "macos") {
        cfg.define("GGML_METAL", "ON")
            .define("GGML_BLAS", "ON");
    }

    // Check for CUDA
    let has_cuda = std::env::var("CUDA_PATH").is_ok()
        || Path::new("/usr/local/cuda").exists()
        || Path::new("/usr/lib/cuda").exists();
    if has_cuda {
        cfg.define("GGML_CUDA", "ON");
        println!("cargo:warning=Building with CUDA support");
    }

    let dst = cfg.build();

    // Link paths — cmake installs to lib/ or lib64/
    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    println!("cargo:rustc-link-search=native={}/lib64", dst.display());

    // Build our C shim against whisper headers (must be before whisper libs
    // so the linker can resolve shim -> whisper dependencies)
    cc::Build::new()
        .file("shim.c")
        .include(format!("{}/include", dst.display()))
        .compile("whisper_shim");

    // Link whisper and ggml libraries (order matters for static linking:
    // dependents first, dependencies last)
    println!("cargo:rustc-link-lib=static=whisper");
    println!("cargo:rustc-link-lib=static=ggml");
    println!("cargo:rustc-link-lib=static=ggml-base");
    println!("cargo:rustc-link-lib=static=ggml-cpu");

    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=static=ggml-metal");
        println!("cargo:rustc-link-lib=static=ggml-blas");
    }

    if has_cuda {
        println!("cargo:rustc-link-lib=static=ggml-cuda");
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cublas");
        println!("cargo:rustc-link-lib=cublasLt");
    }

    // System libraries — use rustc-link-arg to force correct link order
    // (frameworks must come after static libs that reference them)
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=gomp");
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=m");
    } else if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=c++");
        println!("cargo:rustc-link-arg=-framework");
        println!("cargo:rustc-link-arg=Accelerate");
        println!("cargo:rustc-link-arg=-framework");
        println!("cargo:rustc-link-arg=Metal");
        println!("cargo:rustc-link-arg=-framework");
        println!("cargo:rustc-link-arg=MetalKit");
        println!("cargo:rustc-link-arg=-framework");
        println!("cargo:rustc-link-arg=Foundation");
        println!("cargo:rustc-link-arg=-framework");
        println!("cargo:rustc-link-arg=CoreFoundation");
    }
}
