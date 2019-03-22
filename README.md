# debug-try

This crate contains an attribute macro that can help you debug errors in your program.

In a function marked with the `#[debug_try]` attribute, any errors propagated with the `?` operator inside that function will be logged, printing the file, line and column to standard output.

If `nested = true` is set in the attribute, then the same will apply to functions and closures defined inside the marked function as well.

**Note.** This crate requires the `proc_macro_diagnostic` and `proc_macro_span` features, so only Rust nightly is supported.

## Example

```rust
// my_func.rs
use std::{error, fs, io, path};
use debug_try::debug_try;

#[debug_try(nested = true)]
fn my_func() -> Result<(), Box<dyn error::Error>> {
    fn file_size<P: AsRef<path::Path>>(file: P) -> Result<usize, io::Error> {
        let data = fs::read(file)?;
        Ok(data.len())
    }

    println!("file size = {}", file_size("non_existing_file.txt")?);
    Ok(())
}
```

When invoked, the following will be printed to standard output:

```text
Error propagated (my_func.rs:8:33): No such file or directory (os error 2)
Error propagated (my_func.rs:12:65): No such file or directory (os error 2)
```