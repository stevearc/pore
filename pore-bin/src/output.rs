use std::io::Write;

use pore_core::FileSearchResult;
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

use crate::config::SearchConfig;

/// Prints the search results to stdout
pub fn print_results(
    results: Vec<FileSearchResult>,
    conf: &SearchConfig,
) -> Result<bool, anyhow::Error> {
    let mut stdout = StandardStream::stdout(conf.color.clone().into());
    // TODO make colors configurable
    let mut filename_color = ColorSpec::new();
    filename_color.set_fg(Some(Color::Magenta));
    let default_color = ColorSpec::new();
    let mut line_number_color = ColorSpec::new();
    line_number_color.set_fg(Some(Color::Green));

    for (i, result) in results.iter().enumerate() {
        if conf.json {
            println!("{}", serde_json::to_string(&result)?);
        } else {
            stdout.set_color(&filename_color)?;
            writeln!(&mut stdout, "{}", result.file().to_string_lossy())?;
            for line in result.lines() {
                stdout.set_color(&line_number_color)?;
                write!(&mut stdout, "{}", line.number)?;
                stdout.set_color(&default_color)?;
                writeln!(&mut stdout, ":{}", line.text)?;
            }
            if !conf.filename_only {
                if i < results.len() - 1 {
                    println!("");
                }
            }
        }
    }
    Ok(results.len() > 0)
}
