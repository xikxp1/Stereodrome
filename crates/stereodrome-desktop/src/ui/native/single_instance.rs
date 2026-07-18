use std::{
    io::{self, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Name, Stream, prelude::*,
};
use log::warn;
use single_instance::SingleInstance;

const IPC_NAME: &str = "dev.xikxp1.stereodrome.gpui";

pub enum AcquireResult {
    Primary(SingleInstanceService, async_channel::Receiver<()>),
    Secondary,
}

pub struct SingleInstanceService {
    _instance: SingleInstance,
    ipc_name: String,
    stop: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
}

impl SingleInstanceService {
    pub fn acquire() -> Result<AcquireResult, String> {
        Self::acquire_named(IPC_NAME)
    }

    fn acquire_named(ipc_name: &str) -> Result<AcquireResult, String> {
        let instance =
            SingleInstance::new(&lock_name(ipc_name)).map_err(|error| error.to_string())?;
        if !instance.is_single() {
            notify_existing(ipc_name)?;
            return Ok(AcquireResult::Secondary);
        }

        let listener = ListenerOptions::new()
            .name(socket_name(ipc_name).map_err(|error| error.to_string())?)
            .try_overwrite(true)
            .create_sync()
            .map_err(|error| format!("Failed to start single-instance listener: {error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let (sender, receiver) = async_channel::unbounded();
        let listener_thread = thread::Builder::new()
            .name("stereodrome-single-instance".to_string())
            .spawn(move || {
                for connection in listener.incoming() {
                    if thread_stop.load(Ordering::Acquire) {
                        break;
                    }
                    match connection {
                        Ok(_) => {
                            if sender.send_blocking(()).is_err() {
                                break;
                            }
                        }
                        Err(error) => warn!("Single-instance IPC accept failed: {error}"),
                    }
                }
            })
            .map_err(|error| format!("Failed to spawn single-instance listener: {error}"))?;

        Ok(AcquireResult::Primary(
            Self {
                _instance: instance,
                ipc_name: ipc_name.to_string(),
                stop,
                listener_thread: Some(listener_thread),
            },
            receiver,
        ))
    }

    pub fn shutdown(&mut self) {
        if self.stop.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Err(error) = wake_listener(&self.ipc_name) {
            warn!("Failed to wake single-instance listener during shutdown: {error}");
        }
        if let Some(thread) = self.listener_thread.take()
            && thread.join().is_err()
        {
            warn!("Single-instance listener panicked during shutdown");
        }
    }
}

impl Drop for SingleInstanceService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lock_name(ipc_name: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        std::env::temp_dir()
            .join(format!("{ipc_name}.lock"))
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(not(target_os = "macos"))]
    format!("{ipc_name}.lock")
}

fn socket_name(ipc_name: &str) -> io::Result<Name<'static>> {
    if GenericNamespaced::is_supported() {
        ipc_name.to_string().to_ns_name::<GenericNamespaced>()
    } else {
        std::env::temp_dir()
            .join(format!("{ipc_name}.sock"))
            .to_fs_name::<GenericFilePath>()
    }
}

fn notify_existing(ipc_name: &str) -> Result<(), String> {
    let mut last_error = None;
    for _ in 0..20 {
        match Stream::connect(socket_name(ipc_name).map_err(|error| error.to_string())?) {
            Ok(mut stream) => {
                stream
                    .write_all(b"show\n")
                    .map_err(|error| format!("Failed to notify the running instance: {error}"))?;
                return Ok(());
            }
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    Err(format!(
        "Another instance is running but could not be notified: {}",
        last_error.map_or_else(
            || "unknown IPC error".to_string(),
            |error| error.to_string()
        )
    ))
}

fn wake_listener(ipc_name: &str) -> io::Result<()> {
    let mut stream = Stream::connect(socket_name(ipc_name)?)?;
    stream.write_all(b"stop\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_instance_notifies_primary() {
        let name = format!("{IPC_NAME}.test.{}", std::process::id());
        let AcquireResult::Primary(mut primary, receiver) =
            SingleInstanceService::acquire_named(&name).unwrap()
        else {
            panic!("first test instance must be primary");
        };

        assert!(matches!(
            SingleInstanceService::acquire_named(&name).unwrap(),
            AcquireResult::Secondary
        ));
        assert_eq!(receiver.recv_blocking(), Ok(()));
        primary.shutdown();
    }
}
