use std::{error, fs, io, path};

use debug_try::*;

#[debug_try(nested = true)]
fn main() -> Result<(), Box<dyn error::Error>> {
    fn file_size<P: AsRef<path::Path>>(file: P) -> Result<usize, io::Error> {
        let data = fs::read(file)?;
        Ok(data.len())
    }

    println!("file size = {}", file_size("non_existing_file.txt")?);
    Ok(())
}
