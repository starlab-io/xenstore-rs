#[macro_use]
extern crate clap;

use clap::{App, AppSettings, ArgMatches, SubCommand};


fn main() {
    let app_m = App::new("rxenstore")
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::GlobalVersion)
        .setting(AppSettings::SubcommandRequired)
        .global_setting(AppSettings::ColoredHelp)
        .max_term_width(72)
        .subcommand(SubCommand::with_name("ls").about("lists available entries"))
        .get_matches();

    match app_m.subcommand() {
        ("ls", Some(cmd_m)) => ls_cmd(cmd_m),
        _ => unreachable!(),
    }
}

fn ls_cmd(_: &ArgMatches) {
    println!("ls cmd")
}
