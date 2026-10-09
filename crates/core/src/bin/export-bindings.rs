use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let first = arguments
        .next()
        .ok_or("Usage: export-bindings [--check] OUTPUT")?;
    let (check, output) = if first == "--check" {
        (true, arguments.next().ok_or("Missing output path")?)
    } else {
        (false, first)
    };
    if arguments.next().is_some() {
        return Err("Unexpected argument".into());
    }
    let output = PathBuf::from(output);
    let generated = echocord_core::model::typescript_contract();
    if check {
        if fs::read_to_string(output)? != generated {
            return Err("Generated TypeScript contract is out of date".into());
        }
    } else {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(output, generated)?;
    }
    Ok(())
}
