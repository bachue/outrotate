mod flags;
mod cmd;
mod io2logfile;

use error_chain::{error_chain,quick_main};

error_chain! {
    links {
        LogFileError(io2logfile::Error, io2logfile::ErrorKind);
        RunCmdError(cmd::Error, cmd::ErrorKind);
    }
}

quick_main!(|| -> Result<()> {
    let flags = flags::parse_flags();
    let (child, stdout, stderr) = cmd::run_cmd(&flags)?;
    io2logfile::redirect_stdout_stderr(&flags, child, stdout, stderr)?;
    Ok(())
});
