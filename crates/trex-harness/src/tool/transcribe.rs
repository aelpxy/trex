use std::time::Duration;

use anyhow::{Context, bail};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        audio::{AudioInput, AudioResponseFormat, CreateTranscriptionRequest},
        responses::FunctionTool,
    },
};
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{MAX_OUTPUT_BYTES, Tool, ToolContext, file::write};

// three hours of speech is a long meeting; longer recordings are transcribed up to here
const MAX_SECONDS: u32 = 3 * 60 * 60;
// speech stays clear at this bitrate, which keeps three hours near 32 MB
const OPUS_BITRATE: &str = "24k";
// files sent as they are, where the sandbox has no ffmpeg to extract the audio
const MAX_RAW_BYTES: u64 = 100 * 1024 * 1024;
// transcription runs at many times real time on a gpu, but a slow cpu server needs longer
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MISSING_EXIT_CODE: i32 = 3;
const TOO_LARGE_EXIT_CODE: i32 = 4;

// a speech-to-text server speaking the openai audio api, such as speaches or openai itself
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptionConfig {
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl TranscriptionConfig {
    // the `[transcription]` table of the catalog file, if the operator set one up
    pub fn from_toml(raw: &str) -> anyhow::Result<Option<Self>> {
        let mut table: toml::Table = toml::from_str(raw)?;
        table
            .remove("transcription")
            .map(|value| value.try_into().context("invalid [transcription]"))
            .transpose()
    }
}

pub struct Transcribe {
    client: Client<OpenAIConfig>,
    model: String,
}

impl Transcribe {
    pub fn new(config: TranscriptionConfig) -> Self {
        let mut openai = OpenAIConfig::new().with_api_base(config.base_url.trim_end_matches('/'));
        if let Some(key) = config.api_key.as_deref() {
            openai = openai.with_api_key(key);
        }
        Self {
            client: Client::with_config(openai),
            model: config.model,
        }
    }
}

#[derive(Deserialize)]
struct Args {
    path: String,
    language: Option<String>,
    timestamps: Option<bool>,
}

// read from the raw response, since speaches leaves out fields openai's typed one requires
#[derive(Deserialize)]
struct Transcript {
    text: String,
    language: Option<String>,
    duration: Option<f64>,
    #[serde(default)]
    segments: Option<Vec<Segment>>,
}

#[derive(Deserialize)]
struct Segment {
    start: f64,
    text: String,
}

impl Tool for Transcribe {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "transcribe".into(),
            description: Some(format!(
                "Transcribe the speech in an audio or video file in the sandbox, such as a recording, voice memo \
                 or downloaded video. The full transcript is saved next to the file and its start is returned. \
                 Recordings longer than {} hours are transcribed up to that point.",
                MAX_SECONDS / 3600
            )),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path of the audio or video file in the sandbox."},
                    "language": {"type": ["string", "null"], "description": "The spoken language as an ISO 639-1 code like en or de; detected when null."},
                    "timestamps": {"type": ["boolean", "null"], "description": "Start each line with when it's said, like [00:12:05]; false when null."}
                },
                "required": ["path", "language", "timestamps"],
                "additionalProperties": false,
            })),
            strict: Some(true),
            ..Default::default()
        }
    }

    fn call<'a>(
        &'a self,
        ctx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move {
            let args: Args = serde_json::from_value(args)?;
            let (audio, filename) = extract_audio(&ctx, &args.path).await?;
            let request = CreateTranscriptionRequest {
                file: AudioInput::from_vec_u8(filename, audio),
                model: self.model.clone(),
                language: args.language.filter(|language| !language.is_empty()),
                response_format: Some(AudioResponseFormat::VerboseJson),
                ..Default::default()
            };
            let body = tokio::time::timeout(
                TRANSCRIBE_TIMEOUT,
                self.client.audio().transcription().create_raw(request),
            )
            .await
            .context("transcription timed out")?
            .context("transcription failed")?;
            let transcript: Transcript =
                serde_json::from_slice(&body).context("unexpected transcription response")?;

            let text = render(&transcript, args.timestamps.unwrap_or(false));
            if text.trim().is_empty() {
                return Ok(format!("No speech was found in {}.", args.path));
            }
            let saved = transcript_path(&args.path);
            write(&ctx, &saved, text.as_bytes()).await?;
            Ok(format!(
                "{}\n\n{}",
                summary(&args.path, &saved, &transcript),
                preview(&text)
            ))
        })
    }
}

// the audio track as small mono opus when the sandbox has ffmpeg, so long videos upload quickly;
// otherwise the file as it is, which speech servers decode themselves
async fn extract_audio(ctx: &ToolContext<'_>, path: &str) -> anyhow::Result<(Vec<u8>, String)> {
    let script = format!(
        r#"[ -f "$1" ] || exit {MISSING_EXIT_CODE}
if command -v ffmpeg >/dev/null; then
  exec ffmpeg -nostdin -hide_banner -loglevel error -i "$1" -t {MAX_SECONDS} -vn -ac 1 -ar 16000 -c:a libopus -b:a {OPUS_BITRATE} -f ogg pipe:1
fi
[ "$(stat -c %s -- "$1")" -le {MAX_RAW_BYTES} ] || exit {TOO_LARGE_EXIT_CODE}
echo raw >&2
cat -- "$1""#
    );
    let argv = ["sh", "-c", &script, "sh", path].map(String::from).to_vec();
    let output = ctx
        .openshell
        .output(ctx.sandbox().await?, argv, Vec::new())
        .await?;
    match output.exit_code {
        Some(0) => {}
        Some(MISSING_EXIT_CODE) => bail!("{path} doesn't exist"),
        Some(TOO_LARGE_EXIT_CODE) => {
            bail!(
                "{path} is larger than {MAX_RAW_BYTES} bytes and there's no ffmpeg to extract its audio"
            )
        }
        _ => bail!(
            "couldn't read the audio in {path}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
    if output.stdout.is_empty() {
        bail!("{path} has no audio");
    }
    let raw = String::from_utf8_lossy(&output.stderr).trim() == "raw";
    let filename = if raw {
        path.rsplit('/').next().unwrap_or(path).to_owned()
    } else {
        "audio.ogg".to_owned()
    };
    Ok((output.stdout, filename))
}

fn render(transcript: &Transcript, timestamps: bool) -> String {
    match (&transcript.segments, timestamps) {
        (Some(segments), true) if !segments.is_empty() => segments
            .iter()
            .map(|segment| format!("[{}] {}", clock(segment.start), segment.text.trim()))
            .collect::<Vec<_>>()
            .join("\n"),
        (Some(segments), false) if !segments.is_empty() => segments
            .iter()
            .map(|segment| segment.text.trim())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => transcript.text.trim().to_owned(),
    }
}

fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        total / 60 % 60,
        total % 60
    )
}

// beside the recording, as talk.mp4 -> talk.transcript.txt
fn transcript_path(path: &str) -> String {
    let (dir, name) = path
        .rsplit_once('/')
        .map_or(("", path), |(dir, name)| (dir, name));
    let stem = name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map_or(name, |(stem, _)| stem);
    if dir.is_empty() && path.starts_with('/') {
        format!("/{stem}.transcript.txt")
    } else if dir.is_empty() {
        format!("{stem}.transcript.txt")
    } else {
        format!("{dir}/{stem}.transcript.txt")
    }
}

fn summary(path: &str, saved: &str, transcript: &Transcript) -> String {
    let mut facts = Vec::new();
    if let Some(duration) = transcript.duration {
        facts.push(clock(duration));
        if duration >= f64::from(MAX_SECONDS) - 1.0 {
            facts.push(format!(
                "only the first {} hours were transcribed",
                MAX_SECONDS / 3600
            ));
        }
    }
    if let Some(language) = transcript
        .language
        .as_deref()
        .filter(|language| !language.is_empty())
    {
        facts.push(format!("language {language}"));
    }
    let facts = if facts.is_empty() {
        String::new()
    } else {
        format!(" ({})", facts.join(", "))
    };
    format!("Transcribed {path}{facts}; the full transcript is in {saved}.")
}

fn preview(text: &str) -> String {
    if text.len() <= MAX_OUTPUT_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_OUTPUT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n[the transcript continues; read the saved file for the rest]",
        &text[..end]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_beside_the_recording() {
        assert_eq!(
            transcript_path("/sandbox/talks/keynote.mp4"),
            "/sandbox/talks/keynote.transcript.txt"
        );
        assert_eq!(transcript_path("memo.m4a"), "memo.transcript.txt");
        assert_eq!(transcript_path("/clip"), "/clip.transcript.txt");
        assert_eq!(transcript_path(".hidden"), ".hidden.transcript.txt");
    }

    #[test]
    fn renders_segments_with_or_without_times() {
        let transcript = Transcript {
            text: "Hello there. General Kenobi.".into(),
            language: Some("en".into()),
            duration: Some(3725.0),
            segments: Some(vec![
                Segment {
                    start: 0.0,
                    text: " Hello there.".into(),
                },
                Segment {
                    start: 3723.4,
                    text: " General Kenobi.".into(),
                },
            ]),
        };
        assert_eq!(
            render(&transcript, true),
            "[00:00:00] Hello there.\n[01:02:03] General Kenobi."
        );
        assert_eq!(render(&transcript, false), "Hello there.\nGeneral Kenobi.");
        assert_eq!(
            summary("talk.mp4", "talk.transcript.txt", &transcript),
            "Transcribed talk.mp4 (01:02:05, language en); the full transcript is in talk.transcript.txt."
        );
    }

    #[test]
    fn reads_the_config_table() {
        let raw = r#"
            [providers.local]
            base_url = "http://127.0.0.1:8699/v1"

            [transcription]
            base_url = "http://127.0.0.1:8000/v1/"
            model = "whisper"
        "#;
        let config = TranscriptionConfig::from_toml(raw).unwrap().unwrap();
        assert_eq!(config.model, "whisper");
        assert!(config.api_key.is_none());
        assert!(TranscriptionConfig::from_toml("").unwrap().is_none());
        assert!(TranscriptionConfig::from_toml("[transcription]\nbase_url = 1").is_err());
    }
}
