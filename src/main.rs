#![doc = include_str!("../README.md")]

use anyhow::{ensure, Context, Result};
use clap::{crate_description, crate_name, crate_version, Parser, ValueHint};
use core::fmt;
use std::error::Error;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;
use std::{env, fs};
use subparse::timetypes::{TimeDelta, TimePoint, TimeSpan};
use subparse::{SrtFile, SubtitleEntry, SubtitleFileInterface};

#[derive(Debug)]
struct SubtitleError(subparse::errors::Error);

impl fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Error for SubtitleError {
    // Implement this to return the lower level source of this Error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

fn parse_range<T, U>(s: &str) -> Result<(T, U), Box<dyn Error + Send + Sync + 'static>>
where
    T: std::str::FromStr,
    T::Err: Error + Send + Sync + 'static,
    U: std::str::FromStr,
    U::Err: Error + Send + Sync + 'static,
{
    let pos = s
        .find('-')
        .ok_or_else(|| format!("invalid KEY=value: no `-` found in `{}`", s))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}

#[derive(Parser, Debug)]
#[command(name = crate_name!(), about = crate_description!(), version = crate_version!())]
pub struct Args {
    /// Output subtitle file; stdout if not present.
    #[arg(short = 'o', long, value_hint = ValueHint::FilePath)]
    pub output: Option<PathBuf>,

    /// Input subtitle file; stdin if not present.
    #[arg(short = 'i', long, value_hint = ValueHint::FilePath)]
    pub input: Option<PathBuf>,

    /// Blocks, from start to finish.
    #[arg(required=true, value_parser=parse_range::<f64, f64>)]
    pub blocks: Vec<(f64, f64)>,
}

fn trim_subtitles(
    subtitles: &Vec<SubtitleEntry>,
    start: f64,
    duration: f64,
    new_subtitles: &mut Vec<(TimeSpan, std::string::String)>,
) {
    let start_delta =
        TimeDelta::from_components(0, 0, start.trunc() as i64, (start.fract() * 1000.0) as i64);
    let end_point = TimePoint::from_components(
        0,
        0,
        duration.trunc() as i64,
        (duration.fract() * 1000.0) as i64,
    );
    new_subtitles.extend(subtitles.into_iter().filter_map(|entry| {
        let mut new_timespan = entry.timespan - start_delta;
        if new_timespan.end.is_negative() || new_timespan.start >= end_point {
            return None;
        }
        if new_timespan.start.is_negative() {
            new_timespan = TimeSpan::new(TimePoint::from_components(0, 0, 0, 0), new_timespan.end);
        }
        if new_timespan.end > end_point {
            new_timespan = TimeSpan::new(new_timespan.start, end_point);
        }
        let line = entry.line.clone().unwrap_or_else(|| String::new());
        Some((new_timespan, line))
    }));
}

fn validate_blocks(blocks: &[(f64, f64)]) -> Result<()> {
    let mut time = 0.0;
    for block in blocks {
        ensure!(block.0 < block.1);
        ensure!(time <= block.0);
        time = block.1;
    }
    Ok(())
}

fn try_main() -> Result<()> {
    println!(
        "{}",
        env::args()
            .into_iter()
            .map(|x| format!("“{x}”"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let options = Args::parse();

    // Validate blocks are in sequence and disjoint.
    validate_blocks(&options.blocks)?;

    let input_string = match options.input {
        Some(input) => fs::read_to_string(&input).with_context(|| {
            format!(
                "Could not read subtitles from file `{}'",
                input.to_string_lossy()
            )
        })?,
        None => {
            let mut result = String::new();
            io::stdin()
                .read_to_string(&mut result)
                .context("Could not read subtitles from stdin")?;
            result
        }
    };

    let subtitles = SrtFile::parse(&input_string)
        .map_err(|e| SubtitleError(e))
        .context("Could not parse input file as SRT")?
        .get_subtitle_entries()
        .map_err(|e| SubtitleError(e))
        .context("Could not retrieve subtitle entries")?;

    let mut new_subtitles = Vec::new();
    for block in options.blocks {
        trim_subtitles(&subtitles, block.0, block.1, &mut new_subtitles);
    }

    let subtitle_data = SrtFile::create(new_subtitles)
        .map_err(|e| SubtitleError(e))
        .context("Could not create subtitles from data")?
        .to_data()
        .map_err(|e| SubtitleError(e))
        .context("Could not create subtitle data")?;

    match options.output {
        Some(output) => fs::write(&output, &subtitle_data).with_context(|| {
            format!(
                "Could not write subtitles to file `{}'",
                output.to_string_lossy()
            )
        })?,
        None => {
            // Write to stdout.
            io::stdout()
                .write_all(&subtitle_data)
                .context("Could not write subtitles to stdout")?;
        }
    }

    Ok(())
}

fn main() {
    match try_main() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
