//! Accepts the arguments `gpui_windows`'s build script passes to `fxc.exe`:
//! `/T <target> /E <entry> /Fh <header out> /Vn <variable> /O3 <source>`,
//! compiles with `D3DCompileFromFile` (standard `#include` handling) and writes the
//! byte array header in `fxc`'s `/Fh` layout (`const BYTE name[] = { ... };`).
//! Built for Windows only; run from WSL via `scripts/fxc-wsl.sh` / `scripts/regen-windows-shaders.sh`.

#![allow(clippy::disallowed_methods, reason = "standalone build helper binary")]

#[cfg(windows)]
fn main() {
    if let Err(e) = run() {
        eprintln!("fxc-lite: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("fxc-lite only runs on Windows");
    std::process::exit(1);
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    use std::ffi::CString;
    use windows::Win32::Graphics::Direct3D::Fxc::{
        D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompileFromFile,
    };
    use windows::Win32::Graphics::Direct3D::{ID3DBlob, ID3DInclude};
    use windows::core::{HSTRING, PCSTR};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut target, mut entry, mut header, mut var, mut source) = (None, None, None, None, None);
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = || {
            i += 1;
            args.get(i).cloned().ok_or(format!("{a} needs a value"))
        };
        match a {
            "/T" => target = Some(value()?),
            "/E" => entry = Some(value()?),
            "/Fh" => header = Some(value()?),
            "/Vn" => var = Some(value()?),
            "/O3" => {}
            s if s.starts_with('/') => return Err(format!("unsupported option {s}")),
            s => source = Some(s.to_string()),
        }
        i += 1;
    }
    let (target, entry, header, var, source) = (
        target.ok_or("missing /T")?,
        entry.ok_or("missing /E")?,
        header.ok_or("missing /Fh")?,
        var.ok_or("missing /Vn")?,
        source.ok_or("missing source file")?,
    );

    let entry_c = CString::new(entry).map_err(|e| e.to_string())?;
    let target_c = CString::new(target).map_err(|e| e.to_string())?;
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // D3D_COMPILE_STANDARD_FILE_INCLUDE is the sentinel pointer value 1.
    let include = unsafe { std::mem::transmute::<usize, ID3DInclude>(1) };
    let result = unsafe {
        D3DCompileFromFile(
            &HSTRING::from(source.as_str()),
            None,
            &include,
            PCSTR::from_raw(entry_c.as_ptr().cast()),
            PCSTR::from_raw(target_c.as_ptr().cast()),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    let blob_bytes = |b: &ID3DBlob| unsafe {
        std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec()
    };
    if let Err(e) = result {
        let detail = errors
            .as_ref()
            .map(|b| String::from_utf8_lossy(&blob_bytes(b)).into_owned())
            .unwrap_or_default();
        return Err(format!("{e}\n{detail}"));
    }
    let bytes = blob_bytes(code.as_ref().ok_or("compiler returned no code")?);
    let body: Vec<String> = bytes
        .chunks(16)
        .map(|c| {
            c.iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();
    let text = format!(
        "const BYTE {var}[] =\n{{\n    {}\n}};\n",
        body.join(",\n    ")
    );
    std::fs::write(&header, text).map_err(|e| format!("writing {header}: {e}"))
}
