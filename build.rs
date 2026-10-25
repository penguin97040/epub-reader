//! Embed the app icon into the Windows executable. Other platforms set the
//! window icon at runtime from `assets/icon-256.png`.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico");

    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "gnu" {
        let from_env = std::env::var("WINDRES").ok();
        let path = from_env.as_deref().or_else(|| {
            ["x86_64-w64-mingw32-windres", "windres"]
                .iter()
                .find(|c| which(c))
                .copied()
        });
        match path {
            Some(tool) => {
                res.set_windres_path(tool);
            }
            None => {
                println!("cargo:warning=no windres on PATH; building without an embedded icon");
                return;
            }
        }
    }

    if let Err(err) = res.compile() {
        println!("cargo:warning=could not embed the icon: {err}");
    }
}

fn which(tool: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .any(|dir| dir.join(tool).is_file() || dir.join(format!("{tool}.exe")).is_file())
        })
        .unwrap_or(false)
}
