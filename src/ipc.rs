//! Control plane (proposal §7.4): a Unix-socket command channel between
//! the daemon and client invocations of this binary.
//!
//! One binary, two roles (workmeshd pattern, engineering guideline
//! "CLI-first & control plane"): `--daemon`/`--start` starts the daemon
//! and binds the socket — a second daemon exits with an explanatory
//! error (single-instance guard); any other invocation is a client
//! sending one command (default `show`) over the socket.
//!
//! The server runs on the GTK main context via gio `SocketService` +
//! async futures: no worker threads, the UI thread never blocks on
//! socket I/O, and the dispatch closure may touch widgets directly.
//!
//! Transport note: the protocol is line-based and transport-agnostic —
//! v1 is a Unix socket; later transports (HTTP/TCP/UDP, hosted by
//! workmeshd) reuse the same command contract and also make the control
//! plane portable to Windows, which has no Unix sockets.

use std::io::{BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gio::prelude::*;
use gtk::glib;

/// Socket file name inside `$XDG_RUNTIME_DIR` (proposal §7.4; the path
/// becomes TOML-configurable at M5).
const SOCKET_FILE: &str = "hoverclock.sock";

/// Client retry budget before reporting the daemon unreachable
/// (workmeshd-style bounded retries, proposal §7.4).
const CLIENT_RETRIES: u32 = 3;
const CLIENT_RETRY_DELAY: Duration = Duration::from_millis(100);
/// Read guard so a wedged daemon cannot hang the client forever.
const CLIENT_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Control-plane commands (proposal §7.4 baseline; the M6 `Command`
/// registry grows this set). `show` is the default client action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Show,
    Hide,
    Toggle,
}

impl Command {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Hide => "hide",
            Self::Toggle => "toggle",
        }
    }

    /// Parse the first whitespace-separated token of a request line.
    /// Unknown commands produce a deterministic error — never a crash.
    pub fn parse(line: &str) -> Result<Self, String> {
        match line.split_whitespace().next().unwrap_or("") {
            "show" => Ok(Self::Show),
            "hide" => Ok(Self::Hide),
            "toggle" => Ok(Self::Toggle),
            "" => Err("empty command".into()),
            other => Err(format!("unknown command '{other}'")),
        }
    }
}

/// Control socket path: `${XDG_RUNTIME_DIR}/hoverclock.sock`, falling
/// back to the system temp dir when the runtime dir is unset.
pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join(SOCKET_FILE)
}

/// Bind the daemon control socket with the single-instance guard
/// (proposal §7.4). A live listener means another daemon is running —
/// refuse with an explanatory error instead of coexisting silently; a
/// stale socket from a crashed daemon is reclaimed.
pub fn bind_daemon() -> Result<gio::SocketService, String> {
    bind_daemon_at(&socket_path())
}

fn bind_daemon_at(path: &Path) -> Result<gio::SocketService, String> {
    let service = gio::SocketService::new();
    let bind = || {
        service.add_address(
            &gio::UnixSocketAddress::new(path),
            gio::SocketType::Stream,
            gio::SocketProtocol::Default,
            None::<&glib::Object>,
        )
    };
    match bind() {
        Ok(_) => Ok(service),
        Err(bind_err) => {
            // Could not bind. A live daemon is detectable by connecting —
            // that is the single-instance check.
            if UnixStream::connect(path).is_ok() {
                return Err(format!(
                    "another hover-clock daemon is already running (control socket {}). \
                     The daemon is a single-instance service — stop the existing one first, \
                     or control it with `hover-clock <command>`.",
                    path.display()
                ));
            }
            if path.exists() {
                // Stale socket from a crashed daemon: reclaim and retry.
                std::fs::remove_file(path).map_err(|err| {
                    format!(
                        "cannot reclaim stale control socket {}: {err}",
                        path.display()
                    )
                })?;
                bind().map_err(|err| {
                    format!("cannot bind control socket {}: {err}", path.display())
                })?;
            } else {
                return Err(format!(
                    "cannot bind control socket {}: {bind_err}",
                    path.display()
                ));
            }
            Ok(service)
        }
    }
}

/// Handle returned by [`install`] — holds the control service alive for
/// the daemon's lifetime (the caller keeps it, e.g. as application data).
pub struct ControlPlane {
    _service: gio::SocketService,
}

/// Serve the control plane on the GTK main context.
///
/// `dispatch` runs on the main loop (it may touch widgets). Each client
/// connection is served by a spawned async task using gio futures, so
/// the UI thread never blocks on socket I/O.
pub fn install<F>(service: gio::SocketService, dispatch: F) -> Result<ControlPlane, String>
where
    F: Fn(&str) -> String + 'static,
{
    // The thread-default context: the GTK main loop during activate, or a
    // test-owned context. `MainContext::default()` would be the global
    // context, which may never be iterated.
    let context = glib::MainContext::ref_thread_default();
    let dispatch = Rc::new(dispatch);
    service.connect_incoming(move |_service, connection, _source_object| {
        let dispatch = Rc::clone(&dispatch);
        // Owned ref (refcount bump) so the async task outlives the
        // signal callback.
        let connection = connection.clone();
        context.spawn_local(async move {
            let input = gio::DataInputStream::new(&connection.input_stream());
            let output = gio::DataOutputStream::new(&connection.output_stream());
            // EOF (client closed) or read error ends the connection.
            while let Ok(Some(line)) = input.read_line_utf8_future(glib::Priority::DEFAULT).await {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let response = format!("{}\n", dispatch(line));
                if output
                    .write_all_future(response, glib::Priority::DEFAULT)
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        false
    });
    Ok(ControlPlane { _service: service })
}

/// Client: connect to the running daemon, send one command, return the
/// response. Bounded retries (workmeshd pattern), then a deterministic
/// error that says how to start the daemon.
pub fn request(command: Command) -> Result<String, String> {
    let path = socket_path();
    let mut last_error = None;
    for _ in 0..CLIENT_RETRIES {
        match UnixStream::connect(&path) {
            Ok(mut stream) => {
                let _ = stream.set_read_timeout(Some(CLIENT_READ_TIMEOUT));
                let request = format!("{}\n", command.as_str());
                stream
                    .write_all(request.as_bytes())
                    .map_err(|err| format!("cannot send command to the daemon: {err}"))?;
                let _ = stream.shutdown(std::net::Shutdown::Write);
                let mut response = String::new();
                let mut reader = BufReader::new(&mut stream);
                let _ = reader.read_to_string(&mut response);
                return Ok(response.trim().to_string());
            }
            Err(err) => {
                last_error = Some(err);
                std::thread::sleep(CLIENT_RETRY_DELAY);
            }
        }
    }
    Err(format!(
        "cannot reach the hover-clock daemon at {} ({})",
        path.display(),
        last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "unknown error".into())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hover-clock-ipc-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_commands() {
        assert_eq!(Command::parse("show"), Ok(Command::Show));
        assert_eq!(Command::parse("  hide \n"), Ok(Command::Hide));
        assert_eq!(Command::parse("toggle extra args"), Ok(Command::Toggle));
        assert!(Command::parse("ping").is_err());
        assert!(Command::parse("").is_err());
        assert!(Command::parse("   ").is_err());
    }

    #[test]
    fn single_instance_guard_refuses_a_live_daemon() {
        let dir = temp_dir("live");
        let path = dir.join("hoverclock.sock");
        let _first = bind_daemon_at(&path).expect("first daemon binds");
        let second = bind_daemon_at(&path).expect_err("second daemon must be refused");
        assert!(second.contains("already running"), "got: {second}");
        std::fs::remove_file(&path).ok();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn single_instance_guard_reclaims_a_stale_socket() {
        let dir = temp_dir("stale");
        let path = dir.join("hoverclock.sock");
        // Simulate a crashed daemon: socket file exists, nothing listening.
        std::fs::write(&path, b"stale").unwrap();
        let service = bind_daemon_at(&path).expect("stale socket is reclaimed");
        assert!(path.exists(), "reclaimed socket is listening");
        drop(service);
        std::fs::remove_file(&path).ok();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn control_plane_round_trip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("hoverclock.sock");
        // Serve on the default context, exactly like the real daemon on
        // the GTK main loop: bind + install on this thread, then iterate.
        let service = bind_daemon_at(&path).expect("binds");
        install(service, |line| format!("echo:{line}")).unwrap();

        // Client on a background thread (blocking std I/O; a read timeout
        // turns a broken server into a failure instead of a hang).
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let path_for_client = path.clone();
        let client = std::thread::spawn(move || {
            let mut stream = UnixStream::connect(&path_for_client).expect("connects");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream.write_all(b"show\n").unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let mut response = String::new();
            BufReader::new(&mut stream)
                .read_to_string(&mut response)
                .unwrap();
            let _ = result_tx.send(response.trim().to_string());
        });

        // Iterate the default context until the client round-trip lands.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let _ = glib::MainContext::default().iteration(false);
            if let Ok(response) = result_rx.try_recv() {
                assert_eq!(response, "echo:show");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "control-plane round trip timed out"
            );
        }
        client.join().unwrap();
        std::fs::remove_file(&path).ok();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
