mod dap;
mod process;
mod project;
mod session;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub cwd: PathBuf,
    /// Build root, relative to the workspace (required for multiple independent builds).
    pub project_root: Option<PathBuf>,
    pub main_class: Option<String>,
    pub module: Option<String>,
    pub java_home: Option<PathBuf>,
    pub jvm_args: Vec<String>,
    pub args: Vec<String>,
    pub classpath: Vec<PathBuf>,
    pub build_args: Vec<String>,
    pub debounce_ms: u64,
    pub jdwp_port: Option<u16>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            cwd: PathBuf::from("."),
            project_root: None,
            main_class: None,
            module: None,
            java_home: None,
            jvm_args: vec![],
            args: vec![],
            classpath: vec![],
            build_args: vec![],
            debounce_ms: 250,
            jdwp_port: None,
        }
    }
}
impl Config {
    fn load(root: PathBuf) -> Result<Self, String> {
        let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
        let path = root.join(".java-hotswap.json");
        let mut config: Self = if path.exists() {
            serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Invalid .java-hotswap.json: {e}"))?
        } else {
            Self::default()
        };
        config.cwd = root;
        if let Some(home) = &config.java_home {
            if !home.is_absolute() {
                config.java_home = Some(config.cwd.join(home));
            }
        }
        Ok(config)
    }
}
#[derive(Parser)]
#[command(version, about = "Automatic Java compilation and safe JVM HotSwap")]
struct Cli {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    Run {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Select a build inside the workspace when multiple builds exist.
        #[arg(long)]
        project_root: Option<PathBuf>,
        #[arg(long)]
        main: Option<String>,
    },
    Stop {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    Clear {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    Inspect {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
    /// Stdio launch adapter used by the Zed extension.
    Dap,
    /// Print global Zed tasks for installation via “zed: open tasks”.
    Tasks,
}
fn main() {
    if let Err(e) = main_result() {
        eprintln!("[Java HotSwap] ERROR {e}");
        std::process::exit(1);
    }
}
fn main_result() -> Result<(), String> {
    let emit: process::Emit = Arc::new(|category, text| {
        use std::io::Write;
        if category == "stdout" {
            let _ = std::io::stdout().write_all(text.as_bytes());
        } else {
            let _ = std::io::stderr().write_all(text.as_bytes());
        }
    });
    match Cli::parse().command {
        Action::Dap => dap::serve(),
        Action::Tasks => {
            println!("{}", include_str!("../../zed-tasks.json"));
            Ok(())
        }
        Action::Stop { project } => control(project, "stop"),
        Action::Clear { project } => control(project, "clear"),
        Action::Run {
            project,
            project_root,
            main,
        } => {
            let mut config = Config::load(project)?;
            if project_root.is_some() {
                config.project_root = project_root;
            }
            if main.is_some() {
                config.main_class = main;
            }
            let stop = Arc::new(AtomicBool::new(false));
            let signal = stop.clone();
            ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst))
                .map_err(|e| e.to_string())?;
            session::run(config, stop, emit)
        }
        Action::Inspect {
            project,
            project_root,
        } => {
            let mut config = Config::load(project)?;
            if project_root.is_some() {
                config.project_root = project_root;
            }
            let state = config.cwd.join(".java-hotswap");
            fs::create_dir_all(&state).map_err(|e| e.to_string())?;
            let _lock = session::lock(&state)?;
            let p = project::Project::discover(&config, &state, &emit, &AtomicBool::new(false))?;
            println!(
                "Build: {}\nRoot: {}\nModules: {:#?}\nEntry: {:?}",
                p.kind,
                p.root.display(),
                p.modules,
                p.entry(&config).map(|(c, m)| (c, m.name.clone()))
            );
            Ok(())
        }
    }
}

fn control(project: PathBuf, request: &str) -> Result<(), String> {
    let state = fs::canonicalize(project)
        .map_err(|e| e.to_string())?
        .join(".java-hotswap");
    if !state.exists() {
        return Err("No HotSwap session has been started in this project".into());
    }
    fs::write(state.join(request), "request").map_err(|e| e.to_string())
}
