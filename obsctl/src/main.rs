use anyhow::{bail, Context, Result};
use open_build_service_api::{Client, PackageCode, ResultListResult};
use oscrc::Oscrc;
use std::path::PathBuf;
use std::time::Duration;
use structopt::StructOpt;
use url::Url;

#[derive(StructOpt, Debug)]
struct Package {
    project: String,
    package: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MonitorData {
    repository: String,
    arch: String,
    code: PackageCode,
}

impl MonitorData {
    fn from_result(r: ResultListResult, package: &str) -> Self {
        let s = r
            .get_status(package)
            .expect("No status for current package");
        let code = if r.dirty {
            PackageCode::Unknown
        } else {
            s.code
        };
        MonitorData {
            repository: r.repository,
            arch: r.arch,
            code,
        }
    }
}

async fn monitor(client: Client, opts: Package) -> Result<()> {
    println!(
        "Monitoring package: {}  project: {}",
        opts.package, opts.project
    );
    let p = client.project(opts.project).package(opts.package.clone());
    let mut last: Vec<MonitorData> = Vec::new();
    loop {
        let result = p.result().await?;
        for r in result.results {
            let data = MonitorData::from_result(r, &opts.package);

            if let Some(old) = last
                .iter_mut()
                .find(|m| m.repository == data.repository && m.arch == data.arch)
            {
                if data.code != PackageCode::Unknown && old.code != data.code {
                    println!(" * {} {} => {}", data.repository, data.arch, data.code);
                    *old = data;
                }
            } else {
                println!("* {} {} => {}", data.repository, data.arch, data.code);
                last.push(data);
            }
        }

        if last.iter().all(|m| m.code.is_final()) {
            break;
        }
        tokio::time::sleep(Duration::from_secs(20)).await;
    }

    if last
        .iter()
        .all(|m| m.code == PackageCode::Excluded || m.code == PackageCode::Disabled)
    {
        bail!("Package excluded/disabled on all repositories/architectures")
    }

    // TODO write out log fiails optionally

    if last.iter().any(|m| m.code == PackageCode::Failed) {
        bail!("Build failure detected!");
    }

    Ok(())
}

#[derive(StructOpt, Debug)]
enum Command {
    Monitor(Package),
}

#[derive(StructOpt)]
struct Opts {
    #[structopt(long, short)]
    apiurl: Option<Url>,
    #[structopt(long, short, default_value = "/home/sjoerd/.oscrc")]
    config: PathBuf,
    #[structopt(long, short, requires("pass"))]
    user: Option<String>,
    #[structopt(long, short, requires("user"))]
    pass: Option<String>,
    #[structopt(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();
    let (url, user, pass) = match opts {
        Opts {
            apiurl: Some(url),
            user: Some(user),
            pass: Some(pass),
            ..
        } => (url, user, pass),
        _ => {
            let oscrc = Oscrc::from_path(&opts.config)
                .with_context(|| format!("Couldn't open {:?}", opts.config))?;
            let url = opts
                .apiurl
                .unwrap_or_else(|| oscrc.default_service().clone());
            let (user, pass) = if let Some(user) = opts.user {
                // If user is set pass should be set as well
                (user, opts.pass.unwrap())
            } else {
                oscrc.credentials(&url)?
            };
            (url, user, pass)
        }
    };

    let client = Client::new(url, user, pass);
    match opts.command {
        Command::Monitor(o) => monitor(client, o).await,
    }
}
