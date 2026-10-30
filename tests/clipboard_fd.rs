#![cfg(target_os = "linux")]

use ptyra::clipboard::{
    read::{ClipboardReceive, ReadProgress},
    write::{ClipboardSend, SendProgress},
};
use ptyra::error::{AppError, Component};
use std::io::ErrorKind;
use std::{
    fs::File,
    io::{Read, Write},
    thread,
};

#[test]
fn clipboard_bytes_reach_real_fd() {
    let text = "繁體\n ".repeat(4_000);
    let (read_fd, write_fd) = nix::unistd::pipe().unwrap();
    let mut writer = File::from(write_fd);
    let mut reader = File::from(read_fd);
    let receiver = thread::spawn(move || {
        let mut received = Vec::new();
        reader.read_to_end(&mut received).unwrap();
        received
    });
    let mut send = ClipboardSend::new(text.clone()).unwrap();
    while send.pending() > 0 {
        assert!(matches!(
            send.write_ready(&mut writer).unwrap(),
            SendProgress::Written(_)
        ));
    }
    assert_eq!(send.write_ready(&mut writer).unwrap(), SendProgress::Done);
    drop(writer);
    assert_eq!(receiver.join().unwrap(), text.as_bytes());
}

#[test]
fn clipboard_utf8_arrives_from_real_fd() {
    let (read_fd, write_fd) = nix::unistd::pipe().unwrap();
    let mut writer = File::from(write_fd);
    let mut reader = File::from(read_fd);
    writer.write_all("繁體\n ".as_bytes()).unwrap();
    drop(writer);
    let mut receive = ClipboardReceive::new();
    loop {
        match receive.read_ready(&mut reader).unwrap() {
            ReadProgress::Read(_) => continue,
            ReadProgress::Complete(text) => {
                assert_eq!(text, "繁體\n ");
                break;
            }
            _ => panic!("unexpected pipe receive state"),
        }
    }
}

#[test]
fn clipboard_real_fd_rejects_oversized_paste() {
    let (read_fd, write_fd) = nix::unistd::pipe().unwrap();
    let mut writer = File::from(write_fd);
    let mut reader = File::from(read_fd);
    writer.write_all("繁體".as_bytes()).unwrap();
    drop(writer);
    let mut receive = ClipboardReceive::with_limit(4).unwrap();
    assert!(matches!(
        receive.read_ready(&mut reader),
        Err(error) if error == AppError::new(Component::Clipboard, ErrorKind::FileTooLarge)
    ));
    assert!(matches!(
        receive.read_ready(&mut reader).unwrap(),
        ReadProgress::Closed
    ));
}
