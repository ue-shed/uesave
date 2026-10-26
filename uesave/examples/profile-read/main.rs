//! Time parsing separately from I/O and JSON, or print coarse span durations.
use std::{
    hint::black_box,
    io::{Cursor, Read},
    time::{Duration, Instant},
};
use uesave::{CompressedReader, Save, SaveReader, StructType, Types};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: profile-read SAVE [--compressed] [--hints FILE.json] [--paths JSON] [--json] [--trace]")?;
    let (mut compressed, mut json, mut trace) = (false, false, false);
    let mut hints = std::collections::HashMap::<String, String>::new();
    let mut paths: Option<Vec<String>> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compressed" => compressed = true,
            "--json" => json = true,
            "--trace" => trace = true,
            "--hints" => {
                hints = serde_json::from_slice(&std::fs::read(
                    args.next().ok_or("missing hints file")?,
                )?)?
            }
            "--paths" => {
                paths = Some(serde_json::from_str(
                    &args.next().ok_or("missing paths JSON")?,
                )?)
            }
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    let data = std::fs::read(path)?;
    let read = || -> Result<Save, Box<dyn std::error::Error>> {
        let mut types = Types::new();
        let mut nested = vec![];
        for (path, ty) in &hints {
            if ty == "Properties" {
                nested.push(path.clone());
            } else {
                types.add(path.clone(), StructType::from(ty.as_str()));
            }
        }
        let mut reader = SaveReader::new().error_to_raw(true).types(types);
        if !nested.is_empty() {
            reader = reader.nested(nested);
        }
        if let Some(paths) = &paths {
            reader = reader.only(paths.clone());
        }
        Ok(if compressed {
            let mut stream = CompressedReader::new(&data)?;
            let mut prefix = [0; 4];
            stream.read_exact(&mut prefix)?;
            reader.read_seekable(stream.window(4, u32::from_le_bytes(prefix) as u64)?)?
        } else {
            reader.read_seekable(Cursor::new(&data))?
        })
    };
    if trace {
        #[cfg(not(feature = "tracing"))]
        return Err("--trace requires cargo run --features tracing".into());
        #[cfg(feature = "tracing")]
        {
            use tracing_subscriber::{layer::SubscriberExt, Layer};
            let subscriber = tracing_subscriber::registry().with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(std::io::stderr)
                    .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
                    .with_filter(tracing_subscriber::filter::filter_fn(|m| {
                        m.target() == "uesave::perf"
                    })),
            );
            return tracing::subscriber::with_default(subscriber, || {
                let save = read()?;
                if json {
                    let _span = tracing::info_span!(target: "uesave::perf", "save.serialize_json")
                        .entered();
                    black_box(serde_json::to_vec(&save)?);
                }
                Ok(())
            });
        }
    }
    drop(read()?); // Warm up without retaining a second save during the parse measurement.
    time("parse + drop", || {
        black_box(read()?);
        Ok(())
    })?;
    if json {
        let save = read()?;
        time("serialize JSON", || {
            black_box(serde_json::to_vec(&save)?);
            Ok(())
        })?;
    }
    Ok(())
}

fn time(
    label: &str,
    mut f: impl FnMut() -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut samples = [0.0; 7];
    for sample in &mut samples {
        let start = Instant::now();
        let mut count = 0;
        while start.elapsed() < Duration::from_millis(250) {
            f()?;
            count += 1;
        }
        *sample = start.elapsed().as_secs_f64() * 1000.0 / count as f64;
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "{label}: median {:.3} ms (range {:.3}–{:.3}, 7 batches ≥250 ms)",
        samples[3], samples[0], samples[6]
    );
    Ok(())
}
