//! Disposable voice-only worker. No bot token, gateway client or source selection.
use super::protocol::{VoiceCommand, VoiceStatus, valid_key, valid_secret};
use crate::{
    audio,
    model::Output,
    protocol::{self, Failure, Frame, Validator},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    sync::{mpsc, watch},
};

struct DriverEvents(watch::Sender<bool>);
#[serenity::async_trait]
impl songbird::EventHandler for DriverEvents {
    async fn act(&self, event: &songbird::EventContext<'_>) -> Option<songbird::Event> {
        if matches!(event, songbird::EventContext::DriverDisconnect(_)) {
            self.0.send_replace(true);
        }
        None
    }
}

pub async fn run_worker() -> Result<(), Failure> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let frame = tokio::time::timeout(
        Duration::from_secs(10),
        protocol::read_async::<VoiceCommand>(&mut reader),
    )
    .await
    .map_err(|_| Failure::Protocol)?
    .map_err(|_| Failure::Protocol)?
    .ok_or(Failure::Protocol)?;
    let generation = frame.generation;
    let mut validator = Validator::new(generation).map_err(|_| Failure::Protocol)?;
    validator.accept(&frame).map_err(|_| Failure::Protocol)?;
    let VoiceCommand::Init {
        key,
        gateway_epoch,
        credential_revision,
        source_id,
        secret,
        output,
    } = frame.body
    else {
        return Err(Failure::Protocol);
    };
    if !valid_output(&output)
        || !valid_key(&key)
        || !valid_secret(&secret)
        || gateway_epoch == 0
        || credential_revision == 0
        || !source_id.starts_with("mix-handle:")
    {
        return Err(Failure::Protocol);
    }
    let (outputs, mut output_rx) = watch::channel(output);
    let (stops, mut stop_rx) = watch::channel(false);
    let input_stop = stops.clone();
    tokio::spawn(async move {
        while let Ok(Some(frame)) = protocol::read_async::<VoiceCommand>(&mut reader).await {
            if validator.accept(&frame).is_err() {
                break;
            }
            match frame.body {
                VoiceCommand::Output { output } if valid_output(&output) => {
                    outputs.send_replace(output);
                }
                _ => break,
            }
        }
        input_stop.send_replace(true);
    });
    let (statuses, mut status_rx) = mpsc::channel::<VoiceStatus>(16);
    let writer_stop = stops.clone();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        let mut sequence = 0;
        while let Some(body) = status_rx.recv().await {
            sequence += 1;
            let Ok(bytes) = protocol::encode(&Frame {
                version: protocol::VERSION,
                generation,
                sequence,
                body,
            }) else {
                writer_stop.send_replace(true);
                break;
            };
            if stdout.write_all(&bytes).await.is_err() || stdout.flush().await.is_err() {
                writer_stop.send_replace(true);
                break;
            }
        }
    });
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_capture = cancelled.clone();
    let mut capture = None;
    let mut driver = songbird::Driver::new(
        songbird::Config::default()
            .driver_timeout(Some(Duration::from_secs(5)))
            .driver_retry(songbird::driver::retry::Retry {
                strategy: songbird::driver::retry::Strategy::Every(Duration::from_secs(1).into()),
                retry_limit: std::num::NonZeroU8::new(3),
            }),
    );
    driver.add_global_event(
        songbird::Event::Core(songbird::CoreEvent::DriverDisconnect),
        DriverEvents(stops.clone()),
    );
    let info = songbird::ConnectionInfo {
        channel_id: serenity::all::ChannelId::new(
            protocol::discord_id(&key.channel_id).ok_or(Failure::Protocol)?,
        )
        .into(),
        guild_id: serenity::all::GuildId::new(
            protocol::discord_id(&key.server_id).ok_or(Failure::Protocol)?,
        )
        .into(),
        user_id: serenity::all::UserId::new(
            protocol::discord_id(&secret.user_id).ok_or(Failure::Protocol)?,
        )
        .into(),
        endpoint: secret.endpoint,
        session_id: secret.session_id,
        token: secret.token,
    };
    let connecting = driver.connect(info);
    let mut result = tokio::select! { biased; _=stop_rx.changed()=>Ok(()), joined=tokio::time::timeout(Duration::from_secs(25),connecting)=>joined.map_err(|_|Failure::Connection).and_then(|joined|joined.map_err(|_|Failure::Connection)) };
    if result.is_ok() && !*stop_rx.borrow() {
        let source = source_id.clone();
        let opening = tokio::task::spawn_blocking(move || {
            audio::start_capture_with_cancel(&source, &|| cancel_capture.load(Ordering::Acquire))
        });
        match tokio::select! { biased; _=stop_rx.changed()=>None, result=opening=>Some(result) } {
            Some(Ok(Ok(session))) if !*stop_rx.borrow() => {
                let output = output_rx.borrow_and_update().clone();
                session.control.set_linear_output(
                    output
                        .volume_linear
                        .unwrap_or(output.volume_percent as f32 / 100.0),
                    output.muted,
                );
                driver.play_only_input(
                    songbird::input::RawAdapter::new(session.input, 48000, 2).into(),
                );
                capture = Some(session.control);
                let _ = statuses.send(VoiceStatus::Connected).await;
            }
            Some(Ok(Ok(session))) => {
                session.control.stop();
            }
            Some(_) => result = Err(Failure::Capture),
            None => {}
        }
    }
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    while result.is_ok() && capture.is_some() && !*stop_rx.borrow() {
        tokio::select! { biased;
            _=stop_rx.changed()=>break,
            changed=output_rx.changed()=>{if changed.is_err(){break;} let output=output_rx.borrow_and_update().clone();if let Some(capture)=&capture {capture.set_linear_output(output.volume_linear.unwrap_or(output.volume_percent as f32/100.0),output.muted);}},
            _=tick.tick()=>{if let Some(capture)=&capture {let _=statuses.try_send(VoiceStatus::Meter {capture:protocol::clean_capture(capture.snapshot())});}}
        }
    }
    cancelled.store(true, Ordering::Release);
    driver.stop();
    driver.leave();
    if let Some(capture) = capture {
        capture.stop();
    }
    let _ = statuses
        .send(match result {
            Ok(()) => VoiceStatus::Stopped,
            Err(code) => VoiceStatus::Failed { code },
        })
        .await;
    drop(statuses);
    let _ = tokio::time::timeout(Duration::from_millis(100), writer).await;
    result
}
fn valid_output(output: &Output) -> bool {
    output.volume_percent <= 100
        && output
            .volume_linear
            .is_none_or(|gain| gain.is_finite() && (0.0..=1.0).contains(&gain))
}
