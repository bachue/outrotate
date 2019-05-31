use clap::{crate_authors, crate_version, App, AppSettings, Arg};

#[derive(Debug)]
pub struct Flags {
    pub stdout_logfile: String,
    pub stdout_logfile_max_mb: u64,
    pub stdout_logfile_backups: usize,
    pub compress_stdout_logfile_backups: bool,
    pub stderr_logfile: Option<String>,
    pub stderr_logfile_max_mb: u64,
    pub stderr_logfile_backups: usize,
    pub compress_stderr_logfile_backups: bool,
    pub command: String,
    pub command_args: Vec<String>,
}

pub fn parse_flags() -> Flags {
    let matches = App::new("outrotate").about("Rotate your stdout / stderr").author(crate_authors!()).version(crate_version!()).
        setting(AppSettings::AllowExternalSubcommands).
        arg(Arg::with_name("STDOUT_LOGFILE_PATH").required(true).long("stdout-logfile").help("Put process stdout output in this file").takes_value(true)).
        arg(Arg::with_name("STDOUT_LOGFILE_MAX_MB").default_value("0").long("stdout-logfile-max-mb").help("The maximum number of MB that may be consumed by `--stdout-logfile` before it is rotated").takes_value(true)).
        arg(Arg::with_name("STDOUT_LOGFILE_BACKUPS").default_value("0").long("stdout-logfile-backups").help("The number of `--stdout-logfile` backups to keep around resulting from process stdout log file rotation. If set to 0, no backups will be kept.").takes_value(true)).
        arg(Arg::with_name("COMPRESS_STDOUT_LOGFILE_BACKUPS").long("compress-stdout-logfile-backups").help("Compress all `--stdout-logfile` backups by gzip")).
        arg(Arg::with_name("STDERR_LOGFILE_PATH").long("stderr-logfile").help("Put process stderr output in this file").takes_value(true)).
        arg(Arg::with_name("STDERR_LOGFILE_MAX_MB").default_value("0").long("stderr-logfile-max-mb").help("The maximum number of MB that may be consumed by `--stderr-logfile` before it is rotated").takes_value(true)).
        arg(Arg::with_name("STDERR_LOGFILE_BACKUPS").default_value("0").long("stderr-logfile-backups").help("The number of `--stderr-logfile` backups to keep around resulting from process stderr log file rotation. If set to 0, no backups will be kept.").takes_value(true)).
        arg(Arg::with_name("COMPRESS_STDERR_LOGFILE_BACKUPS").long("compress-stderr-logfile-backups").help("Compress all `--stderr-logfile` backups by gzip")).
        get_matches();
    Flags {
        stdout_logfile: matches.value_of("STDOUT_LOGFILE_PATH").unwrap().into(),
        stdout_logfile_max_mb: matches
            .value_of("STDOUT_LOGFILE_MAX_MB")
            .map(|s| {
                s.parse::<u64>()
                    .expect("`--stdout-logfile-max-mb` must pass positive number")
            })
            .unwrap(),
        stdout_logfile_backups: matches
            .value_of("STDOUT_LOGFILE_BACKUPS")
            .map_or(0usize, |s| {
                s.parse::<usize>()
                    .expect("`--stdout-logfile-backups` must pass positive number")
            }),
        compress_stdout_logfile_backups: matches.is_present("COMPRESS_STDOUT_LOGFILE_BACKUPS"),
        stderr_logfile: matches.value_of("STDERR_LOGFILE_PATH").map(|s| s.into()),
        stderr_logfile_max_mb: matches
            .value_of("STDERR_LOGFILE_MAX_MB")
            .map(|s| {
                s.parse::<u64>()
                    .expect("`--stderr-logfile-max-mb` must pass positive number")
            })
            .unwrap(),
        stderr_logfile_backups: matches
            .value_of("STDERR_LOGFILE_BACKUPS")
            .map_or(0usize, |s| {
                s.parse::<usize>()
                    .expect("`--stderr-logfile-backups` must pass positive number")
            }),
        compress_stderr_logfile_backups: matches.is_present("COMPRESS_STDERR_LOGFILE_BACKUPS"),
        command: matches.subcommand().0.into(),
        command_args: matches.subcommand().1.map_or(vec![], |m| {
            m.values_of("")
                .unwrap()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        }),
    }
}
