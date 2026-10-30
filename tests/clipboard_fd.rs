#![cfg(target_os = "linux")]

use ptyra::clipboard::write::{ClipboardSend, SendProgress};
use std::{fs::File, io::Read, thread};

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
