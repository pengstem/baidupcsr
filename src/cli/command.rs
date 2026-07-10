use super::*;

pub(crate) enum CommandAction {
    Doctor { config_dir: Option<PathBuf> },
    VideoBench(VideoBenchCliOptions),
    TuiShell,
    UsageError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VideoBenchCliOptions {
    pub(crate) remote_path: String,
    pub(crate) bench_case: VideoBenchCase,
    pub(crate) duration_seconds: u64,
    pub(crate) output_path: Option<PathBuf>,
    pub(crate) redact: bool,
    pub(crate) chunk_size_bytes: Option<u64>,
    pub(crate) max_chunks: Option<usize>,
    pub(crate) prefetch_chunks: Option<usize>,
    pub(crate) reps: usize,
    pub(crate) range_sizes: Vec<u64>,
    pub(crate) positions: Vec<VideoBenchPosition>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VideoBenchCase {
    Source,
    Gateway,
    All,
}

impl VideoBenchCase {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Gateway => "gateway",
            Self::All => "all",
        }
    }

    pub(crate) const fn includes_source(self) -> bool {
        matches!(self, Self::Source | Self::All)
    }

    pub(crate) const fn includes_gateway(self) -> bool {
        matches!(self, Self::Gateway | Self::All)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VideoBenchPosition {
    Start,
    Percent(u8),
    Tail,
}

impl VideoBenchPosition {
    pub(crate) fn label(self) -> String {
        match self {
            Self::Start => "start".to_string(),
            Self::Percent(percent) => format!("p{percent}"),
            Self::Tail => "tail".to_string(),
        }
    }
}

impl Default for VideoBenchCliOptions {
    fn default() -> Self {
        Self {
            remote_path: String::new(),
            bench_case: VideoBenchCase::All,
            duration_seconds: 30,
            output_path: None,
            redact: true,
            chunk_size_bytes: None,
            max_chunks: None,
            prefetch_chunks: None,
            reps: 3,
            range_sizes: vec![1024 * 1024, 4 * 1024 * 1024],
            positions: vec![
                VideoBenchPosition::Start,
                VideoBenchPosition::Percent(10),
                VideoBenchPosition::Percent(50),
                VideoBenchPosition::Percent(90),
                VideoBenchPosition::Tail,
            ],
        }
    }
}

pub(crate) fn parse_command(args: &[String]) -> CommandAction {
    match args {
        [] => CommandAction::TuiShell,
        [cmd] if cmd == "tui" => CommandAction::TuiShell,
        [cmd] if cmd == "doctor" => CommandAction::Doctor { config_dir: None },
        [cmd, flag] if cmd == "doctor" && flag == "--redact" => {
            CommandAction::Doctor { config_dir: None }
        }
        [cmd, flag, dir_flag, dir]
            if cmd == "doctor" && flag == "--redact" && dir_flag == "--config-dir" =>
        {
            CommandAction::Doctor {
                config_dir: Some(PathBuf::from(dir)),
            }
        }
        [cmd, rest @ ..] if cmd == "video-bench" => parse_video_bench_command(rest)
            .map(CommandAction::VideoBench)
            .unwrap_or(CommandAction::UsageError),
        _ => CommandAction::UsageError,
    }
}

pub(crate) fn parse_video_bench_command(args: &[String]) -> Result<VideoBenchCliOptions, String> {
    let mut options = VideoBenchCliOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--path" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --path value".to_string())?;
                options.remote_path = normalize_remote_command_path(value);
            }
            "--case" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --case value".to_string())?;
                options.bench_case = parse_video_bench_case(value)?;
            }
            "--duration" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --duration value".to_string())?;
                options.duration_seconds = parse_positive_u64(value, "--duration")?;
            }
            "--output" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --output value".to_string())?;
                options.output_path = Some(PathBuf::from(value));
            }
            "--chunk-size" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --chunk-size value".to_string())?;
                options.chunk_size_bytes = Some(parse_byte_size_arg(value)?);
            }
            "--max-chunks" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --max-chunks value".to_string())?;
                options.max_chunks = Some(parse_positive_usize(value, "--max-chunks")?);
            }
            "--prefetch-chunks" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --prefetch-chunks value".to_string())?;
                options.prefetch_chunks = Some(parse_positive_usize(value, "--prefetch-chunks")?);
            }
            "--reps" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --reps value".to_string())?;
                options.reps = parse_positive_usize(value, "--reps")?;
            }
            "--range-sizes" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --range-sizes value".to_string())?;
                options.range_sizes = parse_range_sizes_arg(value)?;
            }
            "--positions" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "missing --positions value".to_string())?;
                options.positions = parse_video_bench_positions(value)?;
            }
            "--redact" => {
                options.redact = true;
            }
            _ => return Err(format!("unknown video-bench option {}", args[index])),
        }
        index += 1;
    }

    if options.remote_path.is_empty() {
        return Err("missing --path".to_string());
    }
    Ok(options)
}

pub(crate) fn parse_video_bench_case(value: &str) -> Result<VideoBenchCase, String> {
    match value {
        "source" => Ok(VideoBenchCase::Source),
        "gateway" => Ok(VideoBenchCase::Gateway),
        "all" => Ok(VideoBenchCase::All),
        _ => Err(format!("invalid --case {value}")),
    }
}

pub(crate) fn parse_positive_u64(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{name} must be a positive integer"))
}

pub(crate) fn parse_positive_usize(value: &str, name: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{name} must be a positive integer"))
}

pub(crate) fn parse_range_sizes_arg(value: &str) -> Result<Vec<u64>, String> {
    let sizes = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(parse_byte_size_arg)
        .collect::<Result<Vec<_>, _>>()?;
    if sizes.is_empty() {
        Err("--range-sizes must include at least one size".to_string())
    } else {
        Ok(sizes)
    }
}

pub(crate) fn parse_byte_size_arg(value: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("byte size must not be empty".to_string());
    }
    let digit_count = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return Err(format!("invalid byte size {value}"));
    }
    let number = trimmed[..digit_count]
        .parse::<u64>()
        .map_err(|_| format!("invalid byte size {value}"))?;
    let suffix = trimmed[digit_count..].to_ascii_lowercase();
    let multiplier = match suffix.as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        _ => return Err(format!("invalid byte size suffix {suffix}")),
    };
    number
        .checked_mul(multiplier)
        .filter(|size| *size > 0)
        .ok_or_else(|| format!("invalid byte size {value}"))
}

pub(crate) fn parse_video_bench_positions(value: &str) -> Result<Vec<VideoBenchPosition>, String> {
    let positions = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(parse_video_bench_position)
        .collect::<Result<Vec<_>, _>>()?;
    if positions.is_empty() {
        Err("--positions must include at least one position".to_string())
    } else {
        Ok(positions)
    }
}

pub(crate) fn parse_video_bench_position(value: &str) -> Result<VideoBenchPosition, String> {
    if value.eq_ignore_ascii_case("start") {
        return Ok(VideoBenchPosition::Start);
    }
    if value.eq_ignore_ascii_case("tail") || value.eq_ignore_ascii_case("end") {
        return Ok(VideoBenchPosition::Tail);
    }
    let percent = value
        .trim_start_matches('p')
        .parse::<u8>()
        .map_err(|_| format!("invalid bench position {value}"))?;
    if percent > 100 {
        return Err(format!("invalid bench position {value}"));
    }
    if percent == 0 {
        Ok(VideoBenchPosition::Start)
    } else if percent == 100 {
        Ok(VideoBenchPosition::Tail)
    } else {
        Ok(VideoBenchPosition::Percent(percent))
    }
}
