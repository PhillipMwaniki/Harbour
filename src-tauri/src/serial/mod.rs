//! A serial console, as one more kind of session.
//!
//! A serial port is a byte pipe with no framing and no negotiation - simpler
//! than telnet - but the `serialport` crate is synchronous, so unlike the other
//! transports the reader runs on a dedicated OS thread rather than a tokio task.
//! It hands its bytes to the same `Receiver<Vec<u8>>` a pty produces, so the
//! pump, the ack budget and the terminal do not know the difference.
//!
//! There is no window size on a serial line, so `resize` does nothing; the
//! terminal still has one, for its own wrapping.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use serialport::{SerialPort, SerialPortType};
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult};
use crate::session::{ExitReason, Transport};

/// How often the reader wakes to check whether it has been asked to stop. Also
/// the read timeout, so a quiet line does not block the thread from noticing a
/// close.
const POLL: Duration = Duration::from_millis(50);

/// Matches the other transports' output queue depth.
const READ_QUEUE_DEPTH: usize = 256;

/// One serial port the machine has, for the connect dialog to offer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    /// The path or name to open - `COM3`, `/dev/ttyUSB0`.
    pub path: String,
    /// A short human description: "USB", "Bluetooth", "PCI", or "Unknown".
    pub kind: String,
    /// For a USB device, the product string if it advertised one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
}

/// Lists the serial ports currently attached. Blocking; call it off the async
/// runtime.
pub fn ports() -> AppResult<Vec<PortInfo>> {
    let found = serialport::available_ports().map_err(|err| AppError::Serial(err.to_string()))?;
    Ok(found
        .into_iter()
        .map(|port| {
            let (kind, product) = match port.port_type {
                SerialPortType::UsbPort(info) => ("USB", info.product),
                SerialPortType::BluetoothPort => ("Bluetooth", None),
                SerialPortType::PciPort => ("PCI", None),
                SerialPortType::Unknown => ("Unknown", None),
            };
            PortInfo {
                path: port.port_name,
                kind: kind.to_string(),
                product,
            }
        })
        .collect())
}

pub struct SerialTransport {
    /// The port, behind a lock so `write` and the reader thread share it. The
    /// reader holds a clone, so this is only ever used for writing.
    port: Arc<Mutex<Box<dyn SerialPort>>>,
    /// Set by `kill`; the reader thread checks it each poll and exits.
    stop: Arc<AtomicBool>,
}

impl Transport for SerialTransport {
    fn write(&self, data: &[u8]) -> AppResult<()> {
        let mut port = self.port.lock();
        port.write_all(data)
            .map_err(|err| AppError::Serial(err.to_string()))?;
        port.flush()
            .map_err(|err| AppError::Serial(err.to_string()))
    }

    fn resize(&self, _cols: u16, _rows: u16) -> AppResult<()> {
        // A serial line has no window size.
        Ok(())
    }

    fn kill(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// A serial connection turned into a session's two halves.
pub struct Connected {
    pub transport: SerialTransport,
    pub output: mpsc::Receiver<Vec<u8>>,
}

/// Opens `path` at `baud` and starts reading it. Blocking (the open is), so
/// call it off the async runtime. `on_exit` fires once, when the line closes or
/// the session is killed.
pub fn open<F>(path: &str, baud: u32, on_exit: F) -> AppResult<Connected>
where
    F: FnOnce(ExitReason, Option<u32>) + Send + 'static,
{
    let port = serialport::new(path, baud)
        .timeout(POLL)
        .open()
        .map_err(|err| AppError::Serial(format!("could not open {path}: {err}")))?;
    // A second handle on the same port: one for the reader thread, one for
    // writing. Without a clone the port could not be both read and written.
    let mut reader = port
        .try_clone()
        .map_err(|err| AppError::Serial(format!("could not open {path} for reading: {err}")))?;

    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(READ_QUEUE_DEPTH);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = Arc::clone(&stop);

    std::thread::Builder::new()
        .name(format!("serial:{path}"))
        .spawn(move || {
            let mut buf = [0u8; 4096];
            let reason = loop {
                if stop_reader.load(Ordering::Acquire) {
                    break ExitReason::Killed;
                }
                match reader.read(&mut buf) {
                    Ok(0) => continue,
                    Ok(n) => {
                        // `blocking_send` because this is a plain OS thread, not
                        // a tokio task; it applies the same backpressure.
                        if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break ExitReason::Killed; // the session was dropped
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::TimedOut => continue,
                    Err(_) => break ExitReason::Lost, // the device went away
                }
            };
            on_exit(reason, None);
        })
        .map_err(|err| AppError::Serial(format!("could not start the serial reader: {err}")))?;

    Ok(Connected {
        transport: SerialTransport {
            port: Arc::new(Mutex::new(port)),
            stop,
        },
        output: out_rx,
    })
}
