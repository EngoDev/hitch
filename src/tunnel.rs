use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, process};

pub trait TunnelHandle {
    fn stop(&mut self) -> io::Result<()>;
}

pub trait TunnelLauncher {
    type Handle: TunnelHandle;

    fn launch(&mut self, destination: &str, port: u16) -> io::Result<Self::Handle>;
}

#[derive(Debug)]
pub struct ProcessTunnelHandle {
    control_dir: PathBuf,
    control_path: PathBuf,
    destination: String,
}

impl ProcessTunnelHandle {
    pub fn new(control_dir: PathBuf, control_path: PathBuf, destination: String) -> Self {
        Self {
            control_dir,
            control_path,
            destination,
        }
    }
}

impl TunnelHandle for ProcessTunnelHandle {
    fn stop(&mut self) -> io::Result<()> {
        let _ = Command::new("ssh")
            .arg("-S")
            .arg(&self.control_path)
            .arg("-O")
            .arg("exit")
            .arg(&self.destination)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        let _ = fs::remove_file(&self.control_path);
        let _ = fs::remove_dir_all(&self.control_dir);
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct SshTunnelLauncher;

impl TunnelLauncher for SshTunnelLauncher {
    type Handle = ProcessTunnelHandle;

    fn launch(&mut self, destination: &str, port: u16) -> io::Result<Self::Handle> {
        let control_dir = create_control_dir()?;
        let control_path = control_dir.join("control.sock");
        let mut args = vec![
            "-o".to_string(),
            "ExitOnForwardFailure=yes".to_string(),
            "-o".to_string(),
            "ControlMaster=yes".to_string(),
            "-o".to_string(),
            format!("ControlPath={}", control_path.display()),
            "-f".to_string(),
        ];
        args.extend(build_reverse_tunnel_args(port, destination));

        let status = Command::new("ssh")
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        if !status.success() {
            let _ = fs::remove_dir_all(&control_dir);
            return Err(io::Error::other(format!("ssh exited with status {status}")));
        }

        Ok(ProcessTunnelHandle::new(
            control_dir,
            control_path,
            destination.to_string(),
        ))
    }
}

pub fn format_destination(origin: &str, user: Option<&str>) -> String {
    match user {
        Some(user) => format!("{user}@{origin}"),
        None => origin.to_string(),
    }
}

pub fn build_reverse_tunnel_args(port: u16, destination: &str) -> Vec<String> {
    vec![
        "-N".to_string(),
        "-R".to_string(),
        format!("{port}:localhost:{port}"),
        destination.to_string(),
    ]
}

fn create_control_dir() -> io::Result<PathBuf> {
    static NEXT_TUNNEL_ID: AtomicU64 = AtomicU64::new(0);

    for _ in 0..32 {
        let tunnel_id = NEXT_TUNNEL_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("hitch-{}-{tunnel_id}", process::id()));

        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::other(
        "could not create a unique temporary directory for the SSH control socket",
    ))
}

#[cfg(test)]
mod tests {
    use super::{build_reverse_tunnel_args, format_destination};

    #[test]
    fn formats_destination_with_explicit_user() {
        let destination = format_destination("10.0.0.5", Some("alice"));

        assert_eq!(destination, "alice@10.0.0.5");
    }

    #[test]
    fn formats_destination_without_user() {
        let destination = format_destination("10.0.0.5", None);

        assert_eq!(destination, "10.0.0.5");
    }

    #[test]
    fn builds_reverse_tunnel_arguments() {
        let args = build_reverse_tunnel_args(4567, "alice@10.0.0.5");

        assert_eq!(
            args,
            vec![
                "-N".to_string(),
                "-R".to_string(),
                "4567:localhost:4567".to_string(),
                "alice@10.0.0.5".to_string(),
            ]
        );
    }
}
