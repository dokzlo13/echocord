#[cfg(not(windows))]
fn main() {
    eprintln!("Run the engine probe as a Windows executable.");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--voice-state-check") {
        let path = std::env::args_os()
            .nth(2)
            .ok_or("Missing private probe configuration")?;
        if std::env::args_os().nth(3).is_some() {
            return Err("Unexpected argument".into());
        }
        return voice_state_check(std::path::Path::new(&path));
    }
    use echocord_core::{
        discord::protocol::{GatewayCommand, RoomKey, VoiceCommand, VoiceSecret},
        model::Output,
    };
    let mut arguments = std::env::args_os().skip(1);
    let executable =
        std::path::PathBuf::from(arguments.next().ok_or("Missing application executable")?);
    let evidence = std::path::PathBuf::from(
        arguments
            .next()
            .ok_or("Missing private evidence directory")?,
    );
    if arguments.next().is_some() {
        return Err("Unexpected argument".into());
    }
    std::fs::create_dir_all(&evidence)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let credential = evidence.join(format!("empty-probe-credential-{nonce}"));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&credential)?;
    let result = (|| {
        rejection(
            &executable,
            "--gateway-worker",
            GatewayCommand::Init {
                token_file: credential.clone(),
            },
            true,
        )?;
        rejection(
            &executable,
            "--gateway-worker",
            serde_json::json!({"type":"init","token":"private-token-canary"}),
            false,
        )?;
        rejection(
            &executable,
            "--voice-worker",
            VoiceCommand::Init {
                key: RoomKey {
                    destination_id: "discord-1".into(),
                    intent: 1,
                    server_id: "1".into(),
                    channel_id: "2".into(),
                },
                gateway_epoch: 0,
                credential_revision: 1,
                source_id: "mix-handle:0".into(),
                secret: VoiceSecret {
                    endpoint: "private-endpoint-canary".into(),
                    session_id: "private-session-canary".into(),
                    token: "private-token-canary".into(),
                    user_id: "3".into(),
                },
                output: Output {
                    volume_linear: Some(0.0),
                    volume_percent: 0,
                    muted: true,
                },
            },
            false,
        )?;
        println!(
            "Native gateway/voice worker modes: protected configuration and private-frame rejection passed; all exits observed; no capture, gateway or voice connection started"
        );
        Ok(())
    })();
    let _ = std::fs::remove_file(credential);
    result
}

#[cfg(windows)]
fn rejection(
    executable: &std::path::Path,
    mode: &str,
    body: impl serde::Serialize,
    configuration: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use echocord_core::{
        process::EngineProcess,
        protocol::{self, Frame, Validator},
    };
    use std::{io::BufReader, time::Duration};
    let mut child = EngineProcess::spawn(executable, &[mode.into()])?;
    let mut input = child.take_stdin().ok_or("Missing private input pipe")?;
    let output = child.take_stdout().ok_or("Missing private output pipe")?;
    let mut diagnostics = child.take_stderr().ok_or("Missing diagnostic pipe")?;
    let reader = std::thread::spawn(move || -> Result<bool, protocol::ProtocolError> {
        let mut reader = BufReader::new(output);
        let mut validator = Validator::new(7)?;
        let mut rejected = false;
        while let Some(frame) = protocol::read::<serde_json::Value>(&mut reader)? {
            validator.accept(&frame)?;
            let text = serde_json::to_string(&frame.body)
                .map_err(|_| protocol::ProtocolError::Malformed)?;
            if [
                "private-token-canary",
                "private-session-canary",
                "private-endpoint-canary",
            ]
            .iter()
            .any(|canary| text.contains(canary))
            {
                return Err(protocol::ProtocolError::Invalid);
            }
            if frame.body.get("type").and_then(|v| v.as_str()) == Some("failed")
                && frame.body.get("code").and_then(|v| v.as_str()) == Some("configuration")
            {
                rejected = true;
            } else {
                return Err(protocol::ProtocolError::Invalid);
            }
        }
        Ok(rejected)
    });
    let drain = std::thread::spawn(move || std::io::copy(&mut diagnostics, &mut std::io::sink()));
    protocol::write(
        &mut input,
        &Frame {
            version: protocol::VERSION,
            generation: 7,
            sequence: 1,
            body,
        },
    )?;
    let exit = match child.wait_timeout(Duration::from_secs(5))? {
        Some(exit) => exit,
        None => {
            child.terminate()?;
            if child.wait_timeout(Duration::from_secs(3))?.is_none() {
                return Err("Worker exit could not be confirmed".into());
            }
            return Err("Worker did not reject invalid private initialization promptly".into());
        }
    };
    drop(input);
    let rejected = reader.join().map_err(|_| "Protocol reader failed")??;
    drain.join().map_err(|_| "Diagnostic drain failed")??;
    if exit == 0 || configuration && !rejected {
        return Err("Worker did not report the expected rejection".into());
    }
    Ok(())
}

#[cfg(windows)]
fn voice_state_check(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct PrivateConfig {
        token_file: std::path::PathBuf,
        server_id: String,
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Private probe configuration unavailable")?
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|_| "Private probe configuration unreadable")?;
    if bytes.len() > 8192 {
        return Err("Private probe configuration too large".into());
    }
    let config: PrivateConfig =
        serde_json::from_slice(&bytes).map_err(|_| "Private probe configuration invalid")?;
    let token = echocord_core::storage::load_token(&config.token_file)
        .map_err(|_| "Protected bot credential unavailable")?;
    let guild = echocord_core::protocol::discord_id(&config.server_id)
        .ok_or("Invalid private server selection")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let request = async {
            let http = serenity::http::Http::new(&token);
            let user = http.get_current_user().await.map_err(|_| "api-error")?;
            if !user.bot {
                return Err("api-error");
            }
            match http
                .get_user_voice_state(serenity::all::GuildId::new(guild), user.id)
                .await
            {
                Ok(state) if state.channel_id.is_some() => Ok("present"),
                Ok(_) => Ok("absent"),
                Err(serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(
                    response,
                ))) if response.error.code == 10065 => Ok("absent"),
                Err(_) => Err("api-error"),
            }
        };
        let status = tokio::time::timeout(std::time::Duration::from_secs(15), request)
            .await
            .unwrap_or(Err("api-error"));
        println!("own-bot-voice-state: {}", status.unwrap_or("api-error"));
        status
            .map(|_| ())
            .map_err(|_| "Read-only voice-state API check failed".into())
    })
}
