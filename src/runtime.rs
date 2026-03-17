//! Runtime orchestration for wrapped-command execution, direct tunnels, and terminal handling.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::config::{Config, Mode};
use crate::confirm::{
    InquireTunnelConfigEditor, TunnelConfig, TunnelConfigEditor, confirm_tunnel_config,
};
use crate::detect::{DetectionEvent, UrlDetector};
use crate::origin::resolve_origin;
use crate::status;
use crate::tunnel::{SshTunnelLauncher, TunnelHandle, TunnelLauncher, format_destination};

/// Scripted wrapped-command session used by runtime tests.
#[cfg(test)]
pub struct ScriptedSession {
    pub output_chunks: Vec<String>,
    pub exit_code: i32,
}

/// Result captured from a scripted wrapped-command test run.
#[cfg(test)]
pub struct RunOutcome {
    pub exit_code: i32,
    pub mirrored_output: String,
    pub messages: Vec<String>,
}

/// Result captured from a scripted direct-tunnel test run.
#[cfg(test)]
pub struct PortModeOutcome {
    pub exit_code: i32,
    pub messages: Vec<String>,
}

/// Shared pause gate used to temporarily stop stdin forwarding.
#[derive(Clone, Debug, Default)]
pub struct InputGate {
    paused: Arc<AtomicBool>,
}

impl InputGate {
    /// Pauses any input-forwarding loop that consults this gate.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    /// Resumes any input-forwarding loop that consults this gate.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    /// Returns whether forwarding is currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

/// Tunnel launcher wrapper that pauses stdin forwarding while SSH authenticates.
pub struct PausingTunnelLauncher<L> {
    inner: L,
    input_gate: InputGate,
}

impl<L> PausingTunnelLauncher<L> {
    /// Creates a pausing tunnel launcher from an inner launcher and input gate.
    pub fn new(inner: L, input_gate: InputGate) -> Self {
        Self { inner, input_gate }
    }
}

/// Confirmation editor wrapper that pauses stdin forwarding while prompting.
struct PausingTunnelConfigEditor<E> {
    inner: E,
    input_gate: InputGate,
}

impl<E> PausingTunnelConfigEditor<E> {
    /// Creates a pausing confirmation editor from an inner editor and input gate.
    fn new(inner: E, input_gate: InputGate) -> Self {
        Self { inner, input_gate }
    }

    /// Runs a prompt operation while stdin forwarding is paused.
    fn with_paused_input<F, T>(&mut self, operation: F) -> io::Result<T>
    where
        F: FnOnce(&mut E) -> io::Result<T>,
    {
        self.input_gate.pause();
        let result = operation(&mut self.inner);
        self.input_gate.resume();
        result
    }
}

impl<E: TunnelConfigEditor> TunnelConfigEditor for PausingTunnelConfigEditor<E> {
    fn confirm_detected_config(&mut self, config: &TunnelConfig) -> io::Result<bool> {
        self.with_paused_input(|inner| inner.confirm_detected_config(config))
    }

    fn edit_port(&mut self, current: u16) -> io::Result<String> {
        self.with_paused_input(|inner| inner.edit_port(current))
    }

    fn edit_user(&mut self, current: Option<&str>) -> io::Result<String> {
        self.with_paused_input(|inner| inner.edit_user(current))
    }

    fn edit_origin(&mut self, current: &str) -> io::Result<String> {
        self.with_paused_input(|inner| inner.edit_origin(current))
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

/// Abstraction for temporarily changing the parent terminal mode.
trait TerminalModeManager {
    type Guard;

    fn enter_raw_mode(&self) -> io::Result<Self::Guard>;
}

/// Runs an operation while a terminal mode guard is active.
fn with_terminal_mode<M, F, T>(manager: &M, operation: F) -> io::Result<T>
where
    M: TerminalModeManager,
    F: FnOnce() -> io::Result<T>,
{
    let _guard = manager.enter_raw_mode()?;
    operation()
}

/// No-op terminal mode manager used on non-Unix platforms.
#[cfg(not(unix))]
struct NoopTerminalModeManager;

#[cfg(not(unix))]
impl TerminalModeManager for NoopTerminalModeManager {
    type Guard = ();

    fn enter_raw_mode(&self) -> io::Result<Self::Guard> {
        Ok(())
    }
}

/// Unix terminal mode manager that places stdin into raw mode.
#[cfg(unix)]
struct StdinRawModeManager;

/// Guard that restores the original Unix terminal mode on drop.
#[cfg(unix)]
struct StdinRawModeGuard {
    fd: libc::c_int,
    original: libc::termios,
    active: bool,
}

#[cfg(unix)]
impl Drop for StdinRawModeGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.original) };
        }
    }
}

#[cfg(unix)]
impl TerminalModeManager for StdinRawModeManager {
    type Guard = StdinRawModeGuard;

    fn enter_raw_mode(&self) -> io::Result<Self::Guard> {
        use std::mem;
        use std::os::fd::AsRawFd;

        let fd = io::stdin().as_raw_fd();
        if unsafe { libc::isatty(fd) } != 1 {
            return Ok(StdinRawModeGuard {
                fd,
                original: unsafe { mem::zeroed() },
                active: false,
            });
        }

        let mut original = unsafe { mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
            return Err(io::Error::last_os_error());
        }

        let raw = build_raw_terminal_mode(&original);
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(StdinRawModeGuard {
            fd,
            original,
            active: true,
        })
    }
}

/// Builds a raw terminal mode while preserving output flags for normal line rendering.
#[cfg(unix)]
fn build_raw_terminal_mode(original: &libc::termios) -> libc::termios {
    let mut raw = *original;
    unsafe { libc::cfmakeraw(&mut raw) };
    raw.c_oflag = original.c_oflag;
    raw
}

#[cfg(not(unix))]
type StdinRawModeManager = NoopTerminalModeManager;

/// Active wrapped-command runtime state.
pub struct Runtime<L: TunnelLauncher> {
    detector: UrlDetector,
    detection_buffer: String,
    launcher: L,
    editor: Box<dyn TunnelConfigEditor>,
    origin: Option<String>,
    user: Option<String>,
    messages: Vec<String>,
    tunnel_handle: Option<L::Handle>,
    reported_missing_origin: bool,
}

impl<L: TunnelLauncher> Runtime<L> {
    /// Creates a new runtime with the given tunnel launcher, confirmation editor, and defaults.
    pub fn new(
        launcher: L,
        editor: Box<dyn TunnelConfigEditor>,
        origin: Option<String>,
        user: Option<String>,
    ) -> Self {
        Self {
            detector: UrlDetector::default(),
            detection_buffer: String::new(),
            launcher,
            editor,
            origin,
            user,
            messages: Vec::new(),
            tunnel_handle: None,
            reported_missing_origin: false,
        }
    }

    /// Consumes newly mirrored child output and reacts to tunnel-relevant events.
    pub fn on_output(&mut self, text: &str) {
        self.detection_buffer.push_str(text);

        match self.detector.consume(&self.detection_buffer) {
            Some(DetectionEvent::StartTunnel { port, .. }) => self.try_start_tunnel(port),
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

        trim_detection_buffer(&mut self.detection_buffer);
    }

    /// Stops any active tunnel and clears runtime tunnel state.
    pub fn finish(&mut self) -> std::io::Result<()> {
        if let Some(handle) = self.tunnel_handle.as_mut() {
            handle.stop()?;
        }

        self.tunnel_handle = None;
        Ok(())
    }

    /// Returns queued status messages without consuming them.
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Drains and returns queued status messages.
    pub fn drain_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    /// Confirms and launches a reverse tunnel for an auto-detected callback port.
    fn try_start_tunnel(&mut self, port: u16) {
        let Some(origin) = self.origin.as_deref() else {
            if !self.reported_missing_origin {
                self.messages.push(status::missing_origin());
                self.reported_missing_origin = true;
            }
            return;
        };

        let config = TunnelConfig {
            port,
            user: self.user.clone(),
            origin: origin.to_string(),
        };
        let config = match confirm_tunnel_config(self.editor.as_mut(), config) {
            Ok(config) => config,
            Err(error) => {
                self.messages
                    .push(status::tunnel_confirmation_failed(&error));
                return;
            }
        };
        let destination = format_destination(&config.origin, config.user.as_deref());

        match self.launcher.launch(&destination, config.port) {
            Ok(handle) => {
                self.messages
                    .push(status::starting_tunnel(&destination, config.port));
                self.tunnel_handle = Some(handle);
            }
            Err(error) => self.messages.push(status::tunnel_launch_failed(&error)),
        }
    }

    /// Executes a scripted wrapped-command session for tests.
    #[cfg(test)]
    pub fn run_scripted_for_test(
        launcher: L,
        editor: Box<dyn TunnelConfigEditor>,
        origin: Option<String>,
        user: Option<String>,
        session: ScriptedSession,
    ) -> RunOutcome {
        let mut runtime = Runtime::new(launcher, editor, origin, user);
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

    /// Executes scripted direct-tunnel mode for tests.
    #[cfg(test)]
    pub fn run_port_mode_for_test(
        launcher: L,
        origin: Option<String>,
        user: Option<String>,
        port: u16,
        interrupted: bool,
    ) -> PortModeOutcome {
        let exit_code =
            run_port_mode_with_wait(launcher, origin.clone(), user, port, || match interrupted {
                true => Ok(()),
                false => Err(io::Error::other("interrupt was not delivered")),
            })
            .unwrap();

        let messages = match (origin.is_some(), interrupted) {
            (false, _) => vec![status::missing_origin()],
            (true, true) => vec![status::opening_direct_tunnel(port)],
            (true, false) => Vec::new(),
        };

        PortModeOutcome {
            exit_code,
            messages,
        }
    }
}

/// Runs Hitch from a validated runtime configuration.
pub fn run_wrapped_command(config: Config) -> Result<i32, Box<dyn std::error::Error>> {
    match config.mode {
        Mode::Command { command } => run_command_mode(config.origin, config.user, command),
        Mode::Port { port } => run_port_mode(config.origin, config.user, port),
    }
}

/// Runs wrapped-command mode inside a PTY and mirrors the child process I/O.
fn run_command_mode(
    origin_override: Option<String>,
    user: Option<String>,
    wrapped_command: Vec<String>,
) -> Result<i32, Box<dyn std::error::Error>> {
    let origin = resolve_origin(
        origin_override.as_deref(),
        std::env::var("SSH_CONNECTION").ok().as_deref(),
    )
    .ok();
    with_terminal_mode(&StdinRawModeManager, || {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize::default())
            .map_err(|error| io::Error::other(error.to_string()))?;

        let mut command = CommandBuilder::new(&wrapped_command[0]);
        if wrapped_command.len() > 1 {
            command.args(&wrapped_command[1..]);
        }

        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| io::Error::other(error.to_string()))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let input_gate = InputGate::default();

        let runtime = Arc::new(Mutex::new(Runtime::new(
            PausingTunnelLauncher::new(SshTunnelLauncher, input_gate.clone()),
            Box::new(PausingTunnelConfigEditor::new(
                InquireTunnelConfigEditor,
                input_gate.clone(),
            )),
            origin,
            user,
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
    })
    .map_err(Into::into)
}

/// Runs direct `--port` mode and keeps the tunnel open until interrupted.
fn run_port_mode(
    origin_override: Option<String>,
    user: Option<String>,
    port: u16,
) -> Result<i32, Box<dyn std::error::Error>> {
    let origin = resolve_origin(
        origin_override.as_deref(),
        std::env::var("SSH_CONNECTION").ok().as_deref(),
    )
    .ok();

    run_port_mode_with_wait(
        PausingTunnelLauncher::new(SshTunnelLauncher, InputGate::default()),
        origin,
        user,
        port,
        wait_for_ctrl_c,
    )
}

/// Shared direct-tunnel implementation parameterized over the interrupt waiter for tests.
fn run_port_mode_with_wait<L, F>(
    mut launcher: L,
    origin: Option<String>,
    user: Option<String>,
    port: u16,
    wait_for_interrupt: F,
) -> Result<i32, Box<dyn std::error::Error>>
where
    L: TunnelLauncher,
    F: FnOnce() -> io::Result<()>,
{
    let Some(origin) = origin.as_deref() else {
        print_status_messages(vec![status::missing_origin()])?;
        return Ok(1);
    };

    let destination = format_destination(origin, user.as_deref());
    print_status_messages(vec![status::opening_direct_tunnel(port)])?;

    let mut handle = match launcher.launch(&destination, port) {
        Ok(handle) => handle,
        Err(error) => {
            print_status_messages(vec![status::tunnel_launch_failed(&error)])?;
            return Ok(1);
        }
    };

    print_status_messages(vec![status::starting_tunnel(&destination, port)])?;
    wait_for_interrupt()?;
    handle.stop()?;
    Ok(0)
}

/// Blocks until the user interrupts the process with Ctrl+C.
fn wait_for_ctrl_c() -> io::Result<()> {
    let (sender, receiver) = mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = sender.send(());
    })
    .map_err(|error| io::Error::other(error.to_string()))?;

    receiver
        .recv()
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(())
}

/// Mirrors PTY output to stdout while feeding buffered data into the detector.
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

/// Trims the detection carry-over buffer to a bounded size.
fn trim_detection_buffer(buffer: &mut String) {
    const MAX_DETECTION_BUFFER_LEN: usize = 2048;

    if buffer.len() <= MAX_DETECTION_BUFFER_LEN {
        return;
    }

    let split_at = buffer.len() - MAX_DETECTION_BUFFER_LEN;
    let split_at = match buffer.char_indices().find(|(index, _)| *index >= split_at) {
        Some((index, _)) => index,
        None => 0,
    };
    buffer.drain(..split_at);
}

/// Forwards stdin to the wrapped PTY on Unix while honoring the input pause gate.
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

/// Forwards stdin to the wrapped PTY on non-Unix platforms.
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

/// Writes Hitch status messages to stderr.
fn print_status_messages(messages: Vec<String>) -> io::Result<()> {
    let mut stderr = io::stderr();
    print_status_messages_to(&mut stderr, messages)
}

/// Writes Hitch status messages to an arbitrary writer.
fn print_status_messages_to<W: Write>(writer: &mut W, messages: Vec<String>) -> io::Result<()> {
    if messages.is_empty() {
        return Ok(());
    }

    for message in messages {
        writeln!(writer, "[hitch] {message}")?;
    }
    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    #[cfg(unix)]
    use super::build_raw_terminal_mode;
    use super::{
        InputGate, PausingTunnelConfigEditor, PausingTunnelLauncher, Runtime, ScriptedSession,
        TerminalModeManager, print_status_messages_to, with_terminal_mode,
    };
    use crate::confirm::{TunnelConfig, TunnelConfigEditor};
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

        fn destinations(&self) -> Vec<String> {
            self.state.borrow().destinations.clone()
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

    #[derive(Default)]
    struct AcceptingEditor;

    impl TunnelConfigEditor for AcceptingEditor {
        fn confirm_detected_config(&mut self, _config: &TunnelConfig) -> io::Result<bool> {
            Ok(true)
        }

        fn edit_port(&mut self, _current: u16) -> io::Result<String> {
            panic!("unexpected port edit")
        }

        fn edit_user(&mut self, _current: Option<&str>) -> io::Result<String> {
            panic!("unexpected user edit")
        }

        fn edit_origin(&mut self, _current: &str) -> io::Result<String> {
            panic!("unexpected origin edit")
        }
    }

    #[derive(Default)]
    struct EditingEditor;

    impl TunnelConfigEditor for EditingEditor {
        fn confirm_detected_config(&mut self, _config: &TunnelConfig) -> io::Result<bool> {
            Ok(false)
        }

        fn edit_port(&mut self, _current: u16) -> io::Result<String> {
            Ok("4000".into())
        }

        fn edit_user(&mut self, _current: Option<&str>) -> io::Result<String> {
            Ok(String::new())
        }

        fn edit_origin(&mut self, _current: &str) -> io::Result<String> {
            Ok("203.0.113.10".into())
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

    struct GateAwareEditor {
        gate: InputGate,
        saw_paused_gate: Arc<Mutex<bool>>,
    }

    impl TunnelConfigEditor for GateAwareEditor {
        fn confirm_detected_config(&mut self, _config: &TunnelConfig) -> io::Result<bool> {
            *self.saw_paused_gate.lock().unwrap() = self.gate.is_paused();
            Ok(true)
        }

        fn edit_port(&mut self, _current: u16) -> io::Result<String> {
            panic!("unexpected port edit")
        }

        fn edit_user(&mut self, _current: Option<&str>) -> io::Result<String> {
            panic!("unexpected user edit")
        }

        fn edit_origin(&mut self, _current: &str) -> io::Result<String> {
            panic!("unexpected origin edit")
        }
    }

    #[derive(Clone, Default)]
    struct FakeTerminalModeManager {
        state: Rc<RefCell<TerminalModeState>>,
    }

    #[derive(Default)]
    struct TerminalModeState {
        enter_count: usize,
        drop_count: usize,
    }

    struct FakeTerminalModeGuard {
        state: Rc<RefCell<TerminalModeState>>,
    }

    impl Drop for FakeTerminalModeGuard {
        fn drop(&mut self) {
            self.state.borrow_mut().drop_count += 1;
        }
    }

    impl TerminalModeManager for FakeTerminalModeManager {
        type Guard = FakeTerminalModeGuard;

        fn enter_raw_mode(&self) -> io::Result<Self::Guard> {
            self.state.borrow_mut().enter_count += 1;
            Ok(FakeTerminalModeGuard {
                state: Rc::clone(&self.state),
            })
        }
    }

    #[test]
    fn starts_tunnel_once_for_first_valid_loopback_url() {
        let launcher = FakeTunnelLauncher::new();
        let mut runtime = Runtime::new(
            launcher.clone(),
            Box::new(AcceptingEditor),
            Some("203.0.113.10".to_string()),
            None,
        );

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
        let mut runtime = Runtime::new(launcher.clone(), Box::new(AcceptingEditor), None, None);

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
        let mut runtime = Runtime::new(
            launcher.clone(),
            Box::new(AcceptingEditor),
            Some("203.0.113.10".to_string()),
            None,
        );

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
        let mut runtime = Runtime::new(
            launcher,
            Box::new(AcceptingEditor),
            Some("203.0.113.10".to_string()),
            None,
        );

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
            Box::new(AcceptingEditor),
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
            Box::new(AcceptingEditor),
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
            Box::new(AcceptingEditor),
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
    fn detects_callback_url_split_across_output_chunks() {
        let launcher = FakeTunnelLauncher::new();
        let outcome = Runtime::run_scripted_for_test(
            launcher.clone(),
            Box::new(AcceptingEditor),
            Some("203.0.113.10".to_string()),
            None,
            ScriptedSession {
                output_chunks: vec![
                    "Waiting for callback on http://local".to_string(),
                    "host:7777/auth/callback".to_string(),
                ],
                exit_code: 0,
            },
        );

        assert_eq!(launcher.started_ports(), vec![7777]);
        assert_eq!(outcome.exit_code, 0);
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

    #[test]
    fn pausing_tunnel_editor_pauses_input_during_confirmation_and_resumes_after() {
        let gate = InputGate::default();
        let saw_paused_gate = Arc::new(Mutex::new(false));
        let mut editor = PausingTunnelConfigEditor::new(
            GateAwareEditor {
                gate: gate.clone(),
                saw_paused_gate: Arc::clone(&saw_paused_gate),
            },
            gate.clone(),
        );

        let accepted = editor
            .confirm_detected_config(&TunnelConfig {
                port: 3001,
                user: Some("engodev".into()),
                origin: "100.70.126.5".into(),
            })
            .unwrap();

        assert!(accepted);
        assert!(*saw_paused_gate.lock().unwrap());
        assert!(!gate.is_paused());
    }

    #[test]
    fn terminal_mode_is_restored_after_successful_operation() {
        let manager = FakeTerminalModeManager::default();
        let state = Rc::clone(&manager.state);

        let result = with_terminal_mode(&manager, || Ok::<_, io::Error>(42)).unwrap();

        assert_eq!(result, 42);
        assert_eq!(state.borrow().enter_count, 1);
        assert_eq!(state.borrow().drop_count, 1);
    }

    #[test]
    fn terminal_mode_is_restored_after_failed_operation() {
        let manager = FakeTerminalModeManager::default();
        let state = Rc::clone(&manager.state);

        let error =
            with_terminal_mode(&manager, || Err::<(), _>(io::Error::other("boom"))).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(state.borrow().enter_count, 1);
        assert_eq!(state.borrow().drop_count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn raw_terminal_mode_preserves_output_flags() {
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        original.c_oflag = 0o12345;

        let raw = build_raw_terminal_mode(&original);

        assert_eq!(raw.c_oflag, original.c_oflag);
    }

    #[test]
    fn port_mode_starts_one_tunnel_for_requested_port() {
        let launcher = FakeTunnelLauncher::new();
        let outcome = Runtime::run_port_mode_for_test(
            launcher.clone(),
            Some("203.0.113.10".to_string()),
            Some("alice".to_string()),
            38983,
            true,
        );

        assert_eq!(launcher.started_ports(), vec![38983]);
        assert_eq!(launcher.stop_count(), 1);
        assert_eq!(outcome.exit_code, 0);
        assert!(
            outcome
                .messages
                .iter()
                .any(|message| message.contains("38983"))
        );
    }

    #[test]
    fn port_mode_fails_when_origin_is_missing() {
        let launcher = FakeTunnelLauncher::new();
        let outcome = Runtime::run_port_mode_for_test(launcher.clone(), None, None, 38983, true);

        assert!(launcher.started_ports().is_empty());
        assert_ne!(outcome.exit_code, 0);
        assert!(
            outcome
                .messages
                .iter()
                .any(|message| message.contains("could not determine tunnel origin"))
        );
    }

    #[test]
    fn port_mode_stops_tunnel_on_interrupt() {
        let launcher = FakeTunnelLauncher::new();
        let _outcome = Runtime::run_port_mode_for_test(
            launcher.clone(),
            Some("203.0.113.10".to_string()),
            None,
            38983,
            true,
        );

        assert_eq!(launcher.stop_count(), 1);
    }

    #[test]
    fn status_messages_are_written_to_stderr_output() {
        let mut stderr = Vec::new();

        print_status_messages_to(&mut stderr, vec!["hello".to_string()]).unwrap();

        assert_eq!(String::from_utf8(stderr).unwrap(), "[hitch] hello\n");
    }

    #[test]
    fn detected_tunnel_uses_confirmed_values_without_edits() {
        let launcher = FakeTunnelLauncher::new();
        let mut runtime = Runtime::new(
            launcher.clone(),
            Box::new(AcceptingEditor),
            Some("100.70.126.5".to_string()),
            Some("engodev".to_string()),
        );

        runtime.on_output("Waiting for callback on http://localhost:3001/auth/callback");

        assert_eq!(launcher.started_ports(), vec![3001]);
        assert_eq!(
            launcher.destinations(),
            vec!["engodev@100.70.126.5".to_string()]
        );
    }

    #[test]
    fn detected_tunnel_uses_edited_values_before_launch() {
        let launcher = FakeTunnelLauncher::new();
        let mut runtime = Runtime::new(
            launcher.clone(),
            Box::new(EditingEditor),
            Some("100.70.126.5".to_string()),
            Some("engodev".to_string()),
        );

        runtime.on_output("Waiting for callback on http://localhost:3001/auth/callback");

        assert_eq!(launcher.started_ports(), vec![4000]);
        assert_eq!(launcher.destinations(), vec!["203.0.113.10".to_string()]);
    }
}
