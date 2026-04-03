mod audio;
mod server;

use ci_music::vocoder;
use clap::{Parser, Subcommand};
use std::time::Instant;

#[derive(Parser)]
#[command(name = "ci_music", about = "Cochlear implant music simulator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Process an audio file (WAV, MP3, FLAC, OGG, AAC/M4A)
    Process {
        /// Input audio file (WAV, MP3, FLAC, OGG/Vorbis, AAC, M4A)
        #[arg(short, long)]
        input: String,
        /// Output WAV file
        #[arg(short, long)]
        output: String,
        /// Number of frequency channels (8 ≈ typical CI, 4 = dramatic, 16 = better)
        #[arg(short, long, default_value_t = 8)]
        channels: usize,
        /// Processing strategy: "cis" (standard CI), "fs4" (MED-EL FineHearing — pitch-preserving apical channels), or "fft" (original)
        #[arg(long, default_value = "cis")]
        strategy: String,
        /// Carrier type: "noise" (band-limited noise, classic CI demo sound) or "sine" (tonal)
        #[arg(long, default_value = "noise")]
        carrier: String,
        /// Print processing details
        #[arg(short, long)]
        verbose: bool,
    },
    /// Start the web UI
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Process { input, output, channels, strategy, carrier, verbose } => {
            let strat = match strategy.to_lowercase().as_str() {
                "fft" => vocoder::Strategy::Fft,
                "fs4" => vocoder::Strategy::Fs4,
                _ => vocoder::Strategy::Cis,
            };
            let carr = match carrier.to_lowercase().as_str() {
                "sine" => vocoder::Carrier::Sine,
                _ => vocoder::Carrier::Noise,
            };

            let (samples, sample_rate) = audio::load_audio_file(&input).unwrap_or_else(|e| {
                eprintln!("Error reading {input}: {e}");
                std::process::exit(1);
            });

            let duration_secs = samples.len() as f32 / sample_rate as f32;

            if verbose {
                println!("Cochlear Implant Simulator");
                println!("  Strategy  : {strategy}");
                println!("  Carrier   : {carrier}");
                println!("  Channels  : {channels}");
                println!("  Freq range: 70 – 8500 Hz");
                println!(
                    "  Duration  : {}m {:02}s",
                    duration_secs as u32 / 60,
                    duration_secs as u32 % 60
                );
                print!("  Processing...");
            }

            let start = Instant::now();
            let processed = vocoder::process(&samples, sample_rate, channels, strat, carr);
            let elapsed = start.elapsed();

            if verbose {
                println!(" done in {}ms", elapsed.as_millis());
            }

            vocoder::write_wav(&output, &processed, sample_rate).unwrap_or_else(|e| {
                eprintln!("Error writing {output}: {e}");
                std::process::exit(1);
            });

            if verbose {
                println!("  Written to: {output}");
            }
        }

        Command::Serve { port } => {
            server::run(port).await;
        }
    }
}
