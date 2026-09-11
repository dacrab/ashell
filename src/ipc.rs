//! IPC via Unix domain socket.
//!
//! The daemon listens on `$XDG_RUNTIME_DIR/ashell.sock`.
//! The same binary acts as a client via `ashell msg <command>`.

use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::str::FromStr;

use crate::xdg;
use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use iced::Subscription;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use iced::futures::StreamExt;

/// Maximum bytes to read from a client connection.
const MAX_REQUEST_LEN: usize = 4096;

/// IPC command that can be sent to the daemon.
#[derive(Subcommand, Debug, Clone)]
pub enum IpcCommand {
    /// Toggle bar visibility
    ToggleVisibility,
    VolumeUp {
        #[arg(long)]
        no_osd: bool,
    },
    VolumeDown {
        #[arg(long)]
        no_osd: bool,
    },
    VolumeToggleMute {
        #[arg(long)]
        no_osd: bool,
    },
    MicrophoneUp {
        #[arg(long)]
        no_osd: bool,
    },
    MicrophoneDown {
        #[arg(long)]
        no_osd: bool,
    },
    MicrophoneToggleMute {
        #[arg(long)]
        no_osd: bool,
    },
    BrightnessUp {
        #[arg(long)]
        no_osd: bool,
    },
    BrightnessDown {
        #[arg(long)]
        no_osd: bool,
    },
    ToggleAirplaneMode {
        #[arg(long)]
        no_osd: bool,
    },
    ToggleIdleInhibitor {
        #[arg(long)]
        no_osd: bool,
    },
}

impl IpcCommand {
    pub fn no_osd(&self) -> bool {
        self.wire().1.unwrap_or(false)
    }
}

const NO_OSD_SUFFIX: &str = "?no-osd";

impl IpcCommand {
    /// Single source of truth for the wire name of this command, plus whether
    /// it carries a `no_osd` flag. Consumed by `Display`, `FromStr` and
    /// `no_osd` so the name only exists in one place.
    fn wire(&self) -> (&'static str, Option<bool>) {
        match self {
            IpcCommand::ToggleVisibility => ("toggle-visibility", None),
            IpcCommand::VolumeUp { no_osd } => ("volume-up", Some(*no_osd)),
            IpcCommand::VolumeDown { no_osd } => ("volume-down", Some(*no_osd)),
            IpcCommand::VolumeToggleMute { no_osd } => ("volume-toggle-mute", Some(*no_osd)),
            IpcCommand::MicrophoneUp { no_osd } => ("microphone-up", Some(*no_osd)),
            IpcCommand::MicrophoneDown { no_osd } => ("microphone-down", Some(*no_osd)),
            IpcCommand::MicrophoneToggleMute { no_osd } => {
                ("microphone-toggle-mute", Some(*no_osd))
            }
            IpcCommand::BrightnessUp { no_osd } => ("brightness-up", Some(*no_osd)),
            IpcCommand::BrightnessDown { no_osd } => ("brightness-down", Some(*no_osd)),
            IpcCommand::ToggleAirplaneMode { no_osd } => ("toggle-airplane-mode", Some(*no_osd)),
            IpcCommand::ToggleIdleInhibitor { no_osd } => ("toggle-idle-inhibitor", Some(*no_osd)),
        }
    }

    /// Parse a base wire name (without the `?no-osd` suffix).
    fn from_wire(name: &str, no_osd: bool) -> Option<Self> {
        Some(match name {
            "toggle-visibility" => IpcCommand::ToggleVisibility,
            "volume-up" => IpcCommand::VolumeUp { no_osd },
            "volume-down" => IpcCommand::VolumeDown { no_osd },
            "volume-toggle-mute" => IpcCommand::VolumeToggleMute { no_osd },
            "microphone-up" => IpcCommand::MicrophoneUp { no_osd },
            "microphone-down" => IpcCommand::MicrophoneDown { no_osd },
            "microphone-toggle-mute" => IpcCommand::MicrophoneToggleMute { no_osd },
            "brightness-up" => IpcCommand::BrightnessUp { no_osd },
            "brightness-down" => IpcCommand::BrightnessDown { no_osd },
            "toggle-airplane-mode" => IpcCommand::ToggleAirplaneMode { no_osd },
            "toggle-idle-inhibitor" => IpcCommand::ToggleIdleInhibitor { no_osd },
            _ => return None,
        })
    }
}

impl fmt::Display for IpcCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.wire().0)?;
        if self.no_osd() {
            write!(f, "{NO_OSD_SUFFIX}")?;
        }
        Ok(())
    }
}

impl FromStr for IpcCommand {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let (cmd, no_osd) = match s.strip_suffix(NO_OSD_SUFFIX) {
            Some(base) => (base, true),
            None => (s, false),
        };
        Self::from_wire(cmd, no_osd).ok_or_else(|| anyhow!("unknown IPC command: {s:?}"))
    }
}

pub fn socket_path() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    match xdg::runtime_dir() {
        Some(dir) => [dir, PathBuf::from("ashell.sock")],
        None => [
            std::env::temp_dir(),
            PathBuf::from(format!("ashell-{uid}.sock")),
        ],
    }
    .iter()
    .collect()
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Run the IPC client: connect to the daemon, send a command, print the response.
pub fn run_client(cmd: &IpcCommand) -> Result<()> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("connect to {} — is ashell running?", path.display()))?;

    let line = format!("{cmd}\n");
    stream.write_all(line.as_bytes()).context("send command")?;
    stream.flush()?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut response = String::new();
    BufReader::new((&stream).take(MAX_REQUEST_LEN as u64))
        .read_line(&mut response)
        .context("read response")?;
    let response = response.trim_end();

    if let Some(err) = response.strip_prefix("error ") {
        return Err(anyhow!("{err}"));
    }

    if !response.is_empty() {
        println!("{response}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

enum ListenerError {
    /// Another ashell instance is already listening on the socket.
    AlreadyRunning,
    Other(anyhow::Error),
}

/// Create the Unix listener, taking care not to steal a live server's socket.
///
/// The socket path is shared across instances, so we probe it first: a
/// successful connect means a primary is already serving and we must not
/// remove the file or bind a new listener — otherwise we'd orphan the
/// primary's fd and break `ashell msg` until it's restarted.
fn create_listener() -> std::result::Result<UnixListener, ListenerError> {
    let path = socket_path();

    match UnixStream::connect(&path) {
        Ok(_) => return Err(ListenerError::AlreadyRunning),
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            if let Err(e) = std::fs::remove_file(&path)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                return Err(ListenerError::Other(
                    anyhow::Error::new(e)
                        .context(format!("remove stale socket {}", path.display())),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(ListenerError::Other(
                anyhow::Error::new(e).context(format!("probe socket {}", path.display())),
            ));
        }
    }

    let listener = UnixListener::bind(&path)
        .with_context(|| format!("bind {}", path.display()))
        .map_err(ListenerError::Other)?;
    listener
        .set_nonblocking(true)
        .context("set_nonblocking")
        .map_err(ListenerError::Other)?;
    log::info!("IPC listening on {}", path.display());
    Ok(listener)
}

/// Read a single command from an accepted client connection.
async fn read_request(stream: &mut tokio::net::UnixStream) -> Result<IpcCommand> {
    // Read up to MAX_REQUEST_LEN + 1 bytes: one extra to detect oversized
    // requests while bounding memory.
    let mut buf = vec![0; MAX_REQUEST_LEN + 1];
    let mut total = 0;
    let mut newline = None;
    while newline.is_none() && total <= MAX_REQUEST_LEN {
        let n = stream
            .read(&mut buf[total..])
            .await
            .context("read IPC command")?;
        if n == 0 {
            break;
        }
        if let Some(pos) = buf[total..total + n].iter().position(|&b| b == b'\n') {
            newline = Some(total + pos);
        }
        total += n;
    }
    let end = match newline {
        Some(pos) => pos,
        None if total > MAX_REQUEST_LEN => {
            anyhow::bail!("request exceeds {} bytes", MAX_REQUEST_LEN);
        }
        None => total,
    };
    let line = String::from_utf8_lossy(&buf[..end]);
    line.trim().parse()
}

/// Write a response line to the client.
async fn write_response(stream: &mut tokio::net::UnixStream, response: &str) {
    let msg = format!("{response}\n");
    if let Err(e) = stream.write_all(msg.as_bytes()).await {
        log::debug!("IPC write response failed: {e}");
    }
}

/// Handle a single accepted client connection.
async fn handle_connection(mut stream: tokio::net::UnixStream) -> Option<IpcCommand> {
    match read_request(&mut stream).await {
        Ok(cmd) => {
            write_response(&mut stream, "ok").await;
            Some(cmd)
        }
        Err(e) => {
            write_response(&mut stream, &format!("error {e:#}")).await;
            None
        }
    }
}

fn init_listener() -> Option<tokio::net::UnixListener> {
    let std_listener = match create_listener() {
        Ok(l) => l,
        Err(ListenerError::AlreadyRunning) => {
            log::warn!(
                "another ashell instance owns the IPC socket; this instance will run without IPC"
            );
            return None;
        }
        Err(ListenerError::Other(e)) => {
            log::error!("Failed to create IPC listener: {e:#}");
            return None;
        }
    };
    match tokio::net::UnixListener::from_std(std_listener) {
        Ok(l) => Some(l),
        Err(e) => {
            log::error!("Failed to convert IPC listener to tokio: {e}");
            None
        }
    }
}

/// Subscription that listens for IPC commands on the Unix socket.
pub fn subscription() -> Subscription<IpcCommand> {
    Subscription::run(|| {
        iced::futures::stream::unfold(None::<tokio::net::UnixListener>, |listener| async {
            let listener = match listener {
                Some(l) => l,
                None => init_listener()?,
            };
            let (request, listener) = match listener.accept().await {
                Ok((stream, _)) => (handle_connection(stream).await, listener),
                Err(e) => {
                    log::error!("IPC accept error: {e}");
                    (None, listener)
                }
            };
            Some((request, Some(listener)))
        })
        .filter_map(iced::futures::future::ready)
    })
}
