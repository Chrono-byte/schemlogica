// Small script to read an existing .litematic file and print its top-level metadata.
// Usage: cargo run --bin read_litematic -- path/to/file.litematic

use std::env;
use std::fs::File;
use std::io::BufReader;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: read_litematic <file.litematic>");
        std::process::exit(2);
    }
    let path = &args[1];
    let file = File::open(path)?;
    let mut buf = BufReader::new(file);
    // Use hematite-nbt crate's Blob reader to read gzip-compressed NBT.
    let blob = nbt::Blob::from_gzip_reader(&mut buf)?;

    // Print the entire NBT tree (pretty-printed via Display impl)
    println!("{}", &blob);
    Ok(())
}
