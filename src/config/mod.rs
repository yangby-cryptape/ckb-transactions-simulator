use std::{
    convert::TryFrom, fmt::Display, fs::OpenOptions, io::Read as _, path::PathBuf, str::FromStr,
};

use crate::{
    client::Client,
    error::{Error, Result},
    storage::Storage,
    types::{MetaData, RunEnv},
};

mod init;
mod run;

pub(crate) enum AppConfig {
    Init(InitConfig),
    Run(RunConfig),
}

pub(crate) struct InitConfig {
    pub(crate) storage: Storage,
    pub(crate) config: MetaData,
}

pub(crate) struct RunConfig {
    pub(crate) storage: Storage,
    pub(crate) client: Client,
    pub(crate) config: RunEnv,
}

impl AppConfig {
    pub(crate) fn load() -> Result<Self> {
        let yaml = clap::load_yaml!("cli.yaml");
        let matches = clap::App::from_yaml(yaml)
            .version(clap::crate_version!())
            .author(clap::crate_authors!("\n"))
            .about(clap::crate_description!())
            .get_matches();
        Self::try_from(&matches)
    }

    pub(crate) fn execute(&self) -> Result<()> {
        log::info!("Executing ...");
        match self {
            Self::Init(ref cfg) => cfg.execute(),
            Self::Run(ref cfg) => cfg.execute(),
        }
    }
}

impl<'a> TryFrom<&'a clap::ArgMatches<'a>> for AppConfig {
    type Error = Error;
    fn try_from(matches: &'a clap::ArgMatches) -> Result<Self> {
        match matches.subcommand() {
            ("init", Some(submatches)) => InitConfig::try_from(submatches).map(AppConfig::Init),
            ("run", Some(submatches)) => RunConfig::try_from(submatches).map(AppConfig::Run),
            (subcmd, _) => Err(Error::config(format!("subcommand {}", subcmd))),
        }
    }
}

impl<'a> TryFrom<&'a clap::ArgMatches<'a>> for InitConfig {
    type Error = Error;
    fn try_from(matches: &'a clap::ArgMatches) -> Result<Self> {
        let data_dir = parse_from_str::<PathBuf>(matches, "data-dir")?;
        let config = parse_from_file::<MetaData>(matches, "config")?;
        let storage = Storage::init(data_dir)?;
        Ok(Self { storage, config })
    }
}

impl<'a> TryFrom<&'a clap::ArgMatches<'a>> for RunConfig {
    type Error = Error;
    fn try_from(matches: &'a clap::ArgMatches) -> Result<Self> {
        let data_dir = parse_from_str::<PathBuf>(matches, "data-dir")?;
        let jsonrpc_url = parse_from_str::<url::Url>(matches, "jsonrpc-url")?;
        let config = parse_from_file::<RunEnv>(matches, "config")?;
        let storage = Storage::load(data_dir)?;
        let client = Client::new(&jsonrpc_url)?;
        Ok(Self {
            storage,
            client,
            config,
        })
    }
}

fn parse_from_str<T: FromStr>(matches: &clap::ArgMatches, name: &str) -> Result<T>
where
    <T as FromStr>::Err: Display,
{
    matches
        .value_of(name)
        .map(|index| T::from_str(index).map_err(Error::config))
        .transpose()?
        .ok_or_else(|| Error::argument_should_exist(name))
}

fn parse_from_file<T: FromStr>(matches: &clap::ArgMatches, name: &str) -> Result<T>
where
    <T as FromStr>::Err: Display,
{
    matches
        .value_of(name)
        .map(|file| {
            OpenOptions::new()
                .read(true)
                .open(file)
                .map_err(|err| Error::config(format!("failed to open {} since {}", file, err)))
                .and_then(|mut f| {
                    let mut buffer = String::new();
                    f.read_to_string(&mut buffer)
                        .map_err(|err| {
                            Error::config(format!("failed to read {} since {}", file, err))
                        })
                        .map(|_| buffer)
                })
                .and_then(|data| T::from_str(&data).map_err(Error::config))
        })
        .transpose()?
        .ok_or_else(|| Error::argument_should_exist(name))
}
