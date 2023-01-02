#![doc = include_str!("../README.md")]

use anyhow::{Context, Result};
use clap::{crate_description, crate_name, crate_version, Parser, ValueHint};
use core::fmt;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;
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

#[derive(Parser, Debug)]
#[command(name = crate_name!(), about = crate_description!(), version = crate_version!())]
pub struct Args {
    /// Time from start in seconds. May be negative.
    #[arg(short = 's', long)]
    pub start: f64,

    /// Duration of clip in seconds.
    #[arg(short = 'd', long)]
    pub duration: f64,

    /// Output subtitle file; stdout if not present.
    #[arg(short = 'o', long, value_hint = ValueHint::FilePath)]
    pub output: Option<PathBuf>,

    /// Input subtitle file; stdin if not present.
    #[arg(short = 'i', long, value_hint = ValueHint::FilePath)]
    pub input: Option<PathBuf>,
}

fn trim_subtitles(
    subtitles: Vec<SubtitleEntry>,
    start: f64,
    duration: f64,
) -> Vec<(TimeSpan, String)> {
    let start_delta =
        TimeDelta::from_components(0, 0, start.trunc() as i64, (start.fract() * 1000.0) as i64);
    let end_point = TimePoint::from_components(
        0,
        0,
        duration.trunc() as i64,
        (duration.fract() * 1000.0) as i64,
    );
    subtitles
        .into_iter()
        .filter_map(|entry| {
            let mut new_timespan = entry.timespan - start_delta;
            if new_timespan.end.is_negative() || new_timespan.start >= end_point {
                return None;
            }
            if new_timespan.start.is_negative() {
                new_timespan =
                    TimeSpan::new(TimePoint::from_components(0, 0, 0, 0), new_timespan.end);
            }
            if new_timespan.end > end_point {
                new_timespan = TimeSpan::new(new_timespan.start, end_point);
            }
            let line = entry.line.unwrap_or_else(|| String::new());
            Some((new_timespan, line))
        })
        .collect()
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

    let input_string = match options.input {
        Some(input) => {
            let subtitle_file = File::open(&input).with_context(|| {
                format!("Could not open `{}' for reading.", input.to_string_lossy())
            })?;
            io::read_to_string(subtitle_file)
        }
        None => io::read_to_string(io::stdin()),
    }
    .context("Could not read input file into string")?;

    let subtitles = SrtFile::parse(&input_string)
        .map_err(|e| SubtitleError(e))
        .context("Could not parse input file as SRT")?
        .get_subtitle_entries()
        .map_err(|e| SubtitleError(e))
        .context("Could not retrieve subtitle entries")?;

    let subtitles = trim_subtitles(subtitles, options.start, options.duration);

    let subtitle_data = SrtFile::create(subtitles)
        .map_err(|e| SubtitleError(e))
        .context("Could not create subtitles from data")?
        .to_data()
        .map_err(|e| SubtitleError(e))
        .context("Could not create subtitle data")?;

    match options.output {
        Some(output) => {
            // Write to file.
            let mut subtitle_file = File::create(&output).with_context(|| {
                format!("Could not open `{}' for writing.", output.to_string_lossy())
            })?;
            subtitle_file.write_all(&subtitle_data).with_context(|| {
                format!(
                    "Could not write subtitles to `{}'.",
                    output.to_string_lossy()
                )
            })?;
        }
        None => {
            // Write to stdout.
            io::stdout()
                .write_all(&subtitle_data)
                .context("Could not write subtitles to stdout.")?;
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
