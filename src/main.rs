mod changelog;
mod git;
mod github;
mod graphql;
mod markdown;
mod npm;
mod output;
mod package;

use crate::changelog::{Amount, Changelog};
use crate::git::Git;
use crate::npm::NPM;
use crate::output::output;
use crate::output::output_indented;
use clap::{AppSettings, Parser, Subcommand};
use color_eyre::eyre::Result;
use colored::*;
use github::github_info::GitHubInfo;
use markdown::ast::Node;
use markdown::tokens::MarkdownToken;
use package::SemVer;
use std::fmt::Debug;

/// Make CHANGELOG.md changes easier
#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Cli {
    /// The current working directory
    #[clap(long, default_value = ".", global = true)]
    pwd: String,

    /// The changelog filename
    #[clap(short, long, default_value = "CHANGELOG.md", global = true)]
    filename: String,

    /// The subcommand to run
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize a new CHANGELOG.md file, if it doesn't exist yet
    Init,

    /// Add a new entry to the changelog in the "Added" section
    #[clap(setting(AppSettings::ArgRequiredElseHelp))]
    Add {
        /// A link to the commit, pr, issue, ...
        #[clap(conflicts_with = "message")]
        link: Option<String>,

        /// A manual message you want to add
        #[clap(short, long, conflicts_with = "link")]
        message: Option<String>,

        /// The section name to add the entry to
        #[clap(hide = true, default_value = "Added")]
        name: String,
    },

    /// Add a new entry to the changelog in the "Fixed" section
    Fix {
        /// A link to the commit, pr, issue, ...
        #[clap(conflicts_with = "message")]
        link: Option<String>,

        /// A manual message you want to add
        #[clap(short, long, conflicts_with = "link")]
        message: Option<String>,

        /// The section name to add the entry to
        #[clap(hide = true, default_value = "Fixed")]
        name: String,
    },

    /// Add a new entry to the changelog in the "Changed" section
    Change {
        /// A link to the commit, pr, issue, ...
        #[clap(conflicts_with = "message")]
        link: Option<String>,

        /// A manual message you want to add
        #[clap(short, long, conflicts_with = "link")]
        message: Option<String>,

        /// The section name to add the entry to
        #[clap(hide = true, default_value = "Changed")]
        name: String,
    },

    /// Add a new entry to the changelog in the "Deprecated" section
    Deprecate {
        /// A link to the commit, pr, issue, ...
        #[clap(conflicts_with = "message")]
        link: Option<String>,

        /// A manual message you want to add
        #[clap(short, long, conflicts_with = "link")]
        message: Option<String>,

        /// The section name to add the entry to
        #[clap(hide = true, default_value = "Deprecated")]
        name: String,
    },

    /// Add a new entry to the changelog in the "Removed" section
    Remove {
        /// A link to the commit, pr, issue, ...
        #[clap(conflicts_with = "message")]
        link: Option<String>,

        /// A manual message you want to add
        #[clap(short, long, conflicts_with = "link")]
        message: Option<String>,

        /// The section name to add the entry to
        #[clap(hide = true, default_value = "Removed")]
        name: String,
    },

    /// Release a new version
    Release {
        /// The version of the release, which can be one of: "major", "minor", "patch", "infer"
        /// (infer from current package.json version) or an explicit version number like "1.2.3"
        #[clap(default_value = "infer")]
        version: SemVer,

        /// Whether or not to run `npm version <version>` (which in turn updates package.json and
        /// creates a new git tag)
        #[clap(long)]
        with_npm: bool,
    },

    /// Get the release notes of a specific version (or unreleased)
    Notes {
        /// The version you want to get the notes from. Should be a valid semver version or one of
        /// "unreleased" or "latest".
        version: Option<String>,
    },

    /// Get a list of all versions
    List {
        /// Amount of versions to show
        #[clap(short, long, default_value = "10")]
        amount: Amount,

        /// Shorthand for "--amount all"
        #[clap(long, conflicts_with = "amount")]
        all: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Cli::parse();

    let mut changelog = Changelog::new(&args.pwd, &args.filename)?;

    match &args.command {
        Commands::Init => changelog.init(),
        Commands::Add {
            link,
            message,
            name,
        }
        | Commands::Fix {
            link,
            message,
            name,
        }
        | Commands::Change {
            link,
            message,
            name,
        }
        | Commands::Remove {
            link,
            message,
            name,
        }
        | Commands::Deprecate {
            link,
            message,
            name,
        } => {
            changelog.parse_contents()?;

            let message = if let Some(message) = message {
                changelog.add_list_item_to_section(name, message.to_string());
                message.to_string()
            } else if let Some(link) = link {
                let data: GitHubInfo = link.parse().unwrap();
                changelog.add_list_item_to_section(name, data.to_string());
                data.to_string()
            } else {
                output(format!(
                    "No {}, {} or {} provided, run `{}` for more info",
                    "<LINK>".blue().bold(),
                    "<COMMIT HASH>".blue().bold(),
                    "--message".blue().bold(),
                    format!(
                        "changelog {} --help",
                        match &args.command {
                            Commands::Add { .. } => "add",
                            Commands::Fix { .. } => "fix",
                            Commands::Change { .. } => "change",
                            Commands::Remove { .. } => "remove",
                            Commands::Deprecate { .. } => "deprecate",
                            _ => unreachable!(),
                        }
                    )
                    .blue()
                    .bold()
                ));
                std::process::exit(1);
            };

            output(format!(
                "Added a new entry to the {} section:",
                name.blue().bold()
            ));

            if let Some(node) = changelog.get_contents_of_section(&Some("unreleased".to_string())) {
                let text = node.to_string();

                let text = text.replace(
                    &format!("- {}", message),
                    &format!("- {}", message.green().bold()),
                );

                output_indented(text);
                eprintln!()
            }

            changelog.persist()
        }
        Commands::Notes { version } => changelog.parse_contents()?.notes(version),
        Commands::Release { version, with_npm } => {
            output(format!("Releasing {}", version.to_string().green().bold()));
            changelog.parse_contents()?.release(version)?;

            if *with_npm {
                // Commit the CHANGELOG.md file
                Git::new(Some(&args.pwd))?
                    .add(changelog.file_path_str())?
                    .commit("update changelog")?;

                // Execute npm version <version>
                NPM::new(Some(&args.pwd))?.version(version)?;
            }

            Ok(())
        }
        Commands::List { amount, all } => changelog.parse_contents()?.list(amount, all),
    }
}
