//! `ppu tg16 rom`: the command line. The ROM format itself is tested in
//! tg16-core (tests/rom.rs) and against Mednafen in tg16_rom_emu.rs.
use std::process::Command;

fn toy(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tg16-core/toys")
        .join(name)
}

#[test]
fn rom_command_writes_a_hucard_image() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("sunset.pce");
    let o = Command::new(env!("CARGO_BIN_EXE_ppu"))
        .args(["tg16", "rom"])
        .arg(toy("sunset.lua"))
        .args(["--duration", "0.5", "--loop", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("30 frames, looping"), "{stdout}");
    let rom = std::fs::read(&out).unwrap();
    assert!(rom.len().is_power_of_two());
    assert_eq!(&rom[0x1F00..0x1F08], b"PPUTG16R");
}

#[test]
fn rom_command_needs_an_output_and_a_duration() {
    let o = Command::new(env!("CARGO_BIN_EXE_ppu"))
        .args(["tg16", "rom"])
        .arg(toy("sunset.lua"))
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("-o <file.pce>"));
    let o = Command::new(env!("CARGO_BIN_EXE_ppu"))
        .args(["tg16", "rom"])
        .arg(toy("sunset.lua"))
        .args(["--duration", "0", "-o", "/dev/null"])
        .output()
        .unwrap();
    assert!(!o.status.success());
}
