use anyhow::{bail, ensure, Context, Result};
use std::path::{Path, PathBuf};

const HELP: &str = "ppu — standalone ppu.toys authoring (60 fps, 256×224)

  ppu new <dir>
  ppu pack <dir> [-o out.ppu.json]
  ppu unpack <file.ppu.json> <dir>
  ppu check <dir|file.ppu.json> [--duration seconds] [--seek seconds,...]
            [--loop seconds] [--allow-overflow]
  ppu render <dir|file.ppu.json> --at seconds,... -o <directory>
  ppu docs [topic|all]
  ppu tg16 new <dir>
  ppu tg16 pack <dir> [-o out.tg16.json]
  ppu tg16 unpack <file.tg16.json> <dir>
  ppu tg16 render <toy> --at seconds,... -o <directory>
  ppu tg16 check <toy> [--duration seconds] [--allow-overflow]
  ppu tg16 audio <toy> [--duration seconds] -o <file.wav>
  ppu tg16 rom <toy> [--duration seconds] [--loop] -o <file.pce>
  ppu --version

check defaults to 30 seconds; includes the endpoint. --seek compares direct
and backward seeks with playback. --loop adds loop comparisons at sample times.
render plays from zero to the last requested time and writes numbered PNGs.
Times round to the nearest 60 Hz frame. PNG sources convert on every load.
A tg16 <toy> is a directory, a .lua file, or a .tg16.json pack.
See ppu docs cli for manifests, examples, and supported features.";

fn option(args: &mut Vec<String>, flag: &str) -> Result<Option<String>> {
    if let Some(i) = args.iter().position(|arg| arg == flag) {
        args.remove(i);
        ensure!(i < args.len(), "{flag} requires a value");
        Ok(Some(args.remove(i)))
    } else {
        Ok(None)
    }
}

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    if args == ["--version"] || args == ["-V"] {
        println!("ppu {} (ppu.toys/1)", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let command = args.remove(0);
    if command == "tg16" {
        return tg16(args);
    }
    if command == "docs" {
        ensure!(args.len() <= 1, "usage: ppu docs [topic|all]");
        print!("{}", ppu_cli::docs(args.first().map(String::as_str))?);
        return Ok(());
    }
    ensure!(
        !args.is_empty(),
        "{command} requires a path; use ppu --help"
    );
    let input = PathBuf::from(args.remove(0));
    match command.as_str() {
        "new" => {
            ensure!(args.is_empty(), "usage: ppu new <dir>");
            ppu_cli::new_project(&input)?;
            println!("Created {}. Edit main.lua and assets/floor.png; run ppu docs cli for the authoring loop.", input.display());
        }
        "pack" => {
            let output = option(&mut args, "-o")?.unwrap_or_else(|| {
                format!(
                    "{}.ppu.json",
                    input.file_name().and_then(|s| s.to_str()).unwrap_or("out")
                )
            });
            ensure!(args.is_empty(), "usage: ppu pack <dir> [-o out.ppu.json]");
            std::fs::write(&output, ppu_cli::pack(&input)?)
                .with_context(|| format!("cannot write {output}"))?;
            println!("Packed {output}");
        }
        "unpack" => {
            ensure!(args.len() == 1, "usage: ppu unpack <file> <dir>");
            let text = std::fs::read_to_string(&input)?;
            ppu_cli::unpack(&text, Path::new(&args[0]))?;
            println!("Unpacked into {}", args[0]);
        }
        "check" => {
            let duration = option(&mut args, "--duration")?
                .unwrap_or_else(|| "30".into())
                .parse()
                .context("invalid duration")?;
            let seeks = option(&mut args, "--seek")?
                .map(|s| ppu_cli::parse_times(&s))
                .transpose()?
                .unwrap_or_default();
            let period = option(&mut args, "--loop")?
                .map(|s| s.parse::<f64>())
                .transpose()
                .context("invalid loop period")?;
            let allow_overflow = if let Some(i) = args.iter().position(|a| a == "--allow-overflow")
            {
                args.remove(i);
                true
            } else {
                false
            };
            ensure!(args.is_empty(), "unknown check arguments: {args:?}");
            ppu_cli::check(&input, duration, &seeks, period, allow_overflow)?;
        }
        "render" => {
            let times = option(&mut args, "--at")?.context("render requires --at seconds,...")?;
            let output = option(&mut args, "-o")?.context("render requires -o <directory>")?;
            ensure!(args.is_empty(), "unknown render arguments: {args:?}");
            ppu_cli::render(&input, &ppu_cli::parse_times(&times)?, Path::new(&output))?;
        }
        _ => bail!("unknown command {command:?}; use ppu --help"),
    }
    Ok(())
}

/// `ppu tg16 <new|pack|unpack|render|check|audio|rom> ...`: TurboGrafx-16 toys
/// (experimental).
fn tg16(mut args: Vec<String>) -> Result<()> {
    ensure!(
        args.len() >= 2,
        "usage: ppu tg16 <new|pack|unpack|render|check|audio|rom> <path> ..."
    );
    let command = args.remove(0);
    let input = PathBuf::from(args.remove(0));
    match command.as_str() {
        "new" => {
            ensure!(args.is_empty(), "usage: ppu tg16 new <dir>");
            ppu_cli::tg16::new_project(&input)?;
            println!(
                "Created {}. Edit main.lua, then run ppu tg16 render.",
                input.display()
            );
        }
        "pack" => {
            let output = option(&mut args, "-o")?.unwrap_or_else(|| {
                let toy = std::fs::canonicalize(&input).unwrap_or_else(|_| input.clone());
                let stem = if toy.is_dir() {
                    toy.file_name()
                } else {
                    toy.file_stem()
                };
                let stem = stem.and_then(|s| s.to_str()).unwrap_or("toy");
                format!("{stem}.tg16.json")
            });
            ensure!(
                args.is_empty(),
                "usage: ppu tg16 pack <dir> [-o out.tg16.json]"
            );
            std::fs::write(&output, ppu_cli::tg16::pack(&input)?)
                .with_context(|| format!("cannot write {output}"))?;
            println!("Packed {output}");
        }
        "unpack" => {
            ensure!(
                args.len() == 1,
                "usage: ppu tg16 unpack <file.tg16.json> <dir>"
            );
            let text = std::fs::read_to_string(&input)
                .with_context(|| format!("cannot read {}", input.display()))?;
            ppu_cli::tg16::unpack(&text, Path::new(&args[0]))?;
            println!("Unpacked into {}", args[0]);
        }
        "render" => {
            let times = option(&mut args, "--at")?.context("render requires --at seconds,...")?;
            let output = option(&mut args, "-o")?.context("render requires -o <directory>")?;
            ensure!(args.is_empty(), "unknown render arguments: {args:?}");
            ppu_cli::tg16::render(&input, &ppu_cli::parse_times(&times)?, Path::new(&output))?;
        }
        "check" => {
            let duration = option(&mut args, "--duration")?
                .unwrap_or_else(|| "30".into())
                .parse()
                .context("invalid duration")?;
            let allow_overflow = if let Some(i) = args.iter().position(|a| a == "--allow-overflow")
            {
                args.remove(i);
                true
            } else {
                false
            };
            ensure!(args.is_empty(), "unknown check arguments: {args:?}");
            ppu_cli::tg16::check(&input, duration, allow_overflow)?;
        }
        "audio" => {
            let duration = option(&mut args, "--duration")?
                .unwrap_or_else(|| "8".into())
                .parse()
                .context("invalid duration")?;
            let output = option(&mut args, "-o")?.context("audio requires -o <file.wav>")?;
            ensure!(args.is_empty(), "unknown audio arguments: {args:?}");
            ppu_cli::tg16::audio(&input, duration, Path::new(&output))?;
        }
        "rom" => {
            let duration = option(&mut args, "--duration")?
                .unwrap_or_else(|| "10".into())
                .parse()
                .context("invalid duration")?;
            let output = option(&mut args, "-o")?.context("rom requires -o <file.pce>")?;
            let looping = if let Some(i) = args.iter().position(|a| a == "--loop") {
                args.remove(i);
                true
            } else {
                false
            };
            ensure!(args.is_empty(), "unknown rom arguments: {args:?}");
            ppu_cli::tg16::rom(&input, duration, looping, Path::new(&output))?;
        }
        _ => bail!("unknown tg16 command {command:?}; use ppu --help"),
    }
    Ok(())
}
