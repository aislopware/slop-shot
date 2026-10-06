use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Start resident with the tray icon, doing nothing else.
    Run,
    CaptureArea,
    CaptureFullscreen,
    CaptureScrolling,
    CaptureText,
    PickColor,
    /// Starts a recording, or stops the running one.
    RecordArea,
    History,
    Edit(PathBuf),
    EditLast,
    OpenLast,
    About,
    Settings,
    Quit,
}

impl Command {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let arg = |i: usize| args.get(i).map(String::as_str);
        Ok(match arg(0) {
            None => Command::Run,
            Some("area") => Command::CaptureArea,
            Some("full" | "fullscreen") => Command::CaptureFullscreen,
            Some("scroll") => Command::CaptureScrolling,
            Some("text") => Command::CaptureText,
            Some("color") => Command::PickColor,
            Some("record") => Command::RecordArea,
            Some("history") => Command::History,
            Some("settings") => Command::Settings,
            Some("edit-last") => Command::EditLast,
            Some("open-last") => Command::OpenLast,
            Some("about") => Command::About,
            Some("quit") => Command::Quit,
            Some("edit") => {
                let path = arg(1).ok_or("edit needs an image or video path")?;
                let path = std::fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
                Command::Edit(path)
            }
            Some(other) => return Err(format!("unknown command: {other}")),
        })
    }

    fn encode(&self) -> String {
        match self {
            Command::Run => "run".into(),
            Command::CaptureArea => "area".into(),
            Command::CaptureFullscreen => "full".into(),
            Command::CaptureScrolling => "scroll".into(),
            Command::CaptureText => "text".into(),
            Command::PickColor => "color".into(),
            Command::RecordArea => "record".into(),
            Command::History => "history".into(),
            Command::Settings => "settings".into(),
            Command::EditLast => "edit-last".into(),
            Command::OpenLast => "open-last".into(),
            Command::About => "about".into(),
            Command::Quit => "quit".into(),
            Command::Edit(path) => format!("edit\t{}", path.display()),
        }
    }

    fn decode(line: &str) -> Option<Self> {
        let args: Vec<String> = line.split('\t').map(str::to_owned).collect();
        Self::parse(&args).ok()
    }
}

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join("slopshot.sock")
}

pub enum Instance {
    /// Another process owns the socket and has received the command.
    Forwarded,
    Primary(UnixListener),
}

/// Becomes the primary instance, or hands `command` to the one already running.
pub fn claim(command: &Command) -> std::io::Result<Instance> {
    let path = socket_path();
    match UnixStream::connect(&path) {
        Ok(mut stream) => {
            writeln!(stream, "{}", command.encode())?;
            return Ok(Instance::Forwarded);
        }
        // A socket file without a listener is left over from a crash.
        Err(err) if err.kind() == ErrorKind::ConnectionRefused => {
            std::fs::remove_file(&path)?;
        }
        Err(_) => {}
    }
    UnixListener::bind(&path).map(Instance::Primary)
}

/// Forwards commands from later `slopshot …` invocations into `tx` until the app exits.
pub fn serve(listener: UnixListener, tx: async_channel::Sender<Command>) {
    std::thread::Builder::new()
        .name("slopshot-ipc".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                if BufReader::new(stream).read_line(&mut line).is_ok()
                    && let Some(command) = Command::decode(line.trim_end())
                    && tx.send_blocking(command).is_err()
                {
                    break;
                }
            }
        })
        .expect("spawn ipc thread");
}

pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}
