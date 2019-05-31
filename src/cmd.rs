use crate::flags;
use std::io::Read;
use std::process::{Child, Command, Stdio};

use error_chain::error_chain;

error_chain! {
    foreign_links {
        SpawnError(std::io::Error);
    }
}

pub fn run_cmd(f: &flags::Flags) -> Result<(Child, impl Read, Option<impl Read>)> {
    let mut command = Command::new(&f.command);
    command.args(&f.command_args).stdin(Stdio::null());
    if f.stderr_logfile.is_some() {
        let (reader1, writer1) = os_pipe::pipe().unwrap();
        let (reader2, writer2) = os_pipe::pipe().unwrap();
        command.stdout(writer1).stderr(writer2);
        let child = command.spawn()?;
        Ok((child, reader1, Some(reader2)))
    } else {
        let (reader, writer) = os_pipe::pipe().unwrap();
        let writer2 = writer.try_clone().unwrap();
        command.stdout(writer).stderr(writer2);
        let child = command.spawn()?;
        Ok((child, reader, None))
    }
}
