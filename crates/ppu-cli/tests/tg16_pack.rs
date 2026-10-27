//! `ppu tg16 pack` / `unpack` and the `.tg16.json` format.

use ppu_cli::tg16::{load_toy, Toy};
use std::path::Path;

const SUNSET: &str = include_str!("../../tg16-core/toys/sunset.lua");

fn run(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_ppu"))
        .args(args)
        .output()
        .unwrap()
}

fn success(args: &[&str]) -> String {
    let output = run(args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn png_bytes(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
    out
}

/// A toy directory with a helper file, `main.lua` and two PNG assets.
fn make_toy(root: &Path) -> std::path::PathBuf {
    let dir = root.join("demo");
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("main.lua"), SUNSET).unwrap();
    std::fs::write(dir.join("helpers.lua"), "HELPER = 1\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "not part of the toy").unwrap();
    std::fs::write(
        dir.join("assets/hills.png"),
        png_bytes(2, 1, &[255, 0, 0, 255, 0, 255, 0, 128]),
    )
    .unwrap();
    std::fs::write(dir.join("assets/raw.png"), [0u8, 1, 2, 255, 254]).unwrap();
    std::fs::write(dir.join("assets/readme.md"), "ignored").unwrap();
    dir
}

#[test]
fn round_trip_dir_pack_unpack() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_toy(tmp.path());
    let pack = tmp.path().join("demo.tg16.json");
    success(&[
        "tg16",
        "pack",
        dir.to_str().unwrap(),
        "-o",
        pack.to_str().unwrap(),
    ]);

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&pack).unwrap()).unwrap();
    assert_eq!(json["format"], "ppu.toys/tg16");
    assert_eq!(json["version"], 1);
    assert_eq!(json["name"], "demo");
    assert_eq!(json["files"]["main.lua"], SUNSET);
    assert!(json["files"].get("notes.txt").is_none());
    assert_eq!(json["assets"]["raw.png"], "AAEC//4=");
    assert!(json["assets"].get("readme.md").is_none());

    let out = tmp.path().join("out");
    success(&[
        "tg16",
        "unpack",
        pack.to_str().unwrap(),
        out.to_str().unwrap(),
    ]);
    for name in [
        "main.lua",
        "helpers.lua",
        "assets/hills.png",
        "assets/raw.png",
    ] {
        assert_eq!(
            std::fs::read(dir.join(name)).unwrap(),
            std::fs::read(out.join(name)).unwrap(),
            "{name}"
        );
    }
    assert!(!out.join("notes.txt").exists());
    let a = Toy::from_dir(&dir).unwrap();
    let b = Toy::from_dir(&out).unwrap();
    assert_eq!((a.files, a.assets), (b.files, b.assets));
}

#[test]
fn pack_renders_like_the_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_toy(tmp.path());
    let pack = tmp.path().join("demo.tg16.json");
    std::fs::write(&pack, ppu_cli::tg16::pack(&dir).unwrap()).unwrap();
    let (from_dir, from_pack) = (tmp.path().join("a"), tmp.path().join("b"));
    for (input, out) in [(&dir, &from_dir), (&pack, &from_pack)] {
        success(&[
            "tg16",
            "render",
            input.to_str().unwrap(),
            "--at",
            "0,1.5",
            "-o",
            out.to_str().unwrap(),
        ]);
    }
    for f in ["frame-00000000.png", "frame-00000090.png"] {
        assert_eq!(
            std::fs::read(from_dir.join(f)).unwrap(),
            std::fs::read(from_pack.join(f)).unwrap(),
            "{f}"
        );
    }
    success(&["tg16", "check", pack.to_str().unwrap(), "--duration", "1"]);
}

#[test]
fn load_order_is_name_order_with_main_last() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("order");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["zeta.lua", "main.lua", "alpha.lua", "middle.lua"] {
        std::fs::write(dir.join(name), format!("-- {name}\n")).unwrap();
    }
    let names = |t: &Toy| t.files.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    let want = ["alpha.lua", "middle.lua", "zeta.lua", "main.lua"];
    let toy = load_toy(&dir).unwrap();
    assert_eq!(names(&toy), want);
    let packed = Toy::from_pack_json(&toy.to_pack_json().unwrap()).unwrap();
    assert_eq!(names(&packed), want);

    // Behaviour: helpers run before main.lua, so main can use their globals.
    std::fs::write(dir.join("alpha.lua"), "BG = rgb(0, 0, 255)\n").unwrap();
    std::fs::write(
        dir.join("main.lua"),
        "function init() color(0, BG) end\nfunction frame(t, f) end\n",
    )
    .unwrap();
    let pack = tmp.path().join("order.tg16.json");
    std::fs::write(&pack, ppu_cli::tg16::pack(&dir).unwrap()).unwrap();
    success(&["tg16", "check", pack.to_str().unwrap(), "--duration", "0.1"]);
}

fn pack_with(files: &str, assets: &str) -> String {
    format!(
        r#"{{"format":"ppu.toys/tg16","version":1,"name":"x","files":{files},"assets":{assets}}}"#
    )
}

#[test]
fn rejects_unsafe_names() {
    for bad in [
        "../evil.lua",
        "/abs.lua",
        "dir/main.lua",
        "..\\\\evil.lua",
        ".lua",
        "main.txt",
        "",
    ] {
        let text = pack_with(&format!(r#"{{"main.lua":"","{bad}":""}}"#), "{}");
        let err = Toy::from_pack_json(&text).unwrap_err();
        assert!(format!("{err:#}").contains("file name"), "{bad}: {err:#}");
    }
    for bad in ["../x.png", "sub/x.png", "x.lua", "..", "x.png/"] {
        let text = pack_with(r#"{"main.lua":""}"#, &format!(r#"{{"{bad}":""}}"#));
        let err = Toy::from_pack_json(&text).unwrap_err();
        assert!(format!("{err:#}").contains("asset name"), "{bad}: {err:#}");
    }
    // Nothing is written when a name is rejected.
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    let text = pack_with(r#"{"main.lua":"", "../escape.lua":""}"#, "{}");
    assert!(ppu_cli::tg16::unpack(&text, &out).is_err());
    assert!(!out.exists());
    assert!(!tmp.path().join("escape.lua").exists());
    // Hand-built toys are checked too.
    let toy = Toy {
        name: "x".into(),
        files: vec![("main.lua".into(), String::new())],
        assets: vec![("../x.png".into(), vec![])],
    };
    assert!(toy.write_dir(&out).is_err());
    assert!(toy.to_pack_json().is_err());
}

#[test]
fn rejects_bad_format_version_and_payloads() {
    let err = |text: &str| format!("{:#}", Toy::from_pack_json(text).unwrap_err());
    let files = r#""files":{"main.lua":""}"#;
    assert!(err(&format!(r#"{{"format":"ppu.toys/1","version":1,{files}}}"#)).contains("format"));
    assert!(err(&format!(r#"{{"version":1,{files}}}"#)).contains("format"));
    assert!(err(&format!(
        r#"{{"format":"ppu.toys/tg16","version":2,{files}}}"#
    ))
    .contains("version"));
    assert!(err(&format!(r#"{{"format":"ppu.toys/tg16",{files}}}"#)).contains("version"));
    assert!(err(&pack_with("{}", "{}")).contains("no .lua"));
    assert!(err(&pack_with(r#"{"main.lua":""}"#, r#"{"a.png":"!!"}"#)).contains("base64"));
    assert!(err("not json").contains("invalid"));

    // Unknown keys are ignored; assets may be omitted.
    let toy = Toy::from_pack_json(&format!(
        r#"{{"format":"ppu.toys/tg16","version":1,"extra":{{"a":1}},{files}}}"#
    ))
    .unwrap();
    assert!(toy.assets.is_empty());
}

#[test]
fn new_creates_assets_dir_and_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("fresh");
    success(&["tg16", "new", dir.to_str().unwrap()]);
    assert!(dir.join("assets").is_dir());
    let toy = load_toy(&dir).unwrap();
    assert_eq!(toy.name, "fresh");
    assert!(toy.assets.is_empty());
    // Default output name is <dir name>.tg16.json in the working directory.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ppu"))
        .args(["tg16", "pack", "fresh"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(tmp.path().join("fresh.tg16.json").is_file());
}
