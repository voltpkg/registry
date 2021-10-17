use lz4::EncoderBuilder;
use serde::{Deserialize, Serialize};
use simple_input::input;
use std::{
    fs::{read_dir, File},
    io,
};

#[derive(Serialize, Deserialize)]
pub struct Libraries {
    libs: Vec<String>,
}

fn compress(source: String, destination: String) {
    let mut input_file = File::open(source).unwrap();
    let output_file = File::create(destination).unwrap();
    let mut encoder = EncoderBuilder::new().level(4).build(output_file).unwrap();
    io::copy(&mut input_file, &mut encoder).unwrap();
    let (_, _) = encoder.finish();
}

fn main() {
    let version: String = input("version: ");
    let files = read_dir(r"C:\Users\xtrem\Desktop\volt\registry\bin").unwrap();
    let mut libs: Vec<String> = vec![];

    for file in files {
        let file = file.unwrap();

        let name = file
            .file_name()
            .to_str()
            .unwrap()
            .to_string()
            .replace(".bin", "");

        libs.push(name);
    }

    let version = version.trim();

    let version_file = File::create(format!(
        r"C:\Users\xtrem\Desktop\volt\registry\versions\{}.bin",
        version
    ))
    .unwrap();

    let lib = Libraries { libs };

    bincode::serialize_into(version_file, &lib).unwrap();

    compress(
        format!(
            r"C:\Users\xtrem\Desktop\volt\registry\versions\{}.bin",
            version
        ),
        format!(
            r"C:\Users\xtrem\Desktop\volt\registry\versions\{}.bin",
            version
        ),
    );
}
