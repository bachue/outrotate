use crate::flags;
use std::io::{BufRead,BufReader,BufWriter,Read,prelude::*};
use std::fs;
use std::fs::{File,OpenOptions};
use std::path::{Path,PathBuf};
use std::process::Child;
use std::os::unix::{fs::MetadataExt,ffi::OsStringExt};
use std::thread;
use std::ffi::OsStr;
use error_chain::error_chain;
use regex::Regex;
use flate2::{GzBuilder,Compression};
use lockfile::Lockfile;
use fs2::FileExt;

error_chain! {
    errors {
        InvalidFileName(b: Vec<u8>) {
            description("filename includes invalid unicode data")
            display("Filename includes invalid unicode data: '{:?}'", b)
        }
        FileLocked(path: PathBuf) {
            description("Log file is locked, maybe there's another outrotate instance is running")
            display("Log file `{}` is locked, maybe there's another outrotate instance is running", path.display())
        }
    }
    foreign_links {
        IOError(std::io::Error);
        RegexpError(regex::Error);
    }
}

pub fn redirect_stdout_stderr<Read1, Read2>(
    f: &flags::Flags,
    mut process: Child,
    stdout: Read1,
    stderr: Option<Read2>,
) -> Result<()>
where
    Read1: Read + Send + 'static,
    Read2: Read + Send + 'static,
{
    let worker1 = LogFileRedirectWorker::new(
        stdout,
        &f.stdout_logfile,
        f.stdout_logfile_max_mb,
        f.stdout_logfile_backups,
        f.compress_stdout_logfile_backups,
    )?
    .spawn();
    let worker2 = match stderr {
        Some(stderr) => Some(
            LogFileRedirectWorker::new(
                stderr,
                f.stderr_logfile.as_ref().unwrap(),
                f.stderr_logfile_max_mb,
                f.stderr_logfile_backups,
                f.compress_stderr_logfile_backups,
            )?
            .spawn(),
        ),
        None => None,
    };
    process.wait()?;
    worker1.join().unwrap();
    if let Some(worker2) = worker2 {
        worker2.join().unwrap();
    }
    Ok(())
}

struct LogFileRedirectWorker<Reader: Read> {
    src: BufReader<Reader>,
    dest: BufWriter<File>,
    dest_file_path: PathBuf,
    dest_file_name: String,
    dest_file_name_matcher: Regex,
    dest_file_gz_name_matcher: Regex,
    dest_dir: PathBuf,
    logfile_size_bytes: u64,
    logfile_max_mb: u64,
    logfile_backups: usize,
    compress_logfile: bool,
}

impl<Reader> LogFileRedirectWorker<Reader>
where
    Reader: Read + Send + 'static,
{
    fn new<AsPath: AsRef<Path>>(
        reader: Reader,
        logfile_path: AsPath,
        logfile_max_mb: u64,
        logfile_backups: usize,
        compress_logfile: bool,
    ) -> Result<LogFileRedirectWorker<Reader>> {
        let dest_file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(logfile_path.as_ref())?;
        if dest_file.try_lock_exclusive().is_err() {
            let _ = fs::remove_file(logfile_path.as_ref());
            return Err(ErrorKind::FileLocked(logfile_path.as_ref().to_path_buf()).into());
        }
        let dest_file_name = Self::convert_osstring_to_string(
            logfile_path.as_ref().file_name().unwrap().to_os_string(),
        )?;
        let dest_file_dir_path = logfile_path.as_ref().parent().unwrap();

        let dest_file_name_matcher_regex = "^".to_string()
            + regex::escape((dest_file_name.clone() + ".").as_str()).as_str()
            + r"(\d+)$";
        let dest_file_gz_name_matcher_regex = "^".to_string()
            + regex::escape((dest_file_name.clone() + ".").as_str()).as_str()
            + r"(\d+)\.gz$";
        let dest_file_name_matcher = Regex::new(&dest_file_name_matcher_regex)?;
        let dest_file_gz_name_matcher = Regex::new(&dest_file_gz_name_matcher_regex)?;

        Ok(LogFileRedirectWorker {
            logfile_size_bytes: dest_file.metadata()?.size(),
            logfile_max_mb: logfile_max_mb,
            logfile_backups: logfile_backups,
            compress_logfile: compress_logfile,
            src: BufReader::new(reader),
            dest: BufWriter::with_capacity(1 << 12, dest_file),
            dest_file_path: logfile_path.as_ref().to_path_buf(),
            dest_file_name: dest_file_name,
            dest_file_name_matcher: dest_file_name_matcher,
            dest_file_gz_name_matcher: dest_file_gz_name_matcher,
            dest_dir: dest_file_dir_path.to_path_buf(),
        })
    }

    fn spawn(self) -> thread::JoinHandle<()> {
        // TODO: use crossbeam instead
        thread::spawn(move || {
            if let Err(err) = self.logworker() {
                eprintln!("Worker Error: {:?}", err);
            }
        })
    }

    fn logworker(mut self) -> Result<()> {
        let logfile_max_size = self.logfile_max_mb * (1 << 20);
        let mut buf = String::with_capacity(1 << 12);
        loop {
            buf.clear();
            let length = self.src.read_line(&mut buf)? as u64;
            if length == 0 {
                break;
            }
            if logfile_max_size > 0 && self.logfile_size_bytes + length > logfile_max_size {
                // TODO: change self.dest here
                // TODO: write to the dest file continueously
                // TODO: rotate logs async
                self.rotatelogs()?;
                let new_log_file = {
                    let file = File::create(&self.dest_file_path)?;
                    file.try_lock_exclusive()?;
                    file
                };
                self.dest = BufWriter::with_capacity(1 << 12, new_log_file);
                self.logfile_size_bytes = 0;
            }
            self.dest.write_all(buf.as_bytes())?;
            self.logfile_size_bytes += length;
        }
        Ok(())
    }

    fn rotatelogs(&mut self) -> Result<()> {
        let lock_file = Lockfile::create(self.dest_dir.join("rotate.lock"));
        if lock_file.is_err() {
            return Ok(());
        }

        let entry_names = {
            let mut file_names = self
                .dest_dir
                .read_dir()?
                .filter_map(|entry_or_error| entry_or_error.ok())
                .filter_map(|entry| Self::convert_osstring_to_string(entry.file_name()).ok())
                .collect::<Vec<String>>();
            file_names.sort_unstable();
            file_names
        };
        let mut suffix_length: Option<usize> = None;
        for file_name in entry_names.into_iter().rev() {

            let (captures, gziped) =
                if let Some(m) = self.dest_file_gz_name_matcher.captures(&file_name) {
                    (m, true)
                } else if let Some(m) = self.dest_file_name_matcher.captures(&file_name) {
                    (m, false)
                } else {
                    continue;
                };

            if let Ok(num) = String::from(&captures[1]).parse::<usize>() {
                if suffix_length.is_none() {
                    suffix_length = Some((num + 1).to_string().len());
                }
                self.rotatelog(file_name, num + 1, suffix_length.unwrap(), gziped)?;
            }
        }
        self.rotatelog(&self.dest_file_name, 1, suffix_length.unwrap_or(1), false)
    }

    fn rotatelog<FileName: AsRef<Path>>(
        &self,
        file_name: FileName,
        new_order_num: usize,
        suffix_length: usize,
        gziped: bool,
    ) -> Result<()> {
        if new_order_num > self.logfile_backups {
            fs::remove_file(self.dest_dir.join(file_name.as_ref()))?;
            return Ok(());
        }

        let new_suffix = Self::format_number(new_order_num, suffix_length);
        let mut new_file_name = self.dest_file_name.clone() + "." + &new_suffix;
        if gziped || self.compress_logfile {
            new_file_name += ".gz"
        }
        if !gziped && self.compress_logfile {
            let tmp_file_path = self.dest_dir.join(new_file_name.clone() + ".tmp");
            fs::rename(self.dest_dir.join(file_name.as_ref()), &tmp_file_path)?;
            Self::gzip_file(&tmp_file_path, self.dest_dir.join(new_file_name))?;
            fs::remove_file(&tmp_file_path)?;
        } else {
            fs::rename(
                self.dest_dir.join(file_name.as_ref()),
                self.dest_dir.join(new_file_name),
            )?;
        }
        Ok(())
    }

    fn gzip_file<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> Result<()> {
        let mut file_reader = File::open(from.as_ref())?;
        let mut gz_encoder = GzBuilder::new()
            .filename(to.as_ref().to_str().unwrap())
            .write(File::create(to.as_ref())?, Compression::best());
        std::io::copy(&mut file_reader, &mut gz_encoder)?;
        gz_encoder.finish()?;
        Ok(())
    }

    fn convert_osstring_to_string<S: AsRef<OsStr>>(s: S) -> Result<String> {
        match s.as_ref().to_str() {
            Some(ss) => Ok(ss.to_string()),
            None => Err(ErrorKind::InvalidFileName(s.as_ref().to_os_string().into_vec()).into()),
        }
    }

    fn format_number(num: usize, size: usize) -> String {
        let mut bytes: Vec<u8> = Vec::with_capacity(size);
        let s = num.to_string();
        let leading_zero_count = if size >= s.len() { size - s.len() } else { 0 };
        bytes.resize(leading_zero_count, 0);
        for &b in s.as_bytes() {
            bytes.push(b);
        }
        return String::from_utf8(bytes).unwrap();
    }
}

impl<Reader> std::fmt::Debug for LogFileRedirectWorker<Reader>
where
    Reader: Read + Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "LogFileRedirectWorker{{dest_file_path:{}, dest_dir:{}, dest_file_name:{}, backups:{}, max_mb:{} size:{}}}",
            self.dest_file_path.display(),
            self.dest_dir.display(),
            self.dest_file_name,
            self.logfile_backups,
            self.logfile_max_mb,
            self.logfile_size_bytes,
        )
    }
}
