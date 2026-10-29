use acme::config::factory_schema;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

const SCHEMA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/factory.schema.json");
const SCHEMA_DISPLAY_PATH: &str = "schema/factory.schema.json";

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let check_only = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some(argument) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown argument: {argument}"),
            )
            .into());
        }
    };
    let mut generated = serde_json::to_string_pretty(&factory_schema())?;
    generated.push('\n');
    let path = Path::new(SCHEMA_PATH);

    if check_only {
        let existing = match fs::read_to_string(path) {
            Ok(existing) => existing,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{SCHEMA_DISPLAY_PATH} is missing; run `just generate-schema`"),
                )
                .into());
            }
            Err(error) => return Err(error.into()),
        };
        if existing != generated {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{SCHEMA_DISPLAY_PATH} is stale; run `just generate-schema`"),
            )
            .into());
        }
    } else {
        fs::create_dir_all(path.parent().expect("schema path has a parent"))?;
        fs::write(path, generated)?;
    }

    Ok(())
}
