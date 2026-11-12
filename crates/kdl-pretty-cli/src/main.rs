//! `kdl-pretty [--check] [--indent N] [FILES...]`: formats KDL v2 files in
//! place, or stdin to stdout. Exit codes: 0 ok, 1 `--check` found files
//! that would change, 2 parse or I/O errors.

use std::io::{Read as _, Write as _};
use std::process::ExitCode;

use anyhow::Context as _;
use kdl_pretty::Options;
use miette::{LabeledSpan, MietteDiagnostic, NamedSource, Report};

const HELP: &str = "\
Format KDL v2 documents (comments kept, output deterministic).

Usage: kdl-pretty [--check] [--indent N] [FILES...]

With FILES, rewrites each file that changes in place. With no FILES
or `-`, reads stdin and writes the result to stdout.

Options:
  --check       Don't write; list files that would change on stderr
                and exit 1 if any would
  --indent N    Spaces per indentation level (default 2)
  -h, --help    Print this help
";

/// What happened to one input.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    Unchanged,
    Changed,
    Failed,
}

struct Args {
    check: bool,
    opts: Options,
    files: Vec<String>,
}

fn parse_args() -> anyhow::Result<Option<Args>> {
    let mut args = pico_args::Arguments::from_env();
    if args.contains(["-h", "--help"]) {
        return Ok(None);
    }
    let check = args.contains("--check");
    let indent = args
        .opt_value_from_str("--indent")
        .context("--indent takes a number")?
        .unwrap_or(2);
    let mut files = Vec::new();
    for arg in args.finish() {
        let arg = arg
            .into_string()
            .map_err(|a| anyhow::anyhow!("not UTF-8: {}", a.to_string_lossy()))?;
        if arg.starts_with("--") {
            anyhow::bail!("unknown option {arg}\n\n{HELP}");
        }
        files.push(arg);
    }
    Ok(Some(Args {
        check,
        opts: Options::default().indent(indent),
        files,
    }))
}

/// Formats `src`, or prints its parse error (as `name`) and returns `None`.
fn format(name: &str, src: &str, opts: &Options) -> Option<String> {
    match kdl_pretty::format_str(src, opts) {
        Ok(out) => Some(out),
        Err(err) => {
            // Rebuild each diagnostic so it renders against the named file
            // (kdl's own carry an unnamed copy of the input).
            if err.diagnostics.is_empty() {
                eprintln!("{name}: {err}");
            }
            for diag in err.diagnostics {
                let mut d =
                    MietteDiagnostic::new(diag.message.unwrap_or_else(|| "invalid KDL".to_owned()))
                        .with_severity(diag.severity)
                        .with_label(LabeledSpan::new_with_span(diag.label, diag.span));
                if let Some(help) = diag.help {
                    d = d.with_help(help);
                }
                let src = NamedSource::new(name, src.to_owned());
                eprintln!("{:?}", Report::new(d).with_source_code(src));
            }
            None
        }
    }
}

fn stdin(args: &Args) -> anyhow::Result<Outcome> {
    let mut src = String::new();
    std::io::stdin()
        .read_to_string(&mut src)
        .context("reading stdin")?;
    let Some(out) = format("<stdin>", &src, &args.opts) else {
        return Ok(Outcome::Failed);
    };
    if !args.check {
        std::io::stdout()
            .write_all(out.as_bytes())
            .context("writing stdout")?;
    }
    Ok(if out == src {
        Outcome::Unchanged
    } else {
        Outcome::Changed
    })
}

fn file(path: &str, args: &Args) -> anyhow::Result<Outcome> {
    let src = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let Some(out) = format(path, &src, &args.opts) else {
        return Ok(Outcome::Failed);
    };
    if out == src {
        return Ok(Outcome::Unchanged);
    }
    if args.check {
        eprintln!("{path}");
    } else {
        std::fs::write(path, out).with_context(|| format!("writing {path}"))?;
    }
    Ok(Outcome::Changed)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("kdl-pretty: {err:#}");
            return ExitCode::from(2);
        }
    };
    let mut worst = Outcome::Unchanged;
    let inputs: Vec<&str> = if args.files.is_empty() {
        vec!["-"]
    } else {
        args.files.iter().map(String::as_str).collect()
    };
    for input in inputs {
        let outcome = if input == "-" {
            stdin(&args)
        } else {
            file(input, &args)
        };
        let outcome = outcome.unwrap_or_else(|err| {
            eprintln!("kdl-pretty: {err:#}");
            Outcome::Failed
        });
        worst = worst.max(outcome);
    }
    match worst {
        Outcome::Failed => ExitCode::from(2),
        Outcome::Changed if args.check => ExitCode::from(1),
        _ => ExitCode::SUCCESS,
    }
}
