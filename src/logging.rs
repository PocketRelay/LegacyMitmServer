use log::LevelFilter;
use log4rs::{
    append::{console::ConsoleAppender, file::FileAppender},
    config::{Appender, Root},
    encode::pattern::PatternEncoder,
    init_config, Config,
};

/// The pattern to use when logging
const LOGGING_PATTERN: &str = "[{d} {h({l})} {M}] {m}{n}";

/// Setup function for setting up the Log4rs logging configuring it
/// for all the different modules and and setting up file and stdout logging
pub fn setup() {
    let pattern = Box::new(PatternEncoder::new(LOGGING_PATTERN));
    let stdout_appender = ConsoleAppender::builder().encoder(pattern.clone()).build();

    let file_appender = FileAppender::builder()
        .encoder(pattern)
        .build("logs/log.log")
        .expect("Unable to create logging file appender");

    const APPENDERS: [&str; 2] = ["stdout", "file"];

    let config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout_appender)))
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .build(
            Root::builder()
                .appenders(APPENDERS)
                .build(LevelFilter::Debug),
        )
        .expect("Failed to create logging config");

    init_config(config).expect("Unable to initialize logger");
}
