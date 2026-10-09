use cpal::traits::{DeviceTrait, HostTrait};
use songbird::input::{Input, RawAdapter, codecs};
use std::{error::Error, io::Cursor, num::NonZeroU16};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let list_devices = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--list-devices") => true,
        Some(_) => return Err("Usage: echocord-audio-dependency-probe [--list-devices]".into()),
    };
    let host = cpal::default_host();
    let devices: Vec<_> = host.input_devices()?.collect();
    println!("Recording device enumeration: {} endpoints", devices.len());
    if devices.is_empty() {
        return Err("No recording endpoints available".into());
    }
    if list_devices {
        // Endpoint descriptions are private diagnostic output, never repository evidence.
        for device in &devices {
            println!(
                "{:?}: {:?}",
                device.description()?,
                device.default_input_config()
            );
        }
    }

    let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(1920);
    producer.push(0.25).map_err(|_| "Ring buffer push failed")?;
    assert_eq!(consumer.pop()?, 0.25);
    println!("Bounded sample transfer: passed");

    let samples: Vec<f32> = (0..960)
        .flat_map(|frame| {
            let sample = (frame as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.1;
            [sample, sample]
        })
        .collect();
    let mut encoder =
        opus2::Encoder::new(48_000, opus2::Channels::Stereo, opus2::Application::Audio)?;
    let mut packet = [0_u8; 4000];
    let encoded = encoder.encode_float(&samples, &mut packet)?;
    if encoded == 0 {
        return Err("Opus produced an empty packet".into());
    }
    println!("Native Opus encode: passed ({encoded} bytes)");
    let mut decoder = opus2::Decoder::new(48_000, opus2::Channels::Stereo)?;
    let mut decoded = vec![0.0; samples.len()];
    if decoder.decode_float(&packet[..encoded], &mut decoded, false)? != 960
        || !decoded.iter().all(|sample| sample.is_finite())
        || !decoded.iter().any(|sample| sample.abs() > 0.001)
    {
        return Err("Opus round trip did not return the expected audio frame".into());
    }
    println!("Native Opus decode: passed (960 stereo frames)");

    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let input: Input = RawAdapter::new(Cursor::new(pcm), 48_000, 2).into();
    let _playable = input
        .make_playable_async(codecs::get_codec_registry(), codecs::get_probe())
        .await?;
    println!("Songbird raw PCM preparation: passed");

    // Synthetic IDs, used only for offline crypto initialization.
    let mut dave = davey::DaveSession::new(
        NonZeroU16::new(davey::DAVE_PROTOCOL_VERSION).ok_or("Invalid DAVE protocol version")?,
        1,
        2,
        None,
    )?;
    if dave.create_key_package()?.is_empty() {
        return Err("DAVE key package was empty".into());
    }
    println!("DAVE offline key package: passed");

    let _manager = songbird::Songbird::serenity();
    let _driver = songbird::Driver::new(songbird::Config::default());
    println!("Songbird driver and Serenity manager: constructed offline");
    println!("No capture stream or Discord connection was opened");
    Ok(())
}
