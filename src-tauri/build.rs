fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=TARGET_TRIPLE={target}");
    // oboe (audio) is C++ and is built without a C++ runtime; Android's libc
    // does not provide one, so link libc++ statically or the .so fails to
    // load (`cannot locate symbol "__cxa_pure_virtual"`).
    if target.contains("android") {
        println!("cargo:rustc-link-arg-cdylib=-lc++_static");
        println!("cargo:rustc-link-arg-cdylib=-lc++abi");
    }
    tauri_build::build();
}

