// Compile optimized bytecode once at build time, also when targeting Windows ARM64.
#[cfg(windows)]
fn main() {
    use windows::{Win32::Graphics::Direct3D::Fxc::*, core::PCSTR};
    println!("cargo:rerun-if-changed=src/gpu_pipeline.hlsl");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let source = std::fs::read("src/gpu_pipeline.hlsl").expect("canvas shader source");
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    for (entry, profile) in [
        ("mesh_vertex", "vs_4_1"),
        ("quad_vertex", "vs_4_1"),
        ("mesh_pixel", "ps_4_1"),
        ("effect_pixel", "ps_4_1"),
    ] {
        let entry_name = std::ffi::CString::new(entry).unwrap();
        let profile_name = std::ffi::CString::new(profile).unwrap();
        let mut code = None;
        let mut errors = None;
        let result = unsafe {
            D3DCompile(
                source.as_ptr().cast(),
                source.len(),
                PCSTR::null(),
                None,
                None,
                PCSTR(entry_name.as_ptr().cast()),
                PCSTR(profile_name.as_ptr().cast()),
                D3DCOMPILE_OPTIMIZATION_LEVEL3 | D3DCOMPILE_ENABLE_STRICTNESS,
                0,
                &mut code,
                Some(&mut errors),
            )
        };
        if let Err(error) = result {
            let diagnostic = errors
                .map(|blob| unsafe {
                    String::from_utf8_lossy(std::slice::from_raw_parts(
                        blob.GetBufferPointer().cast(),
                        blob.GetBufferSize(),
                    ))
                    .into_owned()
                })
                .unwrap_or_default();
            panic!("Canvas shader {entry}: {error}\n{diagnostic}");
        }
        let code = code.expect("shader compiler bytecode");
        let bytes = unsafe {
            std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize())
        };
        std::fs::write(output.join(format!("{entry}.cso")), bytes).expect("write optimized shader");
    }
}

#[cfg(not(windows))]
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        panic!("The Windows GPU backend requires a Windows build host for D3D shader compilation");
    }
}
