use std::path::Path;

fn main() {
    let whisper_dir = Path::new("whisper.cpp");
    if !whisper_dir.exists() {
        panic!(
            "\n\nwhisper.cpp source not found!\n\
             Run: git clone https://github.com/ggml-org/whisper.cpp.git plugins/whisper/whisper.cpp\n\n"
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
        cfg.define("GGML_METAL", "ON");
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

    // Link whisper and ggml libraries (order matters for static linking)
    println!("cargo:rustc-link-lib=static=whisper");
    println!("cargo:rustc-link-lib=static=ggml");
    println!("cargo:rustc-link-lib=static=ggml-base");
    println!("cargo:rustc-link-lib=static=ggml-cpu");

    if has_cuda {
        println!("cargo:rustc-link-lib=static=ggml-cuda");
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cublas");
        println!("cargo:rustc-link-lib=cublasLt");
    }

    // System libraries
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=gomp"); // OpenMP runtime (used by ggml-cpu)
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=m");
    } else if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=c++");
        println!("cargo:rustc-link-framework=Accelerate");
        println!("cargo:rustc-link-framework=Metal");
        println!("cargo:rustc-link-framework=MetalKit");
        println!("cargo:rustc-link-framework=Foundation");
    }

    // Build our C shim against whisper headers
    cc::Build::new()
        .file("shim.c")
        .include(format!("{}/include", dst.display()))
        .compile("whisper_shim");
}
