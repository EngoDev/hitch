use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::config::Config;
use crate::detect::{DetectionEvent, UrlDetector};
use crate::origin::resolve_origin;
use crate::status;
use crate::tunnel::{SshTunnelLauncher, TunnelHandle, TunnelLauncher, format_destination};

#[cfg(test)]
pub struct ScriptedSession {
    pub output_chunks: Vec<String>,
    pub exit_code: i32,
}

#[cfg(test)]
pub struct RunOutcome {
    pub exit_code: i32,
    pub mirrored_output: String,
    pub messages: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InputGate {
    paused: Arc<AtomicBool>,
}

impl InputGate {
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

pub struct PausingTunnelLauncher<L> {
    inner: L,
    input_gate: InputGate,
}

impl<L> PausingTunnelLauncher<L> {
    pub fn new(inner: L, input_gate: InputGate) -> Self {
        Self { inner, input_gate }
    }
}

impl<L: TunnelLauncher> TunnelLauncher for PausingTunnelLauncher<L> {
    type Handle = L::Handle;

    fn launch(&mut self, destination: &str, port: u16) -> io::Result<Self::Handle> {
        self.input_gate.pause();
        let result = self.inner.launch(destination, port);
        self.input_gate.resume();
        result
    }
}

pub struct Runtime<L: TunnelLauncher> {
    detector: UrlDetector,
    launcher: L,
    origin: Option<String>,
    user: Option<String>,
    messages: Vec<String>,
    tunnel_handle: Option<L::Handle>,
    reported_missing_origin: bool,
}

impl<L: TunnelLauncher> Runtime<L> {
    pub fn new(launcher: L, origin: Option<String>, user: Option<String>) -> Self {
        Self {
            detector: UrlDetector::default(),
            launcher,
            origin,
            user,
            messages: Vec::new(),
            tunnel_handle: None,
            reported_missing_origin: false,
        }
    }

    pub fn on_output(&mut self, text: &str) {
        match self.detector.consume(text) {
            Some(DetectionEvent::StartTunnel { host, port, url }) => {
                self.messages
                    .push(status::found_loopback_url(&url, &host, port));
                self.try_start_tunnel(port);
            }
            Some(DetectionEvent::WarnNonLoopback { host, url }) => {
                self.messages
                    .push(status::non_loopback_redirect(&url, &host));
            }
            Some(DetectionEvent::WarnMissingPort { host, url }) => {
                self.messages
                    .push(status::missing_callback_port(&url, &host));
            }
            None => {}
        }
    }

    pub fn finish(&mut self) -> std::io::Result<()> {
        if let Some(handle) = self.tunnel_handle.as_mut() {
            handle.stop()?;
        }

        self.tunnel_handle = None;
        Ok(())
    }

    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    pub fn drain_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    fn try_start_tunnel(&mut self, port: u16) {
        let Some(origin) = self.origin.as_deref() else {
            if !self.reported_missing_origin {
                self.messages.push(status::missing_origin());
                self.reported_missing_origin = true;
            }
            return;
        };

        let destination = format_destination(origin, self.user.as_deref());

        match self.launcher.launch(&destination, port) {
            Ok(handle) => {
                self.messages
                    .push(status::starting_tunnel(&destination, port));
                self.tunnel_handle = Some(handle);
            }
            Err(error) => self.messages.push(status::tunnel_launch_failed(&error)),
        }
    }

    #[cfg(test)]
    pub fn run_scripted_for_test(
        launcher: L,
        origin: Option<String>,
        user: Option<String>,
        session: ScriptedSession,
    ) -> RunOutcome {
        let mut runtime = Runtime::new(launcher, origin, user);
        let mut mirrored_output = String::new();

        for chunk in session.output_chunks {
            mirrored_output.push_str(&chunk);
            runtime.on_output(&chunk);
        }

        runtime.finish().unwrap();

        RunOutcome {
            exit_code: session.exit_code,
            mirrored_output,
            messages: runtime.drain_messages(),
        }
    }
}

pub fn run_wrapped_command(config: Config) -> Result<i32, Box<dyn std::error::Error>> {
    let origin = resolve_origin(
        config.origin.as_deref(),
        std::env::var("SSH_CONNECTION").ok().as_deref(),
    )
    .ok();

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize::default())?;

    let mut command = CommandBuilder::new(&config.command[0]);
    if config.command.len() > 1 {
        command.args(&config.command[1..]);
    }

    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);

    let reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let input_gate = InputGate::default();

    let runtime = Arc::new(Mutex::new(Runtime::new(
        PausingTunnelLauncher::new(SshTunnelLauncher, input_gate.clone()),
        origin,
        config.user,
    )));

    let output_runtime = Arc::clone(&runtime);
    let output_thread = thread::spawn(move || mirror_output(reader, output_runtime));
    let _input_thread = thread::spawn(move || forward_input(writer, input_gate));

    let exit_status = child.wait()?;
    let output_result = output_thread
        .join()
        .map_err(|_| io::Error::other("output thread panicked"))?;
    output_result?;

    let trailing_messages = {
        let mut runtime = runtime
            .lock()
            .map_err(|_| io::Error::other("runtime mutex poisoned"))?;
        runtime.finish()?;
        runtime.drain_messages()
    };

    print_status_messages(trailing_messages)?;

    Ok(exit_status.exit_code() as i32)
}

fn mirror_output<L: TunnelLauncher + Send + 'static>(
    mut reader: Box<dyn Read + Send>,
    runtime: Arc<Mutex<Runtime<L>>>,
) -> io::Result<()> {
    let mut stdout = io::stdout();
    let mut buffer = [0u8; 4096];

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                stdout.write_all(&buffer[..count])?;
                stdout.flush()?;

                let text = String::from_utf8_lossy(&buffer[..count]).to_string();
                let messages = {
                    let mut runtime = runtime
                        .lock()
                        .map_err(|_| io::Error::other("runtime mutex poisoned"))?;
                    runtime.on_output(&text);
                    runtime.drain_messages()
                };

                print_status_messages(messages)?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn forward_input(mut writer: Box<dyn Write + Send>, input_gate: InputGate) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let mut stdin = io::stdin();
    let stdin_fd = stdin.as_raw_fd();
    let mut buffer = [0u8; 4096];

    loop {
        if input_gate.is_paused() {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        let mut poll_fd = libc::pollfd {
            fd: stdin_fd,
            events: libc::POLLIN,
            revents: 0,
        };

        let poll_result = unsafe { libc::poll(&mut poll_fd, 1, 100) };
        if poll_result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }

        if poll_result == 0 || input_gate.is_paused() {
            continue;
        }

        match stdin.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                writer.write_all(&buffer[..count])?;
                writer.flush()?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(not(unix))]
fn forward_input(mut writer: Box<dyn Write + Send>, input_gate: InputGate) -> io::Result<()> {
    let mut stdin = io::stdin();
    let mut buffer = [0u8; 4096];

    loop {
        if input_gate.is_paused() {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        match stdin.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                writer.write_all(&buffer[..count])?;
                writer.flush()?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn print_status_messages(messages: Vec<String>) -> io::Result<()> {
    if messages.is_empty() {
        return Ok(());
    }

    let mut stdout = io::stdout();
    for message in messages {
        writeln!(stdout, "[hitch] {message}")?;
    }
    stdout.flush()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io;
    use std::rc::Rc;

    use super::{InputGate, PausingTunnelLauncher, Runtime, ScriptedSession};
    use crate::tunnel::{TunnelHandle, TunnelLauncher};

    #[derive(Debug, Default, Clone)]
    struct LauncherState {
        started_ports: Vec<u16>,
        destinations: Vec<String>,
        stop_count: usize,
    }

    #[derive(Clone)]
    struct FakeTunnelLauncher {
        state: Rc<RefCell<LauncherState>>,
        fail_launch: bool,
    }

    impl FakeTunnelLauncher {
        fn new() -> Self {
            Self {
                state: Rc::new(RefCell::new(LauncherState::default())),
                fail_launch: false,
            }
        }

        fn failing() -> Self {
            Self {
                state: Rc::new(RefCell::new(LauncherState::default())),
                fail_launch: true,
            }
        }

        fn started_ports(&self) -> Vec<u16> {
            self.state.borrow().started_ports.clone()
        }

        fn stop_count(&self) -> usize {
            self.state.borrow().stop_count
        }
    }

    #[derive(Debug, Clone)]
    struct FakeTunnelHandle {
        state: Rc<RefCell<LauncherState>>,
    }

    impl TunnelHandle for FakeTunnelHandle {
        fn stop(&mut self) -> io::Result<()> {
            self.state.borrow_mut().stop_count += 1;
            Ok(())
        }
    }

    impl TunnelLauncher for FakeTunnelLauncher {
        type Handle = FakeTunnelHandle;

        fn launch(&mut self, destination: &str, port: u16) -> io::Result<Self::Handle> {
            if self.fail_launch {
                return Err(io::Error::other("ssh launch failed"));
            }

            let mut state = self.state.borrow_mut();
            state.started_ports.push(port);
            state.destinations.push(destination.to_string());
            Ok(FakeTunnelHandle {
                state: Rc::clone(&self.state),
            })
        }
    }

    #[derive(Clone)]
    struct GateAwareLauncher {
        gate: InputGate,
        saw_paused_gate: Rc<RefCell<bool>>,
    }

    impl TunnelLauncher for GateAwareLauncher {
        type Handle = FakeTunnelHandle;

        fn launch(&mut self, _destination: &str, _port: u16) -> io::Result<Self::Handle> {
            *self.saw_paused_gate.borrow_mut() = self.gate.is_paused();
            Ok(FakeTunnelHandle {
                state: Rc::new(RefCell::new(LauncherState::default())),
            })
        }
    }

    #[test]
    fn starts_tunnel_once_for_first_valid_loopback_url() {
        let launcher = FakeTunnelLauncher::new();
        let mut runtime = Runtime::new(launcher.clone(), Some("203.0.113.10".to_string()), None);

        runtime.on_output("login at http://localhost:8080/callback");
        runtime.on_output("again at http://localhost:9090/callback");

        assert_eq!(launcher.started_ports(), vec![8080]);
        assert!(
            runtime
                .messages()
                .iter()
                .any(|message| message.contains("203.0.113.10"))
        );
    }

    #[test]
    fn reports_missing_origin_without_starting_tunnel() {
        let launcher = FakeTunnelLauncher::new();
        let mut runtime = Runtime::new(launcher.clone(), None, None);

        runtime.on_output("login at http://localhost:8080/callback");

        assert!(launcher.started_ports().is_empty());
        assert!(
            runtime
                .messages()
                .iter()
                .any(|message| message.contains("could not determine tunnel origin"))
        );
    }

    #[test]
    fn warns_for_non_loopback_redirects() {
        let launcher = FakeTunnelLauncher::new();
        let mut runtime = Runtime::new(launcher.clone(), Some("203.0.113.10".to_string()), None);

        runtime.on_output("login at https://example.com/callback");

        assert!(launcher.started_ports().is_empty());
        assert!(
            runtime
                .messages()
                .iter()
                .any(|message| message.contains("does not lead to localhost"))
        );
    }

    #[test]
    fn reports_tunnel_launch_failures_and_keeps_running() {
        let launcher = FakeTunnelLauncher::failing();
        let mut runtime = Runtime::new(launcher, Some("203.0.113.10".to_string()), None);

        runtime.on_output("login at http://localhost:8080/callback");

        assert!(
            runtime
                .messages()
                .iter()
                .any(|message| message.contains("ssh launch failed"))
        );
    }

    #[test]
    fn scripted_run_returns_wrapped_command_exit_status_and_mirrors_output() {
        let launcher = FakeTunnelLauncher::new();
        let outcome = Runtime::run_scripted_for_test(
            launcher.clone(),
            Some("203.0.113.10".to_string()),
            None,
            ScriptedSession {
                output_chunks: vec![
                    "Device login\n".to_string(),
                    "Open http://localhost:7777/callback\n".to_string(),
                ],
                exit_code: 42,
            },
        );

        assert_eq!(outcome.exit_code, 42);
        assert_eq!(
            outcome.mirrored_output,
            "Device login\nOpen http://localhost:7777/callback\n"
        );
        assert_eq!(launcher.started_ports(), vec![7777]);
    }

    #[test]
    fn scripted_run_stops_tunnel_when_wrapped_command_exits() {
        let launcher = FakeTunnelLauncher::new();
        let _outcome = Runtime::run_scripted_for_test(
            launcher.clone(),
            Some("203.0.113.10".to_string()),
            None,
            ScriptedSession {
                output_chunks: vec!["Open http://localhost:7777/callback\n".to_string()],
                exit_code: 0,
            },
        );

        assert_eq!(launcher.stop_count(), 1);
    }

    #[test]
    fn scripted_run_continues_when_tunnel_launch_fails() {
        let launcher = FakeTunnelLauncher::failing();
        let outcome = Runtime::run_scripted_for_test(
            launcher,
            Some("203.0.113.10".to_string()),
            None,
            ScriptedSession {
                output_chunks: vec![
                    "before\n".to_string(),
                    "Open http://localhost:7777/callback\n".to_string(),
                    "after\n".to_string(),
                ],
                exit_code: 7,
            },
        );

        assert_eq!(outcome.exit_code, 7);
        assert_eq!(
            outcome.mirrored_output,
            "before\nOpen http://localhost:7777/callback\nafter\n"
        );
        assert!(
            outcome
                .messages
                .iter()
                .any(|message| message.contains("ssh launch failed"))
        );
    }

    #[test]
    fn pausing_tunnel_launcher_pauses_input_during_launch_and_resumes_after() {
        let gate = InputGate::default();
        let saw_paused_gate = Rc::new(RefCell::new(false));
        let inner = GateAwareLauncher {
            gate: gate.clone(),
            saw_paused_gate: Rc::clone(&saw_paused_gate),
        };
        let mut launcher = PausingTunnelLauncher::new(inner, gate.clone());

        let _handle = launcher.launch("203.0.113.10", 38983).unwrap();

        assert!(*saw_paused_gate.borrow());
        assert!(!gate.is_paused());
    }
}
