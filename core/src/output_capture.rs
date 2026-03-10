use crate::runner::redact_text;
use anyhow::{Context, Result};
use std::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

pub struct OutputCaptureHandles {
    stdout_task: tokio::task::JoinHandle<Result<()>>,
    stderr_task: tokio::task::JoinHandle<Result<()>>,
}

impl OutputCaptureHandles {
    pub async fn wait(self) -> Result<()> {
        self.stdout_task
            .await
            .context("stdout capture task panicked")??;
        self.stderr_task
            .await
            .context("stderr capture task panicked")??;
        Ok(())
    }
}

pub fn spawn_sanitized_output_capture<
    Stdout: AsyncRead + Unpin + Send + 'static,
    Stderr: AsyncRead + Unpin + Send + 'static,
>(
    stdout: Stdout,
    stderr: Stderr,
    stdout_file: File,
    stderr_file: File,
    redaction_patterns: Vec<String>,
) -> OutputCaptureHandles {
    let stdout_patterns = redaction_patterns.clone();
    let stdout_task =
        tokio::spawn(async move { pipe_and_redact(stdout, stdout_file, stdout_patterns).await });
    let stderr_task =
        tokio::spawn(async move { pipe_and_redact(stderr, stderr_file, redaction_patterns).await });
    OutputCaptureHandles {
        stdout_task,
        stderr_task,
    }
}

struct StreamingRedactor {
    patterns: Vec<String>,
    pending: String,
}

impl StreamingRedactor {
    fn new(patterns: Vec<String>) -> Self {
        Self {
            patterns,
            pending: String::new(),
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> String {
        self.pending.push_str(&String::from_utf8_lossy(chunk));
        let Some(last_newline) = self.pending.rfind('\n') else {
            return String::new();
        };
        let split_at = last_newline + 1;
        let emit = self.pending[..split_at].to_string();
        self.pending.drain(..split_at);
        redact_text(&emit, &self.patterns)
    }

    fn finish(mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        let final_text = redact_text(&self.pending, &self.patterns);
        self.pending.clear();
        final_text
    }
}

async fn pipe_and_redact<R: AsyncRead + Unpin>(
    mut reader: R,
    file: File,
    redaction_patterns: Vec<String>,
) -> Result<()> {
    let mut writer = tokio::fs::File::from_std(file);
    let mut redactor = StreamingRedactor::new(redaction_patterns);
    let mut buf = [0_u8; 8192];
    loop {
        let read = reader
            .read(&mut buf)
            .await
            .context("failed to read child output")?;
        if read == 0 {
            break;
        }
        let redacted = redactor.push_chunk(&buf[..read]);
        if !redacted.is_empty() {
            writer
                .write_all(redacted.as_bytes())
                .await
                .context("failed to write redacted output")?;
        }
    }
    let final_chunk = redactor.finish();
    if !final_chunk.is_empty() {
        writer
            .write_all(final_chunk.as_bytes())
            .await
            .context("failed to flush final redacted output")?;
    }
    writer
        .flush()
        .await
        .context("failed to flush redacted output file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_redactor_redacts_cross_chunk_secrets() {
        let mut redactor = StreamingRedactor::new(vec!["super-secret-value".to_string()]);
        let first = redactor.push_chunk(b"token=super-sec");
        let second = redactor.push_chunk(b"ret-value done\n");
        let final_chunk = redactor.finish();

        let combined = format!("{first}{second}{final_chunk}");
        assert!(combined.contains("[REDACTED]"));
        assert!(!combined.contains("super-secret-value"));
    }

    #[test]
    fn streaming_redactor_preserves_visible_text() {
        let mut redactor = StreamingRedactor::new(vec!["secret".to_string()]);
        let chunk = redactor.push_chunk(b"public=visible");
        let final_chunk = redactor.finish();
        assert_eq!(format!("{chunk}{final_chunk}"), "public=visible");
    }
}
