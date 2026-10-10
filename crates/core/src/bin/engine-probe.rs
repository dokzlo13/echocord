#[cfg(not(windows))]
fn main() {
    eprintln!("Run the engine probe as a Windows executable.");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use echocord_core::{
        model::Output,
        process::EngineProcess,
        protocol::{self, Command, Failure, Frame, Init, Status, Validator},
    };
    use std::{
        ffi::OsString,
        fs,
        io::BufReader,
        path::PathBuf,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    let mut arguments = std::env::args_os().skip(1);
    let executable = PathBuf::from(arguments.next().ok_or("Missing application executable")?);
    let evidence = PathBuf::from(
        arguments
            .next()
            .ok_or("Missing private evidence directory")?,
    );
    if arguments.next().is_some() {
        return Err("Unexpected argument".into());
    }
    fs::create_dir_all(&evidence)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let credential = evidence.join(format!("empty-probe-credential-{nonce}"));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&credential)?;

    let mut worker = EngineProcess::spawn(&executable, &[OsString::from("--engine-worker")])?;
    let mut stdin = worker.take_stdin().ok_or("Missing child stdin")?;
    let stdout = worker.take_stdout().ok_or("Missing child stdout")?;
    let mut stderr = worker.take_stderr().ok_or("Missing child stderr")?;
    let reader = thread::spawn(move || -> Result<bool, protocol::ProtocolError> {
        let mut input = BufReader::new(stdout);
        let mut validator = Validator::new(1)?;
        let mut configuration_rejected = false;
        for _ in 0..16 {
            let Some(frame) = protocol::read::<Status>(&mut input)? else {
                return Ok(configuration_rejected);
            };
            validator.accept(&frame)?;
            configuration_rejected |= matches!(
                frame.body,
                Status::Failed {
                    code: Failure::Configuration
                }
            );
            if matches!(frame.body, Status::Connected | Status::Meter { .. }) {
                return Err(protocol::ProtocolError::Invalid);
            }
        }
        Err(protocol::ProtocolError::Oversized)
    });
    let drain = thread::spawn(move || std::io::copy(&mut stderr, &mut std::io::sink()));
    protocol::write(
        &mut stdin,
        &Frame {
            version: protocol::VERSION,
            generation: 1,
            sequence: 1,
            body: Command::Init(Init {
                token_file: credential,
                source_id: "no-real-recording-device".into(),
                server_id: "1".into(),
                channel_id: "2".into(),
                output: Output {
                    volume_linear: None,
                    volume_percent: 0,
                    muted: true,
                },
            }),
        },
    )?;
    // Keep stdin open: configuration failure must end the worker independently.
    let exit = match worker.wait_timeout(Duration::from_secs(5))? {
        Some(code) => code,
        None => {
            worker.terminate()?;
            if worker.wait_timeout(Duration::from_secs(3))?.is_none() {
                return Err("Engine exit could not be confirmed".into());
            }
            return Err("Engine did not exit after rejecting empty configuration".into());
        }
    };
    drop(stdin);
    let rejected = reader.join().map_err(|_| "Protocol reader failed")??;
    drain.join().map_err(|_| "Diagnostic drain failed")??;
    if exit == 0 || !rejected {
        return Err("Engine did not report the expected configuration rejection".into());
    }
    println!(
        "Native GUI engine mode: protocol and configuration rejection passed; exit observed; no real capture or Discord configuration supplied"
    );
    Ok(())
}
